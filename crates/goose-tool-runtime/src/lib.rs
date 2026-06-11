use async_trait::async_trait;
use goose_session_runtime::ExternalSessionId;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt;
use std::path::{Path, PathBuf};

pub type ToolInvocationArguments = serde_json::Map<String, serde_json::Value>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolRuntimeSession {
    pub external_session_id: ExternalSessionId,
    pub runtime_session_id: String,
    pub working_dir: Option<PathBuf>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolInvocation {
    pub id: Option<String>,
    pub name: String,
    pub arguments: Option<serde_json::Value>,
}

impl ToolInvocation {
    pub fn request_id(&self) -> String {
        self.id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
    }

    pub fn take_argument_object(
        &mut self,
    ) -> Result<Option<ToolInvocationArguments>, ToolInvocationArgumentError> {
        take_invocation_argument_object(self.arguments.take())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PreparedToolInvocation {
    pub request_id: String,
    pub name: String,
    pub arguments: Option<ToolInvocationArguments>,
    pub working_dir: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolRuntimePolicy {
    pub external_session_id: ExternalSessionId,
    pub workspace_bindings: Vec<WorkspaceBinding>,
    pub available_tools: ToolAvailability,
    pub approval_mode: ToolApprovalMode,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkspaceBinding {
    pub owner_id: String,
    pub local_path: PathBuf,
    pub mount_name: String,
    pub writable: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolAvailability {
    All,
    Only(Vec<String>),
    None,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolApprovalMode {
    Auto,
    Ask,
    Deny,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolApprovalDecision {
    Approved,
    Denied { reason: Option<String> },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ToolInventoryVisibility {
    pub tools: Vec<ToolVisibility>,
    pub missing_allowed_tools: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ToolVisibility {
    pub name: String,
    pub visible: bool,
    pub reason: ToolVisibilityReason,
    pub approval_mode: ToolApprovalMode,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolVisibilityReason {
    Available,
    UnavailableByPolicy,
    NotAllowedByPolicy,
}

#[async_trait]
pub trait ToolRuntimePolicyProvider: Send + Sync {
    async fn policy_for_session(&self, external_session_id: ExternalSessionId)
        -> ToolRuntimePolicy;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ToolRuntimePolicyError {
    Unavailable { tool_name: String },
    NotAllowed { tool_name: String },
    RequiresApproval { tool_name: String },
    Denied { tool_name: String },
}

impl fmt::Display for ToolRuntimePolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ToolRuntimePolicyError::Unavailable { tool_name } => {
                write!(f, "Tool `{tool_name}` is not available in this session.")
            }
            ToolRuntimePolicyError::NotAllowed { tool_name } => {
                write!(f, "Tool `{tool_name}` is not allowed in this session.")
            }
            ToolRuntimePolicyError::RequiresApproval { tool_name } => write!(
                f,
                "Tool `{tool_name}` requires host approval before dispatch."
            ),
            ToolRuntimePolicyError::Denied { tool_name } => {
                write!(
                    f,
                    "Tool `{tool_name}` was denied by the session tool policy."
                )
            }
        }
    }
}

impl std::error::Error for ToolRuntimePolicyError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ToolDispatchPreparationError {
    Policy(ToolRuntimePolicyError),
    Arguments(ToolInvocationArgumentError),
}

impl fmt::Display for ToolDispatchPreparationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ToolDispatchPreparationError::Policy(error) => write!(f, "{error}"),
            ToolDispatchPreparationError::Arguments(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ToolDispatchPreparationError {}

impl From<ToolRuntimePolicyError> for ToolDispatchPreparationError {
    fn from(error: ToolRuntimePolicyError) -> Self {
        Self::Policy(error)
    }
}

impl From<ToolInvocationArgumentError> for ToolDispatchPreparationError {
    fn from(error: ToolInvocationArgumentError) -> Self {
        Self::Arguments(error)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolInvocationArgumentError {
    pub message: String,
}

impl ToolInvocationArgumentError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for ToolInvocationArgumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ToolInvocationArgumentError {}

pub fn take_invocation_argument_object(
    arguments: Option<serde_json::Value>,
) -> Result<Option<ToolInvocationArguments>, ToolInvocationArgumentError> {
    match arguments {
        Some(serde_json::Value::Object(arguments)) => Ok(Some(arguments)),
        Some(_) => Err(ToolInvocationArgumentError::new(
            "Tool arguments must be a JSON object.",
        )),
        None => Ok(None),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolRegistryPolicyViolation {
    pub missing_allowed: Vec<String>,
    pub disallowed_active: Vec<String>,
}

impl ToolRegistryPolicyViolation {
    pub fn is_empty(&self) -> bool {
        self.missing_allowed.is_empty() && self.disallowed_active.is_empty()
    }
}

impl fmt::Display for ToolRegistryPolicyViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.missing_allowed.is_empty() {
            write!(
                f,
                "tool policy cannot allow unavailable tools: {}",
                self.missing_allowed.join(", ")
            )?;
        }

        if !self.disallowed_active.is_empty() {
            if !self.missing_allowed.is_empty() {
                write!(f, "; ")?;
            }
            write!(
                f,
                "tool policy requires selective filtering, but active disallowed tools are still registered: {}",
                self.disallowed_active.join(", ")
            )?;
        }

        Ok(())
    }
}

impl std::error::Error for ToolRegistryPolicyViolation {}

pub fn is_tool_available(policy: &ToolRuntimePolicy, tool_name: &str) -> bool {
    match &policy.available_tools {
        ToolAvailability::All => true,
        ToolAvailability::None => false,
        ToolAvailability::Only(allowed) => allowed.iter().any(|name| name == tool_name),
    }
}

pub fn ensure_tool_allowed(
    policy: &ToolRuntimePolicy,
    tool_name: &str,
) -> Result<(), ToolRuntimePolicyError> {
    match &policy.available_tools {
        ToolAvailability::All => Ok(()),
        ToolAvailability::None => Err(ToolRuntimePolicyError::Unavailable {
            tool_name: tool_name.to_string(),
        }),
        ToolAvailability::Only(allowed) if allowed.iter().any(|name| name == tool_name) => Ok(()),
        ToolAvailability::Only(_) => Err(ToolRuntimePolicyError::NotAllowed {
            tool_name: tool_name.to_string(),
        }),
    }
}

pub fn tool_visibility_for_policy(policy: &ToolRuntimePolicy, tool_name: &str) -> ToolVisibility {
    let (visible, reason) = match &policy.available_tools {
        ToolAvailability::All => (true, ToolVisibilityReason::Available),
        ToolAvailability::None => (false, ToolVisibilityReason::UnavailableByPolicy),
        ToolAvailability::Only(allowed) if allowed.iter().any(|name| name == tool_name) => {
            (true, ToolVisibilityReason::Available)
        }
        ToolAvailability::Only(_) => (false, ToolVisibilityReason::NotAllowedByPolicy),
    };

    ToolVisibility {
        name: tool_name.to_string(),
        visible,
        reason,
        approval_mode: policy.approval_mode.clone(),
    }
}

pub fn describe_tool_inventory<I, S>(
    policy: &ToolRuntimePolicy,
    active_tools: I,
) -> ToolInventoryVisibility
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let active_tool_names = active_tools
        .into_iter()
        .map(|tool| tool.as_ref().to_string())
        .collect::<Vec<_>>();
    let active_tool_set = active_tool_names.iter().cloned().collect::<HashSet<_>>();

    let tools = active_tool_names
        .iter()
        .map(|tool_name| tool_visibility_for_policy(policy, tool_name))
        .collect();

    let mut missing_allowed_tools = match &policy.available_tools {
        ToolAvailability::Only(allowed_tools) => allowed_tools
            .iter()
            .filter(|tool_name| !active_tool_set.contains(*tool_name))
            .cloned()
            .collect::<Vec<_>>(),
        ToolAvailability::All | ToolAvailability::None => Vec::new(),
    };
    missing_allowed_tools.sort();

    ToolInventoryVisibility {
        tools,
        missing_allowed_tools,
    }
}

pub fn validate_active_tools_for_policy<I, S>(
    policy: &ToolRuntimePolicy,
    active_tools: I,
) -> Result<(), ToolRegistryPolicyViolation>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let ToolAvailability::Only(allowed_tools) = &policy.available_tools else {
        return Ok(());
    };

    let active_tool_names = active_tools
        .into_iter()
        .map(|tool| tool.as_ref().to_string())
        .collect::<HashSet<_>>();
    let allowed_tool_names = allowed_tools.iter().cloned().collect::<HashSet<_>>();

    let mut missing_allowed = allowed_tool_names
        .difference(&active_tool_names)
        .cloned()
        .collect::<Vec<_>>();
    missing_allowed.sort();

    let mut disallowed_active = active_tool_names
        .difference(&allowed_tool_names)
        .cloned()
        .collect::<Vec<_>>();
    disallowed_active.sort();

    let violation = ToolRegistryPolicyViolation {
        missing_allowed,
        disallowed_active,
    };

    if violation.is_empty() {
        Ok(())
    } else {
        Err(violation)
    }
}

pub fn ensure_dispatch_approved(
    policy: &ToolRuntimePolicy,
    tool_name: &str,
) -> Result<(), ToolRuntimePolicyError> {
    ensure_dispatch_approved_with_decision(policy, tool_name, None)
}

pub fn ensure_dispatch_approved_with_decision(
    policy: &ToolRuntimePolicy,
    tool_name: &str,
    decision: Option<&ToolApprovalDecision>,
) -> Result<(), ToolRuntimePolicyError> {
    match policy.approval_mode {
        ToolApprovalMode::Auto => Ok(()),
        ToolApprovalMode::Ask => match decision {
            Some(ToolApprovalDecision::Approved) => Ok(()),
            Some(ToolApprovalDecision::Denied { .. }) => Err(ToolRuntimePolicyError::Denied {
                tool_name: tool_name.to_string(),
            }),
            None => Err(ToolRuntimePolicyError::RequiresApproval {
                tool_name: tool_name.to_string(),
            }),
        },
        ToolApprovalMode::Deny => Err(ToolRuntimePolicyError::Denied {
            tool_name: tool_name.to_string(),
        }),
    }
}

pub fn authorize_tool_dispatch(
    policy: &ToolRuntimePolicy,
    tool_name: &str,
) -> Result<(), ToolRuntimePolicyError> {
    authorize_tool_dispatch_with_decision(policy, tool_name, None)
}

pub fn authorize_tool_dispatch_with_decision(
    policy: &ToolRuntimePolicy,
    tool_name: &str,
    decision: Option<&ToolApprovalDecision>,
) -> Result<(), ToolRuntimePolicyError> {
    ensure_tool_allowed(policy, tool_name)?;
    ensure_dispatch_approved_with_decision(policy, tool_name, decision)
}

pub fn prepare_tool_dispatch(
    policy: &ToolRuntimePolicy,
    session: &ToolRuntimeSession,
    invocation: ToolInvocation,
    fallback_working_dir: &Path,
) -> Result<PreparedToolInvocation, ToolDispatchPreparationError> {
    let request_id = invocation.request_id();
    prepare_tool_dispatch_with_request_id(
        policy,
        session,
        invocation,
        request_id,
        fallback_working_dir,
    )
}

pub fn prepare_tool_dispatch_with_request_id(
    policy: &ToolRuntimePolicy,
    session: &ToolRuntimeSession,
    invocation: ToolInvocation,
    request_id: String,
    fallback_working_dir: &Path,
) -> Result<PreparedToolInvocation, ToolDispatchPreparationError> {
    prepare_tool_dispatch_with_approval_decision(
        policy,
        session,
        invocation,
        request_id,
        fallback_working_dir,
        None,
    )
}

pub fn prepare_tool_dispatch_with_approval_decision(
    policy: &ToolRuntimePolicy,
    session: &ToolRuntimeSession,
    mut invocation: ToolInvocation,
    request_id: String,
    fallback_working_dir: &Path,
    decision: Option<&ToolApprovalDecision>,
) -> Result<PreparedToolInvocation, ToolDispatchPreparationError> {
    authorize_tool_dispatch_with_decision(policy, &invocation.name, decision)?;
    let arguments = invocation.take_argument_object()?;
    let working_dir = select_working_dir(
        session.working_dir.as_deref(),
        &policy.workspace_bindings,
        fallback_working_dir,
    );

    Ok(PreparedToolInvocation {
        request_id,
        name: invocation.name,
        arguments,
        working_dir,
    })
}

pub fn select_working_dir(
    requested: Option<&Path>,
    bindings: &[WorkspaceBinding],
    fallback: &Path,
) -> PathBuf {
    if let Some(requested) = requested {
        if bindings.is_empty()
            || bindings
                .iter()
                .any(|binding| binding.local_path == requested)
        {
            return requested.to_path_buf();
        }
    }

    bindings
        .iter()
        .find(|binding| binding.writable)
        .or_else(|| bindings.first())
        .map(|binding| binding.local_path.clone())
        .unwrap_or_else(|| fallback.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(
        available_tools: ToolAvailability,
        approval_mode: ToolApprovalMode,
    ) -> ToolRuntimePolicy {
        ToolRuntimePolicy {
            external_session_id: ExternalSessionId::new_v4(),
            workspace_bindings: Vec::new(),
            available_tools,
            approval_mode,
        }
    }

    #[test]
    fn denies_unavailable_tools() {
        let policy = policy(ToolAvailability::None, ToolApprovalMode::Auto);

        let error = ensure_tool_allowed(&policy, "developer__shell").unwrap_err();

        assert_eq!(
            error,
            ToolRuntimePolicyError::Unavailable {
                tool_name: "developer__shell".to_string()
            }
        );
    }

    #[test]
    fn denies_tools_missing_from_allowlist() {
        let policy = policy(
            ToolAvailability::Only(vec!["developer__read".to_string()]),
            ToolApprovalMode::Auto,
        );

        let error = ensure_tool_allowed(&policy, "developer__shell").unwrap_err();

        assert_eq!(
            error,
            ToolRuntimePolicyError::NotAllowed {
                tool_name: "developer__shell".to_string()
            }
        );
    }

    #[test]
    fn ask_mode_requires_host_approval() {
        let policy = policy(ToolAvailability::All, ToolApprovalMode::Ask);

        let error = ensure_dispatch_approved(&policy, "developer__shell").unwrap_err();

        assert_eq!(
            error,
            ToolRuntimePolicyError::RequiresApproval {
                tool_name: "developer__shell".to_string()
            }
        );
    }

    #[test]
    fn ask_mode_can_be_approved_by_host_decision() {
        let policy = policy(ToolAvailability::All, ToolApprovalMode::Ask);

        let result = authorize_tool_dispatch_with_decision(
            &policy,
            "developer__shell",
            Some(&ToolApprovalDecision::Approved),
        );

        assert!(result.is_ok());
    }

    #[test]
    fn ask_mode_can_be_denied_by_host_decision() {
        let policy = policy(ToolAvailability::All, ToolApprovalMode::Ask);

        let error = authorize_tool_dispatch_with_decision(
            &policy,
            "developer__shell",
            Some(&ToolApprovalDecision::Denied {
                reason: Some("too risky".to_string()),
            }),
        )
        .unwrap_err();

        assert_eq!(
            error,
            ToolRuntimePolicyError::Denied {
                tool_name: "developer__shell".to_string()
            }
        );
    }

    #[test]
    fn authorizes_allowed_auto_dispatch() {
        let policy = policy(
            ToolAvailability::Only(vec!["developer__shell".to_string()]),
            ToolApprovalMode::Auto,
        );

        let result = authorize_tool_dispatch(&policy, "developer__shell");

        assert!(result.is_ok());
    }

    #[test]
    fn prepares_tool_dispatch_with_bound_workspace() {
        let requested = PathBuf::from("/tmp/alice");
        let fallback = PathBuf::from("/tmp/fallback");
        let mut policy = policy(ToolAvailability::All, ToolApprovalMode::Auto);
        policy.workspace_bindings.push(WorkspaceBinding {
            owner_id: "alice".to_string(),
            local_path: requested.clone(),
            mount_name: "alice".to_string(),
            writable: true,
        });
        let session = ToolRuntimeSession {
            external_session_id: policy.external_session_id,
            runtime_session_id: "goose-session-1".to_string(),
            working_dir: Some(requested.clone()),
        };

        let prepared = prepare_tool_dispatch_with_request_id(
            &policy,
            &session,
            ToolInvocation {
                id: None,
                name: "developer__shell".to_string(),
                arguments: Some(serde_json::json!({ "command": "pwd" })),
            },
            "request-1".to_string(),
            &fallback,
        )
        .unwrap();

        assert_eq!(prepared.request_id, "request-1");
        assert_eq!(prepared.name, "developer__shell");
        assert_eq!(prepared.working_dir, requested);
        assert_eq!(
            prepared.arguments.unwrap().get("command").unwrap(),
            &serde_json::json!("pwd")
        );
    }

    #[test]
    fn prepares_ask_mode_dispatch_after_host_approval() {
        let fallback = PathBuf::from("/tmp/fallback");
        let policy = policy(ToolAvailability::All, ToolApprovalMode::Ask);
        let session = ToolRuntimeSession {
            external_session_id: policy.external_session_id,
            runtime_session_id: "goose-session-1".to_string(),
            working_dir: None,
        };

        let prepared = prepare_tool_dispatch_with_approval_decision(
            &policy,
            &session,
            ToolInvocation {
                id: None,
                name: "developer__shell".to_string(),
                arguments: Some(serde_json::json!({ "command": "pwd" })),
            },
            "request-1".to_string(),
            &fallback,
            Some(&ToolApprovalDecision::Approved),
        )
        .unwrap();

        assert_eq!(prepared.request_id, "request-1");
        assert_eq!(prepared.working_dir, fallback);
    }

    #[test]
    fn prepare_tool_dispatch_rejects_unavailable_tool() {
        let policy = policy(ToolAvailability::None, ToolApprovalMode::Auto);
        let session = ToolRuntimeSession {
            external_session_id: policy.external_session_id,
            runtime_session_id: "goose-session-1".to_string(),
            working_dir: None,
        };

        let error = prepare_tool_dispatch(
            &policy,
            &session,
            ToolInvocation {
                id: Some("request-1".to_string()),
                name: "developer__shell".to_string(),
                arguments: None,
            },
            Path::new("/tmp/fallback"),
        )
        .unwrap_err();

        assert_eq!(
            error,
            ToolDispatchPreparationError::Policy(ToolRuntimePolicyError::Unavailable {
                tool_name: "developer__shell".to_string()
            })
        );
    }

    #[test]
    fn prepare_tool_dispatch_rejects_non_object_arguments() {
        let policy = policy(ToolAvailability::All, ToolApprovalMode::Auto);
        let session = ToolRuntimeSession {
            external_session_id: policy.external_session_id,
            runtime_session_id: "goose-session-1".to_string(),
            working_dir: None,
        };

        let error = prepare_tool_dispatch(
            &policy,
            &session,
            ToolInvocation {
                id: Some("request-1".to_string()),
                name: "developer__shell".to_string(),
                arguments: Some(serde_json::json!("nope")),
            },
            Path::new("/tmp/fallback"),
        )
        .unwrap_err();

        assert!(matches!(error, ToolDispatchPreparationError::Arguments(_)));
    }

    #[test]
    fn validates_active_tools_against_allowlist() {
        let policy = policy(
            ToolAvailability::Only(vec![
                "developer__read".to_string(),
                "developer__shell".to_string(),
            ]),
            ToolApprovalMode::Auto,
        );

        let result =
            validate_active_tools_for_policy(&policy, ["developer__read", "developer__shell"]);

        assert!(result.is_ok());
    }

    #[test]
    fn reports_missing_and_disallowed_active_tools() {
        let policy = policy(
            ToolAvailability::Only(vec![
                "developer__read".to_string(),
                "developer__shell".to_string(),
            ]),
            ToolApprovalMode::Auto,
        );

        let violation =
            validate_active_tools_for_policy(&policy, ["developer__read", "developer__write"])
                .unwrap_err();

        assert_eq!(violation.missing_allowed, vec!["developer__shell"]);
        assert_eq!(violation.disallowed_active, vec!["developer__write"]);
        assert!(violation.to_string().contains("unavailable tools"));
        assert!(violation.to_string().contains("disallowed tools"));
    }

    #[test]
    fn describes_visible_and_blocked_tool_inventory() {
        let policy = policy(
            ToolAvailability::Only(vec![
                "developer__read".to_string(),
                "developer__shell".to_string(),
            ]),
            ToolApprovalMode::Ask,
        );

        let inventory = describe_tool_inventory(&policy, ["developer__read", "developer__write"]);

        assert_eq!(
            inventory.tools,
            vec![
                ToolVisibility {
                    name: "developer__read".to_string(),
                    visible: true,
                    reason: ToolVisibilityReason::Available,
                    approval_mode: ToolApprovalMode::Ask,
                },
                ToolVisibility {
                    name: "developer__write".to_string(),
                    visible: false,
                    reason: ToolVisibilityReason::NotAllowedByPolicy,
                    approval_mode: ToolApprovalMode::Ask,
                },
            ]
        );
        assert_eq!(inventory.missing_allowed_tools, vec!["developer__shell"]);
    }

    #[test]
    fn describes_none_policy_as_unavailable_inventory() {
        let policy = policy(ToolAvailability::None, ToolApprovalMode::Auto);

        let inventory = describe_tool_inventory(&policy, ["developer__read"]);

        assert_eq!(
            inventory.tools,
            vec![ToolVisibility {
                name: "developer__read".to_string(),
                visible: false,
                reason: ToolVisibilityReason::UnavailableByPolicy,
                approval_mode: ToolApprovalMode::Auto,
            }]
        );
        assert!(inventory.missing_allowed_tools.is_empty());
    }

    #[test]
    fn accepts_missing_or_object_invocation_arguments() {
        assert!(take_invocation_argument_object(None).unwrap().is_none());

        let arguments = serde_json::json!({ "command": "ls" });
        let arguments = take_invocation_argument_object(Some(arguments)).unwrap();

        assert_eq!(
            arguments.unwrap().get("command"),
            Some(&serde_json::Value::String("ls".to_string()))
        );
    }

    #[test]
    fn rejects_non_object_invocation_arguments() {
        let error =
            take_invocation_argument_object(Some(serde_json::json!("not object"))).unwrap_err();

        assert_eq!(error.message, "Tool arguments must be a JSON object.");
    }

    #[test]
    fn selects_requested_bound_workspace() {
        let requested = PathBuf::from("/tmp/alice");
        let fallback = PathBuf::from("/tmp/fallback");
        let bindings = vec![WorkspaceBinding {
            owner_id: "alice".to_string(),
            local_path: requested.clone(),
            mount_name: "alice".to_string(),
            writable: true,
        }];

        let selected = select_working_dir(Some(&requested), &bindings, &fallback);

        assert_eq!(selected, requested);
    }

    #[test]
    fn ignores_requested_unbound_workspace() {
        let requested = PathBuf::from("/tmp/other");
        let allowed = PathBuf::from("/tmp/alice");
        let fallback = PathBuf::from("/tmp/fallback");
        let bindings = vec![WorkspaceBinding {
            owner_id: "alice".to_string(),
            local_path: allowed.clone(),
            mount_name: "alice".to_string(),
            writable: true,
        }];

        let selected = select_working_dir(Some(&requested), &bindings, &fallback);

        assert_eq!(selected, allowed);
    }
}
