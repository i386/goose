use crate::agents::{Agent, AgentEvent, GoosePlatform, RetryConfig, SessionConfig};
use crate::config::GooseMode;
use crate::conversation::message::Message;
pub use crate::providers::runtime_contracts::provider_failure_from_error;
use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;
use goose_agent_loop::{
    run_agent_loop_with_lifecycle_source_runtime, source_event_stream_to_loop_event_stream,
    source_event_to_loop_events, AgentLoopEvent, AgentLoopEventStream, AgentLoopOptions,
    AgentLoopRequest, AgentLoopRetrySpec, AgentLoopRuntime, AgentLoopSessionSpec,
    AgentLoopSourceEvent, AgentLoopSourceEventStream, AgentLoopSourceRuntime,
};
use goose_runtime_policy::{PromptPolicy, PromptPolicyApplier, RuntimeMode, RuntimePlatform};
use goose_tool_runtime::{validate_active_tools_for_policy, ToolAvailability, ToolRuntimePolicy};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub struct GooseAgentLoopRuntime {
    agent: Arc<Agent>,
}

impl GooseAgentLoopRuntime {
    pub fn new(agent: Arc<Agent>) -> Self {
        Self { agent }
    }
}

#[async_trait]
impl AgentLoopRuntime for GooseAgentLoopRuntime {
    type UserMessage = Message;

    async fn run_loop<'a>(
        &'a self,
        request: AgentLoopRequest<Self::UserMessage>,
    ) -> Result<AgentLoopEventStream<'a>> {
        run_agent_loop_with_lifecycle_source_runtime(self, request).await
    }
}

#[async_trait]
impl AgentLoopSourceRuntime for GooseAgentLoopRuntime {
    type UserMessage = Message;

    async fn apply_prompt_policy(&self, policy: &PromptPolicy) -> Result<()> {
        apply_prompt_policy(self.agent.as_ref(), policy).await;
        Ok(())
    }

    async fn enforce_tool_policy(
        &self,
        runtime_session_id: &str,
        policy: Option<&ToolRuntimePolicy>,
    ) -> Result<()> {
        enforce_tool_runtime_policy(self.agent.as_ref(), runtime_session_id, policy).await
    }

    async fn run_source_loop<'a>(
        &'a self,
        user_message: Self::UserMessage,
        session_spec: AgentLoopSessionSpec,
        cancel_token: Option<CancellationToken>,
    ) -> Result<AgentLoopSourceEventStream<'a>> {
        agent_source_event_stream(
            self.agent.as_ref(),
            user_message,
            session_spec,
            cancel_token,
        )
        .await
    }
}

pub async fn reply_with_contracts<'a>(
    agent: &'a Agent,
    user_message: Message,
    session_id: String,
    options: AgentLoopOptions,
    prompt_policy: &PromptPolicy,
    tool_policy: Option<&ToolRuntimePolicy>,
    cancel_token: Option<CancellationToken>,
) -> Result<AgentLoopEventStream<'a>> {
    apply_prompt_policy(agent, prompt_policy).await;
    enforce_tool_runtime_policy(agent, &session_id, tool_policy).await?;

    let source_stream = agent_source_event_stream(
        agent,
        user_message,
        options.session_spec(session_id),
        cancel_token,
    )
    .await?;

    Ok(source_event_stream_to_loop_event_stream(source_stream))
}

async fn agent_source_event_stream<'a>(
    agent: &'a Agent,
    user_message: Message,
    session_spec: AgentLoopSessionSpec,
    cancel_token: Option<CancellationToken>,
) -> Result<AgentLoopSourceEventStream<'a>> {
    let session_config = session_config_from_loop_spec(session_spec);
    let mut goose_events = agent
        .reply(user_message, session_config, cancel_token)
        .await?;

    let source_stream = async_stream::try_stream! {
        while let Some(event) = goose_events.next().await {
            yield agent_event_to_source_event(event?);
        }
    };

    Ok(Box::pin(source_stream))
}

pub async fn apply_prompt_policy(agent: &Agent, policy: &PromptPolicy) {
    agent
        .prompt_manager
        .lock()
        .await
        .apply_prompt_policy(policy);
}

pub async fn enforce_tool_runtime_policy(
    agent: &Agent,
    session_id: &str,
    policy: Option<&ToolRuntimePolicy>,
) -> Result<()> {
    let Some(policy) = policy else {
        return Ok(());
    };

    match &policy.available_tools {
        ToolAvailability::All | ToolAvailability::None => Ok(()),
        ToolAvailability::Only(_) => {
            let active_tools = agent.list_tools(session_id, None).await;
            let active_tool_names = active_tools
                .iter()
                .map(|tool| tool.name.to_string())
                .collect::<Vec<_>>();

            validate_active_tools_for_policy(policy, active_tool_names)
                .map_err(|violation| anyhow::anyhow!(violation))
        }
    }
}

pub fn goose_mode_from_policy(mode: RuntimeMode) -> GooseMode {
    match mode {
        RuntimeMode::Auto => GooseMode::Auto,
        RuntimeMode::Approve => GooseMode::Approve,
        RuntimeMode::SmartApprove => GooseMode::SmartApprove,
        RuntimeMode::Chat => GooseMode::Chat,
    }
}

