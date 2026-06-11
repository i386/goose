use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderErrorKind {
    InvalidConfig,
    Authentication,
    ContextLengthExceeded,
    RateLimited,
    CreditsExhausted,
    Network,
    Server,
    EndpointNotFound,
    Unsupported,
    RequestFailed,
    Execution,
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProviderFailure {
    pub kind: ProviderErrorKind,
    pub message: String,
    pub source: Option<String>,
    pub retryable: bool,
}

impl ProviderFailure {
    pub fn new(kind: ProviderErrorKind, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            kind,
            message: message.into(),
            source: None,
            retryable,
        }
    }

    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }
}
