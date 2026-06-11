use async_trait::async_trait;
use goose_session_runtime::ExternalSessionId;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::{
    take_invocation_argument_object, ToolInvocationArgumentError, ToolInvocationArguments,
};

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
