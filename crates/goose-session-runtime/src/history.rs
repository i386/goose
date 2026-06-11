use anyhow::Result;
use goose_providers::conversation::message::{Message, MessageContent};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{ExternalSessionId, ExternalTaskId};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeHistoryEntry {
    pub id: Uuid,
    pub external_session_id: ExternalSessionId,
    pub external_task_id: Option<ExternalTaskId>,
    pub source_message_id: Option<String>,
    pub runtime: String,
    pub role: String,
    pub content: String,
    pub message_json: Option<serde_json::Value>,
}

impl RuntimeHistoryEntry {
    pub fn from_message(external_session_id: ExternalSessionId, message: &Message) -> Option<Self> {
        runtime_history_from_message(external_session_id, message)
    }

    pub fn into_message(&self) -> Result<Message> {
        message_from_runtime_history(self)
    }
}

pub fn message_from_runtime_history(entry: &RuntimeHistoryEntry) -> Result<Message> {
    if let Some(message_json) = &entry.message_json {
        return Ok(serde_json::from_value(message_json.clone())?);
    }

    let message = match entry.role.as_str() {
        "assistant" => Message::assistant(),
        _ => Message::user(),
    };

    let message = if let Some(source_message_id) = &entry.source_message_id {
        message.with_id(source_message_id.clone())
    } else {
        message.with_id(entry.id.to_string())
    };

    Ok(message.with_text(entry.content.clone()))
}

pub fn messages_from_runtime_history(entries: &[RuntimeHistoryEntry]) -> Result<Vec<Message>> {
    entries.iter().map(message_from_runtime_history).collect()
}

pub fn runtime_history_from_message(
    external_session_id: ExternalSessionId,
    message: &Message,
) -> Option<RuntimeHistoryEntry> {
    let message_json = serde_json::to_value(message).ok();
    let content = message
        .content
        .iter()
        .filter_map(|content| match content {
            MessageContent::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");

    if content.is_empty() && message.content.is_empty() {
        return None;
    }

    Some(RuntimeHistoryEntry {
        id: message
            .id
            .as_deref()
            .and_then(|id| Uuid::parse_str(id).ok())
            .unwrap_or_else(Uuid::new_v4),
        external_session_id,
        external_task_id: None,
        source_message_id: message.id.clone(),
        runtime: "goose".to_string(),
        role: role_name(message),
        content,
        message_json,
    })
}

fn role_name(message: &Message) -> String {
    match message.role {
        rmcp::model::Role::Assistant => "assistant".to_string(),
        rmcp::model::Role::User => "user".to_string(),
    }
}
