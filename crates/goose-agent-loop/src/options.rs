use goose_provider_runtime::ProviderRetryPolicy;
use serde::{Deserialize, Serialize};

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
