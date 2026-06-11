use anyhow::Result;
use async_trait::async_trait;
use std::path::PathBuf;

use crate::{ExternalSessionId, RuntimeHistoryEntry, RuntimeSessionRecord, SessionLink};

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
