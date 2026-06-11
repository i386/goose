use crate::config::GooseMode;
use crate::conversation::Conversation;
use crate::session::{SessionManager, SessionType};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use goose_session_runtime::{
    message_from_runtime_history, messages_from_runtime_history, runtime_history_from_message,
    ExternalSessionId, RuntimeHistoryEntry, RuntimeSessionCatalog, RuntimeSessionRecord,
    SessionLink, SessionLinkStore, SessionPersistence,
};
use std::path::PathBuf;
use std::sync::Arc;

pub struct GooseSessionPersistenceAdapter {
    manager: SessionManager,
    links: Arc<dyn SessionLinkStore>,
}

impl GooseSessionPersistenceAdapter {
    pub fn new(manager: SessionManager, links: Arc<dyn SessionLinkStore>) -> Self {
        Self { manager, links }
    }

    pub async fn link_or_create_session(
        &self,
        external_session_id: ExternalSessionId,
        working_dir: PathBuf,
        name: String,
        session_type: SessionType,
        goose_mode: GooseMode,
    ) -> Result<SessionLink> {
        if let Some(link) = self.load_session_link(external_session_id).await? {
            return Ok(link);
        }

        let session = self
            .manager
            .create_session(working_dir.clone(), name, session_type, goose_mode)
            .await?;
        let link = SessionLink {
            external_session_id,
            goose_session_id: session.id,
            working_dir,
        };
        self.save_session_link(link.clone()).await?;
        Ok(link)
    }

    pub async fn link_or_create_runtime_session(
        &self,
        mut record: RuntimeSessionRecord,
        name: String,
        session_type: SessionType,
        goose_mode: GooseMode,
    ) -> Result<RuntimeSessionRecord> {
        if let Some(existing) = self
            .load_runtime_session(record.external_session_id)
            .await?
        {
            return Ok(existing);
        }

        let working_dir = record.workspace.require_local_path()?;
        let session = self
            .manager
            .create_session(working_dir.clone(), name, session_type, goose_mode)
            .await?;

        record.runtime_session_id = session.id;
        record.workspace = record.workspace.with_local_path(working_dir);
        self.save_runtime_session(record.clone()).await?;
        Ok(record)
    }

    async fn require_link(&self, external_session_id: ExternalSessionId) -> Result<SessionLink> {
        self.load_session_link(external_session_id)
            .await?
            .ok_or_else(|| {
                anyhow!("no goose session link for external session {external_session_id}")
            })
    }
}

#[async_trait]
impl SessionLinkStore for GooseSessionPersistenceAdapter {
    async fn load_session_link(
        &self,
        external_session_id: ExternalSessionId,
    ) -> Result<Option<SessionLink>> {
        self.links.load_session_link(external_session_id).await
    }

    async fn save_session_link(&self, link: SessionLink) -> Result<()> {
        self.links.save_session_link(link).await
    }
}

#[async_trait]
impl RuntimeSessionCatalog for GooseSessionPersistenceAdapter {
    async fn load_runtime_session(
        &self,
        external_session_id: ExternalSessionId,
    ) -> Result<Option<RuntimeSessionRecord>> {
        Ok(self
            .load_session_link(external_session_id)
            .await?
            .map(RuntimeSessionRecord::from_session_link))
    }

    async fn save_runtime_session(&self, record: RuntimeSessionRecord) -> Result<()> {
        self.save_session_link(record.try_into_session_link()?)
            .await
    }
}

#[async_trait]
impl SessionPersistence for GooseSessionPersistenceAdapter {
    async fn working_dir_for_session(
        &self,
        external_session_id: ExternalSessionId,
    ) -> Result<PathBuf> {
        Ok(self.require_link(external_session_id).await?.working_dir)
    }

    async fn append_history(&self, entry: RuntimeHistoryEntry) -> Result<()> {
        let link = self.require_link(entry.external_session_id).await?;
        let message = message_from_runtime_history(&entry)?;
        self.manager
            .add_message(&link.goose_session_id, &message)
            .await
    }

    async fn replace_history(
        &self,
        external_session_id: ExternalSessionId,
        entries: Vec<RuntimeHistoryEntry>,
    ) -> Result<()> {
        let link = self.require_link(external_session_id).await?;
        let messages = messages_from_runtime_history(&entries)?;
        let conversation = Conversation::new_unvalidated(messages);
        self.manager
            .replace_conversation(&link.goose_session_id, &conversation)
            .await
    }

