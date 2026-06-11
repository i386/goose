use anyhow::Result;
use async_trait::async_trait;
use futures::stream::BoxStream;
use futures::StreamExt;
use goose_provider_runtime::ProviderRetryPolicy;
use goose_providers::conversation::message::{ActionRequiredData, Message, MessageContent};
use goose_runtime_policy::PromptPolicy;
use goose_tool_runtime::ToolRuntimePolicy;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

pub type AgentLoopEventStream<'a> = BoxStream<'a, Result<AgentLoopEvent>>;
pub type AgentLoopSourceEventStream<'a> = BoxStream<'a, Result<AgentLoopSourceEvent>>;

pub const DEFAULT_AGENT_LOOP_MAX_TURNS: u32 = 1000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentLoopTurnLimit {
    explicit_max_turns: Option<u32>,
    configured_max_turns: Option<u32>,
    default_max_turns: u32,
}

impl Default for AgentLoopTurnLimit {
    fn default() -> Self {
        Self {
            explicit_max_turns: None,
            configured_max_turns: None,
            default_max_turns: DEFAULT_AGENT_LOOP_MAX_TURNS,
        }
    }
}

impl AgentLoopTurnLimit {
    pub fn new(explicit_max_turns: Option<u32>) -> Self {
        Self {
            explicit_max_turns,
            ..Self::default()
        }
    }

    pub fn with_configured_max_turns(mut self, configured_max_turns: Option<u32>) -> Self {
        self.configured_max_turns = configured_max_turns;
        self
    }

    pub fn with_default_max_turns(mut self, default_max_turns: u32) -> Self {
        self.default_max_turns = default_max_turns;
        self
    }

    pub fn resolve(self) -> u32 {
        self.explicit_max_turns
            .or(self.configured_max_turns)
            .unwrap_or(self.default_max_turns)
    }
}

#[derive(Clone, Debug)]
pub struct AgentLoopControl {
    max_turns: u32,
    turns_taken: u32,
    cancellation_token: Option<CancellationToken>,
}

impl AgentLoopControl {
    pub fn new(max_turns: u32, cancellation_token: Option<CancellationToken>) -> Self {
        Self {
            max_turns,
            turns_taken: 0,
            cancellation_token,
        }
    }

    pub fn max_turns(&self) -> u32 {
        self.max_turns
    }

    pub fn turns_taken(&self) -> u32 {
        self.turns_taken
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancellation_token
            .as_ref()
            .is_some_and(CancellationToken::is_cancelled)
    }

    pub fn begin_turn(&mut self, count_turn: bool) -> AgentLoopControlDecision {
        if self.is_cancelled() {
            return AgentLoopControlDecision::Cancelled;
        }

        if count_turn {
            self.turns_taken = self.turns_taken.saturating_add(1);
        }

        if self.turns_taken > self.max_turns {
            AgentLoopControlDecision::MaxTurnsReached
        } else {
            AgentLoopControlDecision::Continue
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentLoopControlDecision {
    Continue,
    Cancelled,
    MaxTurnsReached,
}

pub struct AgentLoopRequest<UserMessage> {
    pub user_message: UserMessage,
    pub runtime_session_id: String,
    pub options: AgentLoopOptions,
    pub prompt_policy: PromptPolicy,
    pub tool_policy: Option<ToolRuntimePolicy>,
    pub cancellation_token: Option<CancellationToken>,
}

#[async_trait]
pub trait AgentLoopRuntime: Send + Sync {
    type UserMessage: Send + 'static;

    async fn run_loop<'a>(
        &'a self,
        request: AgentLoopRequest<Self::UserMessage>,
    ) -> Result<AgentLoopEventStream<'a>>;
}

#[async_trait]
pub trait AgentLoopSourceRuntime: Send + Sync {
    type UserMessage: Send + 'static;

    async fn apply_prompt_policy(&self, policy: &PromptPolicy) -> Result<()>;

    async fn enforce_tool_policy(
        &self,
        runtime_session_id: &str,
        policy: Option<&ToolRuntimePolicy>,
    ) -> Result<()>;

    async fn run_source_loop<'a>(
        &'a self,
        user_message: Self::UserMessage,
        session_spec: AgentLoopSessionSpec,
        cancellation_token: Option<CancellationToken>,
    ) -> Result<AgentLoopSourceEventStream<'a>>;
}

pub async fn run_agent_loop_with_source_runtime<'a, Runtime>(
    runtime: &'a Runtime,
    request: AgentLoopRequest<Runtime::UserMessage>,
) -> Result<AgentLoopEventStream<'a>>
where
    Runtime: AgentLoopSourceRuntime + ?Sized,
{
    let AgentLoopRequest {
        user_message,
        runtime_session_id,
        options,
        prompt_policy,
        tool_policy,
        cancellation_token,
    } = request;

    runtime.apply_prompt_policy(&prompt_policy).await?;
    runtime
        .enforce_tool_policy(&runtime_session_id, tool_policy.as_ref())
        .await?;

    let session_spec = options.session_spec(runtime_session_id);
    let source_events = runtime
        .run_source_loop(user_message, session_spec, cancellation_token)
        .await?;

    Ok(source_event_stream_to_loop_event_stream(source_events))
}

