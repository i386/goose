use anyhow::Result;
use futures::stream::BoxStream;
use futures::StreamExt;
use goose_providers::conversation::message::{ActionRequiredData, Message, MessageContent};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub type AgentLoopEventStream<'a> = BoxStream<'a, Result<AgentLoopEvent>>;
pub type AgentLoopSourceEventStream<'a> = BoxStream<'a, Result<AgentLoopSourceEvent>>;

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
