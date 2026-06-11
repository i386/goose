use anyhow::Result;
use async_trait::async_trait;
use goose_providers::conversation::message::{Message, MessageContent};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

pub type ExternalSessionId = Uuid;
pub type ExternalTaskId = Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionLink {
    pub external_session_id: ExternalSessionId,
    pub goose_session_id: String,
    pub working_dir: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeWorkspaceRef {
    pub owner_id: Option<String>,
    pub mount_name: Option<String>,
    pub local_path: Option<PathBuf>,
    pub remote_uri: Option<String>,
}

impl RuntimeWorkspaceRef {
    pub fn local_path(local_path: impl Into<PathBuf>) -> Self {
        Self {
            owner_id: None,
            mount_name: None,
            local_path: Some(local_path.into()),
            remote_uri: None,
        }
    }

    pub fn remote_uri(remote_uri: impl Into<String>) -> Self {
        Self {
            owner_id: None,
            mount_name: None,
            local_path: None,
            remote_uri: Some(remote_uri.into()),
        }
    }

    pub fn with_owner(mut self, owner_id: impl Into<String>) -> Self {
        self.owner_id = Some(owner_id.into());
        self
    }

    pub fn with_mount_name(mut self, mount_name: impl Into<String>) -> Self {
        self.mount_name = Some(mount_name.into());
        self
    }

    pub fn with_local_path(mut self, local_path: impl Into<PathBuf>) -> Self {
        self.local_path = Some(local_path.into());
        self
    }

    pub fn with_remote_uri(mut self, remote_uri: impl Into<String>) -> Self {
        self.remote_uri = Some(remote_uri.into());
        self
    }

    pub fn is_local_bound(&self) -> bool {
        self.local_path.is_some()
    }

    pub fn is_remote(&self) -> bool {
        self.remote_uri.is_some()
    }

    pub fn require_local_path(&self) -> Result<PathBuf> {
        self.local_path
            .clone()
            .ok_or_else(|| anyhow::anyhow!("runtime workspace has no local path binding"))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeSessionRecord {
    pub external_session_id: ExternalSessionId,
    pub runtime_session_id: String,
    pub title: Option<String>,
    pub workspace: RuntimeWorkspaceRef,
    pub metadata: serde_json::Value,
}

impl RuntimeSessionRecord {
    pub fn new(
        external_session_id: ExternalSessionId,
        runtime_session_id: impl Into<String>,
        workspace: RuntimeWorkspaceRef,
    ) -> Self {
        Self {
            external_session_id,
            runtime_session_id: runtime_session_id.into(),
            title: None,
            workspace,
            metadata: serde_json::Value::Object(serde_json::Map::new()),
        }
    }

    pub fn from_session_link(link: SessionLink) -> Self {
        Self::new(
            link.external_session_id,
            link.goose_session_id,
            RuntimeWorkspaceRef::local_path(link.working_dir),
        )
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = metadata;
        self
    }

    pub fn bind_local_workspace(mut self, local_path: impl Into<PathBuf>) -> Self {
        self.workspace = self.workspace.with_local_path(local_path);
        self
    }

    pub fn try_into_session_link(&self) -> Result<SessionLink> {
        Ok(SessionLink {
            external_session_id: self.external_session_id,
            goose_session_id: self.runtime_session_id.clone(),
            working_dir: self.workspace.require_local_path()?,
        })
    }
}

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

#[async_trait]
pub trait SessionLinkStore: Send + Sync {
    async fn load_session_link(
        &self,
        external_session_id: ExternalSessionId,
    ) -> Result<Option<SessionLink>>;

    async fn save_session_link(&self, link: SessionLink) -> Result<()>;
}

#[async_trait]
pub trait RuntimeSessionCatalog: Send + Sync {
    async fn load_runtime_session(
        &self,
        external_session_id: ExternalSessionId,
    ) -> Result<Option<RuntimeSessionRecord>>;

    async fn save_runtime_session(&self, record: RuntimeSessionRecord) -> Result<()>;
}

#[async_trait]
pub trait SessionPersistence: SessionLinkStore {
    async fn working_dir_for_session(
        &self,
        external_session_id: ExternalSessionId,
    ) -> Result<PathBuf>;

    async fn append_history(&self, entry: RuntimeHistoryEntry) -> Result<()>;

    async fn replace_history(
        &self,
        external_session_id: ExternalSessionId,
        entries: Vec<RuntimeHistoryEntry>,
    ) -> Result<()>;

    async fn list_history(
        &self,
        external_session_id: ExternalSessionId,
    ) -> Result<Vec<RuntimeHistoryEntry>>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::CallToolRequestParams;

    #[test]
    fn builds_text_message_from_legacy_history_entry() {
        let entry = RuntimeHistoryEntry {
            id: Uuid::new_v4(),
            external_session_id: Uuid::new_v4(),
            external_task_id: None,
            source_message_id: Some("external-msg-1".to_string()),
            runtime: "test".to_string(),
            role: "user".to_string(),
            content: "hello".to_string(),
            message_json: None,
        };

        let message = message_from_runtime_history(&entry).unwrap();

        assert_eq!(message.id.as_deref(), Some("external-msg-1"));
        assert_eq!(message.as_concat_text(), "hello");
    }

    #[test]
    fn builds_messages_from_runtime_history_batch() {
        let external_session_id = Uuid::new_v4();
        let entries = vec![
            RuntimeHistoryEntry {
                id: Uuid::new_v4(),
                external_session_id,
                external_task_id: None,
                source_message_id: Some("msg-1".to_string()),
                runtime: "test".to_string(),
                role: "user".to_string(),
                content: "hello".to_string(),
                message_json: None,
            },
            RuntimeHistoryEntry {
                id: Uuid::new_v4(),
                external_session_id,
                external_task_id: None,
                source_message_id: Some("msg-2".to_string()),
                runtime: "test".to_string(),
                role: "assistant".to_string(),
                content: "hi".to_string(),
                message_json: None,
            },
        ];

        let messages = messages_from_runtime_history(&entries).unwrap();

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].id.as_deref(), Some("msg-1"));
        assert_eq!(messages[0].as_concat_text(), "hello");
        assert_eq!(messages[1].id.as_deref(), Some("msg-2"));
        assert_eq!(messages[1].as_concat_text(), "hi");
    }

    #[test]
    fn preserves_structured_tool_request_history() {
        let external_session_id = Uuid::new_v4();
        let message = Message::assistant()
            .with_id("assistant-msg-1")
            .with_tool_request("tool-1", Ok(CallToolRequestParams::new("developer__shell")));

        let entry = runtime_history_from_message(external_session_id, &message).unwrap();
        let restored = message_from_runtime_history(&entry).unwrap();

        assert_eq!(entry.external_session_id, external_session_id);
        assert_eq!(entry.source_message_id.as_deref(), Some("assistant-msg-1"));
        assert_eq!(entry.role, "assistant");
        assert!(entry.content.is_empty());
        assert!(entry.message_json.is_some());
        assert_eq!(restored.id.as_deref(), Some("assistant-msg-1"));
        assert_eq!(restored.content.len(), 1);
    }

    #[test]
    fn runtime_session_record_converts_local_session_link() {
        let external_session_id = Uuid::new_v4();
        let link = SessionLink {
            external_session_id,
            goose_session_id: "goose-session-1".to_string(),
            working_dir: PathBuf::from("/tmp/workspace"),
        };

        let record = RuntimeSessionRecord::from_session_link(link.clone()).with_title("Demo");
        let restored = record.try_into_session_link().unwrap();

        assert_eq!(record.external_session_id, external_session_id);
        assert_eq!(record.runtime_session_id, "goose-session-1");
        assert_eq!(record.title.as_deref(), Some("Demo"));
        assert_eq!(restored.external_session_id, link.external_session_id);
        assert_eq!(restored.goose_session_id, link.goose_session_id);
        assert_eq!(restored.working_dir, link.working_dir);
    }

    #[test]
    fn remote_runtime_workspace_requires_host_binding_before_goose_link() {
        let record = RuntimeSessionRecord::new(
            Uuid::new_v4(),
            "goose-session-1",
            RuntimeWorkspaceRef::remote_uri("s3://tenant/session"),
        );

        let error = record.try_into_session_link().unwrap_err();

        assert!(error.to_string().contains("no local path binding"));
    }

    #[test]
    fn remote_runtime_workspace_can_be_bound_to_local_goose_workspace() {
        let external_session_id = Uuid::new_v4();
        let record = RuntimeSessionRecord::new(
            external_session_id,
            "goose-session-1",
            RuntimeWorkspaceRef::remote_uri("s3://tenant/session")
                .with_owner("alice")
                .with_mount_name("alice-workspace"),
        )
        .bind_local_workspace("/tmp/alice-workspace");

        assert!(record.workspace.is_remote());
        assert!(record.workspace.is_local_bound());
        assert_eq!(record.workspace.owner_id.as_deref(), Some("alice"));
        assert_eq!(
            record.workspace.remote_uri.as_deref(),
            Some("s3://tenant/session")
        );

        let link = record.try_into_session_link().unwrap();

        assert_eq!(link.external_session_id, external_session_id);
        assert_eq!(link.goose_session_id, "goose-session-1");
        assert_eq!(link.working_dir, PathBuf::from("/tmp/alice-workspace"));
    }
}