pub async fn run_agent_loop_with_lifecycle_source_runtime<'a, Runtime>(
    runtime: &'a Runtime,
    request: AgentLoopRequest<Runtime::UserMessage>,
) -> Result<AgentLoopEventStream<'a>>
where
    Runtime: AgentLoopSourceRuntime + ?Sized,
{
    let runtime_session_id = request.runtime_session_id.clone();
    let inner = run_agent_loop_with_source_runtime(runtime, request).await?;
    Ok(with_lifecycle_events(runtime_session_id, inner))
}

pub fn with_lifecycle_events<'a>(
    runtime_session_id: String,
    mut stream: AgentLoopEventStream<'a>,
) -> AgentLoopEventStream<'a> {
    let stream = async_stream::try_stream! {
        yield AgentLoopEvent::RunStarted {
            runtime_session_id: runtime_session_id.clone(),
        };

        while let Some(event) = stream.next().await {
            yield event?;
        }

        yield AgentLoopEvent::RunCompleted {
            runtime_session_id,
        };
    };

    Box::pin(stream)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentLoopOptions {
    pub max_turns: Option<u32>,
    pub retry: ProviderRetryPolicy,
}

impl Default for AgentLoopOptions {
    fn default() -> Self {
        Self {
            max_turns: Some(12),
            retry: ProviderRetryPolicy::default(),
        }
    }
}