pub fn goose_platform_from_policy(platform: RuntimePlatform) -> GoosePlatform {
    match platform {
        RuntimePlatform::Cli => GoosePlatform::GooseCli,
        RuntimePlatform::Desktop => GoosePlatform::GooseDesktop,
    }
}

pub fn session_config_from_loop_spec(spec: AgentLoopSessionSpec) -> SessionConfig {
    SessionConfig {
        id: spec.runtime_session_id,
        schedule_id: None,
        max_turns: spec.max_turns,
        retry_config: spec.retry.map(retry_config_from_loop_spec),
    }
}

pub fn retry_config_from_loop_spec(spec: AgentLoopRetrySpec) -> RetryConfig {
    RetryConfig {
        max_retries: spec.max_retries,
        checks: Vec::new(),
        on_failure: None,
        timeout_seconds: spec.timeout_seconds,
        on_failure_timeout_seconds: None,
    }
}

pub fn agent_event_to_loop_events(event: AgentEvent) -> Vec<AgentLoopEvent> {
    source_event_to_loop_events(agent_event_to_source_event(event))
}

pub fn agent_event_to_source_event(event: AgentEvent) -> AgentLoopSourceEvent {
    match event {
        AgentEvent::Message(message) => AgentLoopSourceEvent::Message(message),
        AgentEvent::McpNotification((request_id, notification)) => {
            AgentLoopSourceEvent::McpNotification {
                request_id,
                notification: format!("{notification:?}"),
            }
        }
        AgentEvent::HistoryReplaced(conversation) => AgentLoopSourceEvent::HistoryReplaced {
            message_count: conversation.len(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation::message::Message;
    use crate::conversation::Conversation;
    use crate::providers::errors::ProviderError;
    use goose_agent_loop::{message_to_loop_events, AgentLoopEvent};
    use goose_provider_runtime::ProviderErrorKind;
    use rmcp::model::CallToolRequestParams;

    #[test]
    fn maps_text_and_tool_request_messages_to_loop_events() {
        let message = Message::assistant()
            .with_id("msg-1")
            .with_text("hello")
            .with_tool_request("tool-1", Ok(CallToolRequestParams::new("shell")));

        let events = message_to_loop_events(message);

        assert_eq!(events.len(), 2);
        match &events[0] {
            AgentLoopEvent::Text { message_id, text } => {
                assert_eq!(message_id.as_deref(), Some("msg-1"));
                assert_eq!(text, "hello");
            }
            event => panic!("unexpected event: {event:?}"),
        }
        match &events[1] {
            AgentLoopEvent::ToolCall { id, name, .. } => {
                assert_eq!(id, "tool-1");
                assert_eq!(name, "shell");
            }
            event => panic!("unexpected event: {event:?}"),
        }
    }

    #[test]
    fn maps_provider_errors_to_retryable_failures() {
        let failure = provider_failure_from_error(ProviderError::NetworkError("offline".into()));

        assert_eq!(failure.kind, ProviderErrorKind::Network);
        assert!(failure.retryable);
        assert!(failure.message.contains("offline"));
    }

    #[test]
    fn lifecycle_events_are_available_to_runtime_clients() {
        let started = AgentLoopEvent::RunStarted {
            runtime_session_id: "session-1".to_string(),
        };
        let completed = AgentLoopEvent::RunCompleted {
            runtime_session_id: "session-1".to_string(),
        };

        match started {
            AgentLoopEvent::RunStarted { runtime_session_id } => {
                assert_eq!(runtime_session_id, "session-1");
            }
            event => panic!("unexpected event: {event:?}"),
        }
        match completed {
            AgentLoopEvent::RunCompleted { runtime_session_id } => {
                assert_eq!(runtime_session_id, "session-1");
            }
            event => panic!("unexpected event: {event:?}"),
        }
    }

    #[test]
    fn builds_goose_session_config_from_loop_spec() {
        let config = session_config_from_loop_spec(AgentLoopSessionSpec {
            runtime_session_id: "session-1".to_string(),
            max_turns: Some(5),
            retry: Some(AgentLoopRetrySpec {
                max_retries: 2,
                timeout_seconds: Some(90),
            }),
        });

        assert_eq!(config.id, "session-1");
        assert_eq!(config.max_turns, Some(5));
        let retry = config.retry_config.unwrap();
        assert_eq!(retry.max_retries, 2);
        assert_eq!(retry.timeout_seconds, Some(90));
        assert!(retry.checks.is_empty());
    }

    #[test]
    fn normalizes_history_replaced_event_to_source_event() {
        let conversation = Conversation::new_unvalidated([
            Message::user().with_text("hello"),
            Message::assistant().with_text("hi"),
        ]);

        let source = agent_event_to_source_event(AgentEvent::HistoryReplaced(conversation));

        match source {
            AgentLoopSourceEvent::HistoryReplaced { message_count } => {
                assert_eq!(message_count, 2);
            }
            event => panic!("unexpected event: {event:?}"),
        }
    }
}
