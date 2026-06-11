use crate::agents::tool_execution::ToolCallResult;
use crate::agents::Agent;
use goose_tool_runtime::{
    describe_tool_inventory, prepare_tool_dispatch_with_approval_decision,
    tool_visibility_for_policy, PreparedToolInvocation, ToolApprovalDecision,
    ToolDispatchPreparationError, ToolInventoryVisibility, ToolInvocation, ToolRuntimePolicy,
    ToolRuntimePolicyProvider, ToolRuntimeSession, ToolVisibility,
};
use rmcp::model::{CallToolRequestParams, ErrorCode, ErrorData, Tool};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub struct GooseToolRuntimeAdapter {
    agent: Arc<Agent>,
    policy_provider: Arc<dyn ToolRuntimePolicyProvider>,
}

impl GooseToolRuntimeAdapter {
    pub fn new(agent: Arc<Agent>, policy_provider: Arc<dyn ToolRuntimePolicyProvider>) -> Self {
        Self {
            agent,
            policy_provider,
        }
    }

    pub async fn list_tools(&self, session: &ToolRuntimeSession) -> Vec<Tool> {
        let policy = self
            .policy_provider
            .policy_for_session(session.external_session_id)
            .await;
        let tools = self
            .agent
            .list_tools(&session.runtime_session_id, None)
            .await;
        filter_tools_for_policy(tools, &policy)
    }

    pub async fn describe_tool_inventory(
        &self,
        session: &ToolRuntimeSession,
    ) -> ToolInventoryVisibility {
        let policy = self
            .policy_provider
            .policy_for_session(session.external_session_id)
            .await;
        let tools = self
            .agent
            .list_tools(&session.runtime_session_id, None)
            .await;

        describe_tools_for_policy(&tools, &policy)
    }

    pub async fn dispatch_tool_call(
        &self,
        session: &ToolRuntimeSession,
        invocation: ToolInvocation,
        cancellation_token: Option<CancellationToken>,
    ) -> (String, Result<ToolCallResult, ErrorData>) {
        self.dispatch_tool_call_with_approval_decision(
            session,
            invocation,
            None,
            cancellation_token,
        )
        .await
    }

    pub async fn dispatch_tool_call_with_approval_decision(
        &self,
        session: &ToolRuntimeSession,
        invocation: ToolInvocation,
        approval_decision: Option<ToolApprovalDecision>,
        cancellation_token: Option<CancellationToken>,
    ) -> (String, Result<ToolCallResult, ErrorData>) {
        let request_id = invocation
            .id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let policy = self
            .policy_provider
            .policy_for_session(session.external_session_id)
            .await;

        let mut goose_session = match self
            .agent
            .config
            .session_manager
            .get_session(&session.runtime_session_id, false)
            .await
        {
            Ok(session) => session,
            Err(error) => {
                return (
                    request_id,
                    Err(ErrorData::new(
                        ErrorCode::INTERNAL_ERROR,
                        error.to_string(),
                        None,
                    )),
                )
            }
        };

        let prepared = match prepare_tool_dispatch_with_approval_decision(
            &policy,
            session,
            invocation,
            request_id.clone(),
            &goose_session.working_dir,
            approval_decision.as_ref(),
        ) {
            Ok(prepared) => prepared,
            Err(error) => {
                return (
                    request_id,
                    Err(dispatch_preparation_error_to_error_data(error)),
                )
            }
        };

        goose_session.working_dir = prepared.working_dir.clone();
        let tool_call = prepared_invocation_to_call_tool_params(prepared);

        self.agent
            .dispatch_tool_call(tool_call, request_id, cancellation_token, &goose_session)
            .await
    }
}

pub fn filter_tools_for_policy(tools: Vec<Tool>, policy: &ToolRuntimePolicy) -> Vec<Tool> {
    tools
        .into_iter()
        .filter(|tool| tool_visibility_for_policy(policy, tool.name.as_ref()).visible)
        .collect()
}

pub fn describe_tools_for_policy(
    tools: &[Tool],
    policy: &ToolRuntimePolicy,
) -> ToolInventoryVisibility {
    describe_tool_inventory(policy, tools.iter().map(|tool| tool.name.as_ref()))
}

pub fn tool_visibility_for_rmcp_tool(tool: &Tool, policy: &ToolRuntimePolicy) -> ToolVisibility {
    tool_visibility_for_policy(policy, tool.name.as_ref())
}

fn prepared_invocation_to_call_tool_params(
    prepared: PreparedToolInvocation,
) -> CallToolRequestParams {
    let mut params = CallToolRequestParams::new(prepared.name);
    if let Some(arguments) = prepared.arguments {
        params = params.with_arguments(arguments);
    }
    params
}