    async fn list_history(
        &self,
        external_session_id: ExternalSessionId,
    ) -> Result<Vec<RuntimeHistoryEntry>> {
        let link = self.require_link(external_session_id).await?;
        let session = self
            .manager
            .get_session(&link.goose_session_id, true)
            .await?;
        let Some(conversation) = session.conversation else {
            return Ok(Vec::new());
        };

        Ok(conversation
            .messages()
            .iter()
            .filter_map(|message| runtime_history_from_message(external_session_id, message))
            .collect())
    }
}

#[cfg(all(test, feature = "rustls-tls"))]
mod tests {
    use super::*;
    use crate::conversation::message::Message;
    use goose_session_runtime::RuntimeWorkspaceRef;
    use std::collections::HashMap;
    use tokio::sync::Mutex;
    use uuid::Uuid;

    #[derive(Default)]
    struct InMemoryLinkStore {
        links: Mutex<HashMap<ExternalSessionId, SessionLink>>,
    }

    #[async_trait]
    impl SessionLinkStore for InMemoryLinkStore {
        async fn load_session_link(
            &self,
            external_session_id: ExternalSessionId,
        ) -> Result<Option<SessionLink>> {
            Ok(self.links.lock().await.get(&external_session_id).cloned())
        }

        async fn save_session_link(&self, link: SessionLink) -> Result<()> {
            self.links
                .lock()
                .await
                .insert(link.external_session_id, link);
            Ok(())
        }
    }

    #[tokio::test]
    async fn adapter_appends_and_lists_text_history() {
        let tempdir = tempfile::tempdir().unwrap();
        let manager = SessionManager::new(tempdir.path().to_path_buf());
        let adapter =
            GooseSessionPersistenceAdapter::new(manager, Arc::new(InMemoryLinkStore::default()));
        let external_session_id = Uuid::new_v4();

        adapter
            .link_or_create_session(
                external_session_id,
                tempdir.path().to_path_buf(),
                "runtime test".to_string(),
                SessionType::User,
                GooseMode::Chat,
            )
            .await
            .unwrap();

        adapter
            .append_history(RuntimeHistoryEntry {
                id: Uuid::new_v4(),
                external_session_id,
                external_task_id: None,
                source_message_id: Some("external-msg-1".to_string()),
                runtime: "test".to_string(),
                role: "user".to_string(),
                content: "hello".to_string(),
                message_json: None,
            })
            .await
            .unwrap();

        let history = adapter.list_history(external_session_id).await.unwrap();

        assert_eq!(history.len(), 1);
        assert_eq!(
            history[0].source_message_id.as_deref(),
            Some("external-msg-1")
        );
        assert_eq!(history[0].role, "user");
        assert_eq!(history[0].content, "hello");
        assert!(history[0].message_json.is_some());
    }

    #[tokio::test]
    async fn adapter_replaces_history_batch() {
        let tempdir = tempfile::tempdir().unwrap();
        let manager = SessionManager::new(tempdir.path().to_path_buf());
        let adapter =
            GooseSessionPersistenceAdapter::new(manager, Arc::new(InMemoryLinkStore::default()));
        let external_session_id = Uuid::new_v4();

        adapter
            .link_or_create_session(
                external_session_id,
                tempdir.path().to_path_buf(),
                "runtime replace test".to_string(),
                SessionType::User,
                GooseMode::Chat,
            )
            .await
            .unwrap();

        adapter
            .append_history(RuntimeHistoryEntry {
                id: Uuid::new_v4(),
                external_session_id,
                external_task_id: None,
                source_message_id: Some("old-msg".to_string()),
                runtime: "test".to_string(),
                role: "user".to_string(),
                content: "old".to_string(),
                message_json: None,
            })
            .await
            .unwrap();

        adapter
            .replace_history(
                external_session_id,
                vec![
                    RuntimeHistoryEntry {
                        id: Uuid::new_v4(),
                        external_session_id,
                        external_task_id: None,
                        source_message_id: Some("new-user".to_string()),
                        runtime: "test".to_string(),
                        role: "user".to_string(),
                        content: "summarized question".to_string(),
                        message_json: None,
                    },
                    RuntimeHistoryEntry {
                        id: Uuid::new_v4(),
                        external_session_id,
                        external_task_id: None,
                        source_message_id: Some("new-assistant".to_string()),
                        runtime: "test".to_string(),
                        role: "assistant".to_string(),
                        content: "summarized answer".to_string(),
                        message_json: None,
                    },
                ],
            )
            .await
            .unwrap();

        let history = adapter.list_history(external_session_id).await.unwrap();

        assert_eq!(history.len(), 2);
        assert_eq!(history[0].source_message_id.as_deref(), Some("new-user"));
        assert_eq!(history[0].content, "summarized question");
        assert_eq!(
            history[1].source_message_id.as_deref(),
            Some("new-assistant")
        );
        assert_eq!(history[1].content, "summarized answer");
    }

