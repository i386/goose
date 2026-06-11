use crate::*;
use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;
use goose_provider_runtime::ProviderRetryPolicy;
use goose_providers::conversation::message::Message;
use goose_runtime_policy::PromptPolicy;
use goose_tool_runtime::ToolRuntimePolicy;
use rmcp::model::CallToolRequestParams;
use std::sync::Mutex;
use tokio_util::sync::CancellationToken;

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