fn dispatch_preparation_error_to_error_data(error: ToolDispatchPreparationError) -> ErrorData {
    match error {
        ToolDispatchPreparationError::Policy(error) => {
            ErrorData::new(ErrorCode::INVALID_REQUEST, error.to_string(), None)
        }
        ToolDispatchPreparationError::Arguments(error) => {
            ErrorData::new(ErrorCode::INVALID_PARAMS, error.to_string(), None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use goose_session_runtime::ExternalSessionId;
    use goose_tool_runtime::{
        ToolAvailability, ToolInvocationArgumentError, ToolRuntimePolicyError, ToolVisibilityReason,
    };
    use std::path::PathBuf;

    fn policy(available_tools: ToolAvailability) -> ToolRuntimePolicy {
        ToolRuntimePolicy {
            external_session_id: ExternalSessionId::new_v4(),
            workspace_bindings: Vec::new(),
            available_tools,
            approval_mode: goose_tool_runtime::ToolApprovalMode::Ask,
        }
    }

    fn tool(name: &str) -> Tool {
        let schema = serde_json::json!({ "type": "object" })
            .as_object()
            .unwrap()
            .clone();
        Tool::new(name.to_string(), "test tool", schema)
    }

    #[test]
    fn denies_unavailable_tools_before_dispatch() {
        let error = dispatch_preparation_error_to_error_data(ToolDispatchPreparationError::Policy(
            ToolRuntimePolicyError::NotAllowed {
                tool_name: "developer__shell".to_string(),
            },
        ));

        assert_eq!(error.code, ErrorCode::INVALID_REQUEST);
        assert!(error.message.contains("not allowed"));
    }

    #[test]
    fn ask_mode_requires_host_approval_before_dispatch() {
        let error = dispatch_preparation_error_to_error_data(ToolDispatchPreparationError::Policy(
            ToolRuntimePolicyError::RequiresApproval {
                tool_name: "developer__shell".to_string(),
            },
        ));

        assert_eq!(error.code, ErrorCode::INVALID_REQUEST);
        assert!(error.message.contains("requires host approval"));
    }

    #[test]
    fn host_denial_maps_to_invalid_request_before_dispatch() {
        let error = dispatch_preparation_error_to_error_data(ToolDispatchPreparationError::Policy(
            ToolRuntimePolicyError::Denied {
                tool_name: "developer__shell".to_string(),
            },
        ));

        assert_eq!(error.code, ErrorCode::INVALID_REQUEST);
        assert!(error.message.contains("was denied"));
    }

    #[test]
    fn converts_prepared_invocation_to_rmcp_call() {
        let tool_call = prepared_invocation_to_call_tool_params(PreparedToolInvocation {
            request_id: "request-1".to_string(),
            name: "developer__shell".to_string(),
            arguments: Some(serde_json::Map::from_iter([(
                "command".to_string(),
                serde_json::json!("pwd"),
            )])),
            working_dir: PathBuf::from("/tmp/alice"),
        });

        assert_eq!(tool_call.name, "developer__shell");
        assert_eq!(
            tool_call.arguments.unwrap().get("command").unwrap(),
            &serde_json::json!("pwd")
        );
    }

    #[test]
    fn rejects_non_object_tool_arguments_before_dispatch() {
        let error =
            dispatch_preparation_error_to_error_data(ToolDispatchPreparationError::Arguments(
                ToolInvocationArgumentError::new("Tool arguments must be a JSON object."),
            ));

        assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
        assert!(error.message.contains("JSON object"));
    }

    #[test]
    fn filters_rmcp_tools_with_runtime_visibility_policy() {
        let policy = policy(ToolAvailability::Only(vec!["developer__read".to_string()]));
        let tools = vec![tool("developer__read"), tool("developer__shell")];

        let filtered = filter_tools_for_policy(tools, &policy);

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name.as_ref(), "developer__read");
    }

    #[test]
    fn describes_rmcp_tool_inventory_with_block_reasons() {
        let policy = policy(ToolAvailability::Only(vec![
            "developer__read".to_string(),
            "developer__shell".to_string(),
        ]));
        let tools = vec![tool("developer__read"), tool("developer__write")];

        let inventory = describe_tools_for_policy(&tools, &policy);

        assert_eq!(inventory.tools.len(), 2);
        assert_eq!(inventory.tools[0].name, "developer__read");
        assert!(inventory.tools[0].visible);
        assert_eq!(inventory.tools[0].reason, ToolVisibilityReason::Available);
        assert_eq!(inventory.tools[1].name, "developer__write");
        assert!(!inventory.tools[1].visible);
        assert_eq!(
            inventory.tools[1].reason,
            ToolVisibilityReason::NotAllowedByPolicy
        );
        assert_eq!(inventory.missing_allowed_tools, vec!["developer__shell"]);
    }
}