    #[tokio::test]
    async fn adapter_appends_structured_message_history() {
        let tempdir = tempfile::tempdir().unwrap();
        let manager = SessionManager::new(tempdir.path().to_path_buf());
        let adapter =
            GooseSessionPersistenceAdapter::new(manager, Arc::new(InMemoryLinkStore::default()));
        let external_session_id = Uuid::new_v4();

        adapter
            .link_or_create_session(
                external_session_id,
                tempdir.path().to_path_buf(),
                "runtime structured test".to_string(),
                SessionType::User,
                GooseMode::Chat,
            )
            .await
            .unwrap();

        let message = Message::assistant()
            .with_id("assistant-msg-1")
            .with_tool_request(
                "tool-1",
                Ok(rmcp::model::CallToolRequestParams::new("developer__shell")),
            );

        adapter
            .append_history(RuntimeHistoryEntry {
                id: Uuid::new_v4(),
                external_session_id,
                external_task_id: None,
                source_message_id: message.id.clone(),
                runtime: "test".to_string(),
                role: "assistant".to_string(),
                content: String::new(),
                message_json: Some(serde_json::to_value(&message).unwrap()),
            })
            .await
            .unwrap();

        let history = adapter.list_history(external_session_id).await.unwrap();

        assert_eq!(history.len(), 1);
        assert_eq!(
            history[0].source_message_id.as_deref(),
            Some("assistant-msg-1")
        );
        assert!(history[0].message_json.is_some());
    }

    #[tokio::test]
    async fn adapter_saves_and_loads_runtime_session_record() {
        let tempdir = tempfile::tempdir().unwrap();
        let manager = SessionManager::new(tempdir.path().to_path_buf());
        let adapter =
            GooseSessionPersistenceAdapter::new(manager, Arc::new(InMemoryLinkStore::default()));
        let external_session_id = Uuid::new_v4();
        let record = RuntimeSessionRecord::new(
            external_session_id,
            "goose-session-1",
            RuntimeWorkspaceRef::local_path(tempdir.path()),
        )
        .with_title("Cloud session");

        adapter.save_runtime_session(record).await.unwrap();
        let loaded = adapter
            .load_runtime_session(external_session_id)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(loaded.external_session_id, external_session_id);
        assert_eq!(loaded.runtime_session_id, "goose-session-1");
        assert_eq!(
            loaded.workspace.local_path,
            Some(tempdir.path().to_path_buf())
        );
    }

    #[tokio::test]
    async fn adapter_links_runtime_session_record_with_remote_workspace_metadata() {
        let tempdir = tempfile::tempdir().unwrap();
        let workspace_dir = tempdir.path().join("alice-workspace");
        std::fs::create_dir_all(&workspace_dir).unwrap();

        let manager = SessionManager::new(tempdir.path().to_path_buf());
        let adapter =
            GooseSessionPersistenceAdapter::new(manager, Arc::new(InMemoryLinkStore::default()));
        let external_session_id = Uuid::new_v4();
        let record = RuntimeSessionRecord::new(
            external_session_id,
            "pending-runtime-session",
            RuntimeWorkspaceRef::remote_uri("s3://tenant/session")
                .with_owner("alice")
                .with_mount_name("alice")
                .with_local_path(&workspace_dir),
        )
        .with_title("Cloud session");

        let linked = adapter
            .link_or_create_runtime_session(
                record,
                "runtime remote workspace".to_string(),
                SessionType::User,
                GooseMode::Chat,
            )
            .await
            .unwrap();

        assert_eq!(linked.external_session_id, external_session_id);
        assert_ne!(linked.runtime_session_id, "pending-runtime-session");
        assert_eq!(linked.workspace.owner_id.as_deref(), Some("alice"));
        assert_eq!(
            linked.workspace.remote_uri.as_deref(),
            Some("s3://tenant/session")
        );
        assert_eq!(
            linked.workspace.local_path.as_deref(),
            Some(workspace_dir.as_path())
        );

        let working_dir = adapter
            .working_dir_for_session(external_session_id)
            .await
            .unwrap();
        assert_eq!(working_dir, workspace_dir);
    }
}
