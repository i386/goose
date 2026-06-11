use anyhow::Result;
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