impl AgentLoopOptions {
    pub fn session_spec(&self, runtime_session_id: impl Into<String>) -> AgentLoopSessionSpec {
        AgentLoopSessionSpec {
            runtime_session_id: runtime_session_id.into(),
            max_turns: self.max_turns,
            retry: AgentLoopRetrySpec::from_provider_retry_policy(&self.retry),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AgentLoopSessionSpec {
    pub runtime_session_id: String,
    pub max_turns: Option<u32>,
    pub retry: Option<AgentLoopRetrySpec>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AgentLoopRetrySpec {
    pub max_retries: u32,
    pub timeout_seconds: Option<u64>,
}

impl AgentLoopRetrySpec {
    pub fn from_provider_retry_policy(policy: &ProviderRetryPolicy) -> Option<Self> {
        if !policy.retry_transient_errors || policy.max_attempts <= 1 {
            return None;
        }

        Some(Self {
            max_retries: policy.max_attempts.saturating_sub(1).max(1),
            timeout_seconds: policy.timeout_seconds,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentLoopEvent {
    RunStarted {
        runtime_session_id: String,
    },
    RunCompleted {
        runtime_session_id: String,
    },
    Text {
        message_id: Option<String>,
        text: String,
    },
    ToolCall {
        id: String,
        name: String,
        arguments: Value,
    },
    ToolResult {
        id: String,
        ok: bool,
        summary: String,
    },
    ActionRequired {
        id: String,
        kind: String,
        message: String,
    },
    SystemNotification {
        message: String,
        data: Option<Value>,
    },
    McpNotification {
        request_id: String,
        notification: String,
    },
    HistoryReplaced {
        message_count: usize,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentLoopSourceEvent {
    Message(Message),
    McpNotification {
        request_id: String,
        notification: String,
    },
    HistoryReplaced {
        message_count: usize,
    },
}

pub fn source_event_to_loop_events(event: AgentLoopSourceEvent) -> Vec<AgentLoopEvent> {
    match event {
        AgentLoopSourceEvent::Message(message) => message_to_loop_events(message),
        AgentLoopSourceEvent::McpNotification {
            request_id,
            notification,
        } => vec![AgentLoopEvent::McpNotification {
            request_id,
            notification,
        }],
        AgentLoopSourceEvent::HistoryReplaced { message_count } => {
            vec![AgentLoopEvent::HistoryReplaced { message_count }]
        }
    }
}

pub fn source_event_stream_to_loop_event_stream<'a>(
    mut stream: BoxStream<'a, Result<AgentLoopSourceEvent>>,
) -> AgentLoopEventStream<'a> {
    let stream = async_stream::try_stream! {
        while let Some(event) = stream.next().await {
            for loop_event in source_event_to_loop_events(event?) {
                yield loop_event;
            }
        }
    };

    Box::pin(stream)
}

pub fn message_to_loop_events(message: Message) -> Vec<AgentLoopEvent> {
    let message_id = message.id.clone();
    message
        .content
        .into_iter()
        .filter_map(|content| message_content_to_loop_event(message_id.clone(), content))
        .collect()
}

pub fn message_content_to_loop_event(
    message_id: Option<String>,
    content: MessageContent,
) -> Option<AgentLoopEvent> {
    match content {
        MessageContent::Text(text) => Some(AgentLoopEvent::Text {
            message_id,
            text: text.text.clone(),
        }),
        MessageContent::ToolRequest(request) => match request.tool_call {
            Ok(tool_call) => Some(AgentLoopEvent::ToolCall {
                id: request.id,
                name: tool_call.name.to_string(),
                arguments: serde_json::to_value(&tool_call.arguments).unwrap_or(Value::Null),
            }),
            Err(error) => Some(AgentLoopEvent::ToolResult {
                id: request.id,
                ok: false,
                summary: format!("invalid tool call: {error}"),
            }),
        },
        MessageContent::ToolResponse(response) => Some(match response.tool_result {
            Ok(result) => AgentLoopEvent::ToolResult {
                id: response.id,
                ok: true,
                summary: format!("tool returned {} content item(s)", result.content.len()),
            },
            Err(error) => AgentLoopEvent::ToolResult {
                id: response.id,
                ok: false,
                summary: error.to_string(),
            },
        }),
        MessageContent::ToolConfirmationRequest(request) => Some(AgentLoopEvent::ActionRequired {
            id: request.id,
            kind: "tool_confirmation".to_string(),
            message: request.prompt.unwrap_or_else(|| {
                format!(
                    "Confirm tool call `{}` before continuing.",
                    request.tool_name
                )
            }),
        }),
        MessageContent::ActionRequired(action) => Some(action_required_to_loop_event(action.data)),
        MessageContent::FrontendToolRequest(request) => match request.tool_call {
            Ok(tool_call) => Some(AgentLoopEvent::ToolCall {
                id: request.id,
                name: tool_call.name.to_string(),
                arguments: serde_json::to_value(&tool_call.arguments).unwrap_or(Value::Null),
            }),
            Err(error) => Some(AgentLoopEvent::ToolResult {
                id: request.id,
                ok: false,
                summary: format!("invalid frontend tool call: {error}"),
            }),
        },
        MessageContent::SystemNotification(notification) => {
            Some(AgentLoopEvent::SystemNotification {
                message: notification.msg,
                data: notification.data,
            })
        }
        MessageContent::Image(_)
        | MessageContent::Thinking(_)
        | MessageContent::RedactedThinking(_) => None,
    }
}

fn action_required_to_loop_event(action: ActionRequiredData) -> AgentLoopEvent {
    match action {
        ActionRequiredData::ToolConfirmation {
            id,
            tool_name,
            prompt,
            ..
        } => AgentLoopEvent::ActionRequired {
            id,
            kind: "tool_confirmation".to_string(),
            message: prompt
                .unwrap_or_else(|| format!("Confirm tool call `{tool_name}` before continuing.")),
        },
        ActionRequiredData::Elicitation { id, message, .. } => AgentLoopEvent::ActionRequired {
            id,
            kind: "elicitation".to_string(),
            message,
        },
        ActionRequiredData::ElicitationResponse { id, .. } => AgentLoopEvent::ActionRequired {
            id,
            kind: "elicitation_response".to_string(),
            message: "Elicitation response received.".to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::CallToolRequestParams;
    use std::sync::Mutex;

    #[test]
    fn loop_control_tracks_turn_budget() {
        let mut control = AgentLoopControl::new(2, None);

        assert_eq!(control.begin_turn(true), AgentLoopControlDecision::Continue);
        assert_eq!(control.turns_taken(), 1);
        assert_eq!(control.begin_turn(true), AgentLoopControlDecision::Continue);
        assert_eq!(control.turns_taken(), 2);
        assert_eq!(
            control.begin_turn(true),
            AgentLoopControlDecision::MaxTurnsReached
        );
        assert_eq!(control.turns_taken(), 3);
    }

    #[test]
    fn loop_control_can_skip_turn_increment_for_retries() {
        let mut control = AgentLoopControl::new(1, None);

        assert_eq!(
            control.begin_turn(false),
            AgentLoopControlDecision::Continue
        );
        assert_eq!(control.turns_taken(), 0);
        assert_eq!(control.begin_turn(true), AgentLoopControlDecision::Continue);
        assert_eq!(
            control.begin_turn(true),
            AgentLoopControlDecision::MaxTurnsReached
        );
    }

    #[test]
    fn loop_control_reports_cancellation_before_incrementing() {
        let token = CancellationToken::new();
        token.cancel();
        let mut control = AgentLoopControl::new(2, Some(token));

        assert_eq!(
            control.begin_turn(true),
            AgentLoopControlDecision::Cancelled
        );
        assert_eq!(control.turns_taken(), 0);
    }

    #[test]
    fn turn_limit_prefers_explicit_then_configured_then_default() {
        assert_eq!(
            AgentLoopTurnLimit::new(Some(5))
                .with_configured_max_turns(Some(10))
                .with_default_max_turns(20)
                .resolve(),
            5
        );
        assert_eq!(
            AgentLoopTurnLimit::new(None)
                .with_configured_max_turns(Some(10))
                .with_default_max_turns(20)
                .resolve(),
            10
        );
        assert_eq!(
            AgentLoopTurnLimit::new(None)
                .with_configured_max_turns(None)
                .with_default_max_turns(20)
                .resolve(),
            20
        );
    }

    #[test]
    fn turn_limit_default_matches_goose_loop_default() {
        assert_eq!(
            AgentLoopTurnLimit::new(None).resolve(),
            DEFAULT_AGENT_LOOP_MAX_TURNS
        );
    }

    #[derive(Default)]
    struct TestSourceRuntime {
        calls: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl AgentLoopSourceRuntime for TestSourceRuntime {
        type UserMessage = Message;

        async fn apply_prompt_policy(&self, policy: &PromptPolicy) -> Result<()> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("prompt:{:?}", policy.runtime_mode));
            Ok(())
        }

        async fn enforce_tool_policy(
            &self,
            runtime_session_id: &str,
            policy: Option<&ToolRuntimePolicy>,
        ) -> Result<()> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("tools:{runtime_session_id}:{}", policy.is_some()));
            Ok(())
        }

        async fn run_source_loop<'a>(
            &'a self,
            _user_message: Self::UserMessage,
            session_spec: AgentLoopSessionSpec,
            _cancellation_token: Option<CancellationToken>,
        ) -> Result<AgentLoopSourceEventStream<'a>> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("run:{}", session_spec.runtime_session_id));
            let stream = futures::stream::iter(vec![Ok(AgentLoopSourceEvent::Message(
                Message::assistant().with_text("hello from source runtime"),
            ))]);
            Ok(Box::pin(stream))
        }
    }

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
    fn maps_invalid_tool_request_to_failed_tool_result() {
        let message = Message::assistant().with_tool_request(
            "tool-1",
            Err(rmcp::model::ErrorData::invalid_request(
                "bad arguments",
                None,
            )),
        );

        let events = message_to_loop_events(message);

        match &events[..] {
            [AgentLoopEvent::ToolResult { id, ok, summary }] => {
                assert_eq!(id, "tool-1");
                assert!(!ok);
                assert!(summary.contains("invalid tool call"));
            }
            events => panic!("unexpected events: {events:?}"),
        }
    }

    #[test]
    fn maps_source_events_to_loop_events() {
        let events = source_event_to_loop_events(AgentLoopSourceEvent::McpNotification {
            request_id: "request-1".to_string(),
            notification: "progress".to_string(),
        });

        match &events[..] {
            [AgentLoopEvent::McpNotification {
                request_id,
                notification,
            }] => {
                assert_eq!(request_id, "request-1");
                assert_eq!(notification, "progress");
            }
            events => panic!("unexpected events: {events:?}"),
        }

        let events =
            source_event_to_loop_events(AgentLoopSourceEvent::HistoryReplaced { message_count: 3 });

        match &events[..] {
            [AgentLoopEvent::HistoryReplaced { message_count }] => {
                assert_eq!(*message_count, 3);
            }
            events => panic!("unexpected events: {events:?}"),
        }
    }

    #[tokio::test]
    async fn expands_source_event_stream_to_loop_event_stream() {
        let source = futures::stream::iter(vec![
            Ok(AgentLoopSourceEvent::Message(
                Message::assistant().with_text("hello"),
            )),
            Ok(AgentLoopSourceEvent::HistoryReplaced { message_count: 1 }),
        ]);
        let mut stream = source_event_stream_to_loop_event_stream(Box::pin(source));

        match stream.next().await.transpose().unwrap().unwrap() {
            AgentLoopEvent::Text { text, .. } => assert_eq!(text, "hello"),
            event => panic!("unexpected event: {event:?}"),
        }

        match stream.next().await.transpose().unwrap().unwrap() {
            AgentLoopEvent::HistoryReplaced { message_count } => {
                assert_eq!(message_count, 1);
            }
            event => panic!("unexpected event: {event:?}"),
        }

        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn source_runtime_runner_applies_policies_and_expands_events() {
        let runtime = TestSourceRuntime::default();
        let request = AgentLoopRequest {
            user_message: Message::user().with_text("hello"),
            runtime_session_id: "session-1".to_string(),
            options: AgentLoopOptions::default(),
            prompt_policy: PromptPolicy::chat(),
            tool_policy: None,
            cancellation_token: None,
        };

        let mut stream = run_agent_loop_with_source_runtime(&runtime, request)
            .await
            .unwrap();

        match stream.next().await.transpose().unwrap().unwrap() {
            AgentLoopEvent::Text { text, .. } => {
                assert_eq!(text, "hello from source runtime");
            }
            event => panic!("unexpected event: {event:?}"),
        }
        assert!(stream.next().await.is_none());

        assert_eq!(
            runtime.calls.lock().unwrap().as_slice(),
            ["prompt:Chat", "tools:session-1:false", "run:session-1"]
        );
    }

    #[tokio::test]
    async fn lifecycle_source_runtime_runner_wraps_start_and_completion() {
        let runtime = TestSourceRuntime::default();
        let request = AgentLoopRequest {
            user_message: Message::user().with_text("hello"),
            runtime_session_id: "session-1".to_string(),
            options: AgentLoopOptions::default(),
            prompt_policy: PromptPolicy::chat(),
            tool_policy: None,
            cancellation_token: None,
        };

        let mut stream = run_agent_loop_with_lifecycle_source_runtime(&runtime, request)
            .await
            .unwrap();

        match stream.next().await.transpose().unwrap().unwrap() {
            AgentLoopEvent::RunStarted { runtime_session_id } => {
                assert_eq!(runtime_session_id, "session-1");
            }
            event => panic!("unexpected event: {event:?}"),
        }
        match stream.next().await.transpose().unwrap().unwrap() {
            AgentLoopEvent::Text { text, .. } => {
                assert_eq!(text, "hello from source runtime");
            }
            event => panic!("unexpected event: {event:?}"),
        }
        match stream.next().await.transpose().unwrap().unwrap() {
            AgentLoopEvent::RunCompleted { runtime_session_id } => {
                assert_eq!(runtime_session_id, "session-1");
            }
            event => panic!("unexpected event: {event:?}"),
        }
        assert!(stream.next().await.is_none());
    }

    #[test]
    fn builds_session_spec_from_loop_options() {
        let options = AgentLoopOptions {
            max_turns: Some(7),
            retry: ProviderRetryPolicy {
                max_attempts: 3,
                retry_transient_errors: true,
                timeout_seconds: Some(60),
            },
        };

        let spec = options.session_spec("session-1");

        assert_eq!(
            spec,
            AgentLoopSessionSpec {
                runtime_session_id: "session-1".to_string(),
                max_turns: Some(7),
                retry: Some(AgentLoopRetrySpec {
                    max_retries: 2,
                    timeout_seconds: Some(60),
                }),
            }
        );
    }

    #[test]
    fn omits_retry_spec_when_provider_retry_policy_does_not_retry() {
        let policy = ProviderRetryPolicy {
            max_attempts: 1,
            retry_transient_errors: true,
            timeout_seconds: Some(60),
        };

        assert_eq!(
            AgentLoopRetrySpec::from_provider_retry_policy(&policy),
            None
        );
    }
}
