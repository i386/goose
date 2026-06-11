use anyhow::Result;
use async_trait::async_trait;
use futures::Stream;
use futures::StreamExt;
use goose_providers::conversation::message::{Message, MessageContent};
use rmcp::model::Tool;
use serde::{Deserialize, Serialize};
use std::ops::{Add, AddAssign};
use std::path::PathBuf;
use std::pin::Pin;
use std::time::Duration;
use thiserror::Error;

pub const DEFAULT_MAX_RETRIES: usize = 3;
pub const DEFAULT_INITIAL_RETRY_INTERVAL_MS: u64 = 1000;
pub const DEFAULT_BACKOFF_MULTIPLIER: f64 = 2.0;
pub const DEFAULT_MAX_RETRY_INTERVAL_MS: u64 = 30_000;

#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts.
    max_retries: usize,
    /// Initial interval between retries in milliseconds.
    initial_interval_ms: u64,
    /// Multiplier for exponential backoff.
    backoff_multiplier: f64,
    /// Maximum interval between retries in milliseconds.
    max_interval_ms: u64,
    /// When true, hosts should only retry transient provider failures.
    transient_only: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: DEFAULT_MAX_RETRIES,
            initial_interval_ms: DEFAULT_INITIAL_RETRY_INTERVAL_MS,
            backoff_multiplier: DEFAULT_BACKOFF_MULTIPLIER,
            max_interval_ms: DEFAULT_MAX_RETRY_INTERVAL_MS,
            transient_only: false,
        }
    }
}

impl RetryConfig {
    pub fn new(
        max_retries: usize,
        initial_interval_ms: u64,
        backoff_multiplier: f64,
        max_interval_ms: u64,
    ) -> Self {
        Self {
            max_retries,
            initial_interval_ms,
            backoff_multiplier,
            max_interval_ms,
            transient_only: false,
        }
    }

    pub fn transient_only(mut self) -> Self {
        self.transient_only = true;
        self
    }

    pub fn max_retries(&self) -> usize {
        self.max_retries
    }

    pub fn is_transient_only(&self) -> bool {
        self.transient_only
    }

    pub fn delay_for_attempt(&self, attempt: usize) -> Duration {
        if attempt == 0 {
            return Duration::from_millis(0);
        }

        let exponent = (attempt - 1) as u32;
        let base_delay_ms = (self.initial_interval_ms as f64
            * self.backoff_multiplier.powi(exponent as i32)) as u64;

        let capped_delay_ms = std::cmp::min(base_delay_ms, self.max_interval_ms);

        let jitter_factor_to_avoid_thundering_herd = 0.8 + (rand::random::<f64>() * 0.4);
        let jitter_delay_ms =
            (capped_delay_ms as f64 * jitter_factor_to_avoid_thundering_herd) as u64;

        Duration::from_millis(jitter_delay_ms)
    }
}

#[async_trait]
pub trait ProviderRuntimeFactory: Send + Sync {
    type Provider: Send + Sync;
    type ExtensionConfig: Send;
    type ModelConfig: Send;

    fn model_config_from_runtime_config(
        &self,
        config: &ProviderRuntimeConfig,
    ) -> Result<Self::ModelConfig>;

    async fn create_provider(
        &self,
        config: &ProviderRuntimeConfig,
        extensions: Vec<Self::ExtensionConfig>,
    ) -> Result<Self::Provider>;

    async fn create_provider_with_working_dir(
        &self,
        config: &ProviderRuntimeConfig,
        extensions: Vec<Self::ExtensionConfig>,
        working_dir: PathBuf,
    ) -> Result<Self::Provider>;

    fn streaming_policy_from_runtime_config(
        &self,
        config: &ProviderRuntimeConfig,
    ) -> ProviderStreamingPolicy {
        config.streaming.clone()
    }
}

#[derive(Clone, Debug)]
pub struct ProviderCreateRequest<ModelConfig, ExtensionConfig> {
    pub provider_name: String,
    pub model_config: ModelConfig,
    pub extensions: Vec<ExtensionConfig>,
    pub working_dir: Option<PathBuf>,
}

impl<ModelConfig, ExtensionConfig> ProviderCreateRequest<ModelConfig, ExtensionConfig> {
    pub fn new(
        provider_name: impl Into<String>,
        model_config: ModelConfig,
        extensions: Vec<ExtensionConfig>,
    ) -> Self {
        Self {
            provider_name: provider_name.into(),
            model_config,
            extensions,
            working_dir: None,
        }
    }

    pub fn from_model_spec(
        spec: &ProviderModelSpec,
        model_config: ModelConfig,
        extensions: Vec<ExtensionConfig>,
    ) -> Self {
        Self::new(spec.provider_name.clone(), model_config, extensions)
    }

    pub fn with_working_dir(mut self, working_dir: PathBuf) -> Self {
        self.working_dir = Some(working_dir);
        self
    }
}

#[async_trait]
pub trait ProviderCreator: Send + Sync {
    type Provider: Send + Sync;
    type ExtensionConfig: Send;
    type ModelConfig: Send;

    async fn create_from_request(
        &self,
        request: ProviderCreateRequest<Self::ModelConfig, Self::ExtensionConfig>,
    ) -> Result<Self::Provider>;
}

#[async_trait]
pub trait ProviderInventory: Send + Sync {
    async fn runtime_providers(&self) -> Result<Vec<ProviderRuntimeMetadata>>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PermissionRouting {
    ActionRequired,
    Noop,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, Copy)]
pub struct Usage {
    pub input_tokens: Option<i32>,
    pub output_tokens: Option<i32>,
    pub total_tokens: Option<i32>,
    pub cache_read_input_tokens: Option<i32>,
    pub cache_write_input_tokens: Option<i32>,
}

fn sum_optionals<T>(a: Option<T>, b: Option<T>) -> Option<T>
where
    T: Add<Output = T> + Default,
{
    match (a, b) {
        (Some(x), Some(y)) => Some(x + y),
        (Some(x), None) => Some(x + T::default()),
        (None, Some(y)) => Some(T::default() + y),
        (None, None) => None,
    }
}

impl Add for Usage {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self::new(
            sum_optionals(self.input_tokens, other.input_tokens),
            sum_optionals(self.output_tokens, other.output_tokens),
            sum_optionals(self.total_tokens, other.total_tokens),
        )
        .with_cache_tokens(
            sum_optionals(self.cache_read_input_tokens, other.cache_read_input_tokens),
            sum_optionals(
                self.cache_write_input_tokens,
                other.cache_write_input_tokens,
            ),
        )
    }
}

impl AddAssign for Usage {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl Usage {
    pub fn new(
        input_tokens: Option<i32>,
        output_tokens: Option<i32>,
        total_tokens: Option<i32>,
    ) -> Self {
        let calculated_total = if total_tokens.is_none() {
            match (input_tokens, output_tokens) {
                (Some(input), Some(output)) => Some(input + output),
                (Some(input), None) => Some(input),
                (None, Some(output)) => Some(output),
                (None, None) => None,
            }
        } else {
            total_tokens
        };

        Self {
            input_tokens,
            output_tokens,
            total_tokens: calculated_total,
            cache_read_input_tokens: None,
            cache_write_input_tokens: None,
        }
    }

    pub fn with_cache_tokens(
        mut self,
        cache_read_input_tokens: Option<i32>,
        cache_write_input_tokens: Option<i32>,
    ) -> Self {
        self.cache_read_input_tokens = cache_read_input_tokens;
        self.cache_write_input_tokens = cache_write_input_tokens;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderUsage {
    pub model: String,
    pub usage: Usage,
}

impl ProviderUsage {
    pub fn new(model: String, usage: Usage) -> Self {
        Self { model, usage }
    }

    /// Combine this ProviderUsage with another, adding their token counts.
    /// Uses the model from this ProviderUsage.
    pub fn combine_with(&self, other: &ProviderUsage) -> ProviderUsage {
        ProviderUsage {
            model: self.model.clone(),
            usage: self.usage + other.usage,
        }
    }
}

/// A provider stream yields partial text content and complete structured content
/// inside Message values.
pub type MessageStream = Pin<
    Box<dyn Stream<Item = Result<(Option<Message>, Option<ProviderUsage>), ProviderError>> + Send>,
>;

pub fn stream_from_single_message(message: Message, usage: ProviderUsage) -> MessageStream {
    let stream = futures::stream::once(async move { Ok((Some(message), Some(usage))) });
    Box::pin(stream)
}

/// Collect all chunks from a MessageStream into a single Message and ProviderUsage.
pub async fn collect_stream(
    mut stream: MessageStream,
) -> Result<(Message, ProviderUsage), ProviderError> {
    let mut final_message: Option<Message> = None;
    let mut final_usage: Option<ProviderUsage> = None;

    while let Some(result) = stream.next().await {
        let (msg_opt, usage_opt) = result?;

        if let Some(msg) = msg_opt {
            final_message = Some(match final_message {
                Some(mut prev) => {
                    for new_content in msg.content {
                        match (&mut prev.content.last_mut(), &new_content) {
                            (
                                Some(MessageContent::Text(last_text)),
                                MessageContent::Text(new_text),
                            ) => {
                                last_text.text.push_str(&new_text.text);
                            }
                            _ => {
                                prev.content.push(new_content);
                            }
                        }
                    }
                    prev
                }
                None => msg,
            });
        }

        if let Some(usage) = usage_opt {
            final_usage = Some(usage);
        }
    }

    match final_message {
        Some(msg) => {
            let usage = final_usage
                .unwrap_or_else(|| ProviderUsage::new("unknown".to_string(), Usage::default()));
            Ok((msg, usage))
        }
        None => Err(ProviderError::ExecutionError(
            "Stream yielded no message".to_string(),
        )),
    }
}

#[async_trait]
pub trait ProviderRuntime: Send + Sync {
    type ModelConfig: Send + Sync;

    fn get_name(&self) -> &str;

    fn get_model_config(&self) -> Self::ModelConfig;

    fn retry_config(&self) -> RetryConfig {
        RetryConfig::default()
    }

    async fn stream(
        &self,
        model_config: &Self::ModelConfig,
        session_id: &str,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
    ) -> Result<MessageStream, ProviderError>;

    async fn complete(
        &self,
        model_config: &Self::ModelConfig,
        session_id: &str,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
    ) -> Result<(Message, ProviderUsage), ProviderError> {
        let stream = self
            .stream(model_config, session_id, system, messages, tools)
            .await?;
        collect_stream(stream).await
    }
}

#[async_trait]
pub trait ProviderRuntimeEntry: Send + Sync {
    type Provider: Send + Sync;
    type ExtensionConfig: Send;
    type ModelConfig: Send;

    fn runtime_metadata(&self) -> ProviderRuntimeMetadata;

    fn supports_inventory_refresh(&self) -> bool {
        false
    }

    async fn create(
        &self,
        model_config: Self::ModelConfig,
        extensions: Vec<Self::ExtensionConfig>,
    ) -> Result<Self::Provider>;

    async fn create_with_working_dir(
        &self,
        model_config: Self::ModelConfig,
        extensions: Vec<Self::ExtensionConfig>,
        working_dir: PathBuf,
    ) -> Result<Self::Provider>;

    async fn create_from_request(
        &self,
        request: ProviderCreateRequest<Self::ModelConfig, Self::ExtensionConfig>,
    ) -> Result<Self::Provider> {
        match request.working_dir {
            Some(working_dir) => {
                self.create_with_working_dir(request.model_config, request.extensions, working_dir)
                    .await
            }
            None => self.create(request.model_config, request.extensions).await,
        }
    }
}

#[async_trait]
pub trait ProviderRuntimeRegistry: Send + Sync {
    type Entry: ProviderRuntimeEntry<
        Provider = Self::Provider,
        ExtensionConfig = Self::ExtensionConfig,
        ModelConfig = Self::ModelConfig,
    >;
    type Provider: Send + Sync;
    type ExtensionConfig: Send;
    type ModelConfig: Send;

    async fn runtime_entries(&self) -> Result<Vec<Self::Entry>>;

    async fn runtime_entry(&self, provider_name: &str) -> Result<Self::Entry>;
}

pub async fn runtime_metadata_from_registry<Registry>(
    registry: &Registry,
) -> Result<Vec<ProviderRuntimeMetadata>>
where
    Registry: ProviderRuntimeRegistry + ?Sized,
{
    let entries = registry.runtime_entries().await?;
    Ok(entries
        .into_iter()
        .map(|entry| entry.runtime_metadata())
        .collect())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProviderRuntimeConfig {
    pub provider_name: String,
    pub model_name: String,
    pub max_tokens: Option<i32>,
    pub streaming: ProviderStreamingPolicy,
    pub retry: ProviderRetryPolicy,
}

impl ProviderRuntimeConfig {
    pub fn new(provider_name: impl Into<String>, model_name: impl Into<String>) -> Self {
        Self {
            provider_name: provider_name.into(),
            model_name: model_name.into(),
            max_tokens: None,
            streaming: ProviderStreamingPolicy::default(),
            retry: ProviderRetryPolicy::default(),
        }
    }

    pub fn with_max_tokens(mut self, max_tokens: Option<i32>) -> Self {
        self.max_tokens = max_tokens;
        self
    }

    pub fn with_streaming(mut self, streaming: ProviderStreamingPolicy) -> Self {
        self.streaming = streaming;
        self
    }

    pub fn with_retry(mut self, retry: ProviderRetryPolicy) -> Self {
        self.retry = retry;
        self
    }

    pub fn model_spec(&self) -> ProviderModelSpec {
        ProviderModelSpec {
            provider_name: self.provider_name.clone(),
            model_name: self.model_name.clone(),
            max_tokens: self.max_tokens,
        }
    }

    pub fn streaming_policy(&self) -> ProviderStreamingPolicy {
        self.streaming.clone()
    }

    pub fn retry_policy(&self) -> ProviderRetryPolicy {
        self.retry.clone()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProviderModelSpec {
    pub provider_name: String,
    pub model_name: String,
    pub max_tokens: Option<i32>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProviderModelHints {
    pub context_limit: usize,
    pub output_limit: Option<usize>,
    pub reasoning: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProviderModelConfigSnapshot {
    pub model_name: String,
    pub context_limit: Option<usize>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<i32>,
    pub toolshim: bool,
    pub toolshim_model: Option<String>,
    pub request_params: Option<serde_json::Map<String, serde_json::Value>>,
    pub reasoning: Option<bool>,
    pub fast_model: Option<Box<ProviderModelConfigSnapshot>>,
}

impl ProviderModelConfigSnapshot {
    pub fn new(model_name: impl Into<String>) -> Self {
        Self {
            model_name: model_name.into(),
            context_limit: None,
            temperature: None,
            max_tokens: None,
            toolshim: false,
            toolshim_model: None,
            request_params: None,
            reasoning: None,
            fast_model: None,
        }
    }

    pub fn model_spec(&self, provider_name: impl Into<String>) -> ProviderModelSpec {
        ProviderModelSpec {
            provider_name: provider_name.into(),
            model_name: self.model_name.clone(),
            max_tokens: self.max_tokens,
        }
    }
}

pub trait ProviderModelConfigSnapshotSource {
    fn to_provider_model_config_snapshot(&self) -> ProviderModelConfigSnapshot;
}

pub trait ProviderModelConfigTarget {
    fn model_name(&self) -> &str;
    fn context_limit(&self) -> Option<usize>;
    fn max_tokens(&self) -> Option<i32>;
    fn reasoning(&self) -> Option<bool>;
    fn set_context_limit(&mut self, context_limit: Option<usize>);
    fn set_max_tokens(&mut self, max_tokens: Option<i32>);
    fn set_reasoning(&mut self, reasoning: Option<bool>);
}

pub fn apply_provider_model_spec<T>(
    target: &mut T,
    spec: &ProviderModelSpec,
    hints: Option<&ProviderModelHints>,
) where
    T: ProviderModelConfigTarget + ?Sized,
{
    if let Some(max_tokens) = spec.max_tokens {
        target.set_max_tokens(Some(max_tokens));
    }

    apply_provider_model_hints(target, hints);
}

pub fn apply_provider_model_hints<T>(target: &mut T, hints: Option<&ProviderModelHints>)
where
    T: ProviderModelConfigTarget + ?Sized,
{
    let Some(hints) = hints else {
        return;
    };

    if target.context_limit().is_none() {
        target.set_context_limit(Some(hints.context_limit));
    }

    if target.max_tokens().is_none() {
        if let Some(output_limit) = hints
            .output_limit
            .filter(|output_limit| *output_limit < hints.context_limit)
        {
            target.set_max_tokens(Some(output_limit as i32));
        }
    }

    if target.reasoning().is_none() {
        target.set_reasoning(hints.reasoning);
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProviderStreamingPolicy {
    pub enabled: bool,
    pub prefer_incremental_events: bool,
}

impl Default for ProviderStreamingPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            prefer_incremental_events: true,
        }
    }
}

impl ProviderStreamingPolicy {
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            prefer_incremental_events: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProviderRetryPolicy {
    pub max_attempts: u32,
    pub retry_transient_errors: bool,
    pub timeout_seconds: Option<u64>,
}

impl Default for ProviderRetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 1,
            retry_transient_errors: true,
            timeout_seconds: None,
        }
    }
}

impl ProviderRetryPolicy {
    pub fn to_retry_config(&self) -> RetryConfig {
        let max_retries = self.max_attempts.saturating_sub(1) as usize;
        let config = RetryConfig::new(
            max_retries,
            DEFAULT_INITIAL_RETRY_INTERVAL_MS,
            DEFAULT_BACKOFF_MULTIPLIER,
            DEFAULT_MAX_RETRY_INTERVAL_MS,
        );

        if self.retry_transient_errors {
            config.transient_only()
        } else {
            config
        }
    }
}

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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderRuntimeType {
    Preferred,
    Builtin,
    Declarative,
    Custom,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProviderRuntimeModelInfo {
    pub name: String,
    pub resolved_model: Option<String>,
    pub context_limit: usize,
    pub input_token_cost: Option<f64>,
    pub output_token_cost: Option<f64>,
    pub currency: Option<String>,
    pub supports_cache_control: Option<bool>,
    pub reasoning: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProviderRuntimeConfigKey {
    pub name: String,
    pub required: bool,
    pub secret: bool,
    pub default: Option<String>,
    pub oauth_flow: bool,
    pub device_code_flow: bool,
    pub primary: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProviderRuntimeMetadata {
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub default_model: String,
    pub known_models: Vec<ProviderRuntimeModelInfo>,
    pub model_doc_link: String,
    pub config_keys: Vec<ProviderRuntimeConfigKey>,
    pub setup_steps: Vec<String>,
    pub model_selection_hint: Option<String>,
    pub provider_type: ProviderRuntimeType,
}

impl ProviderRuntimeMetadata {
    pub fn model_names(&self) -> Vec<&str> {
        self.known_models
            .iter()
            .map(|model| model.name.as_str())
            .collect()
    }
}

#[derive(Error, Debug, Clone, PartialEq)]
pub enum ProviderError {
    #[error("Authentication error: {0}")]
    Authentication(String),

    #[error("Context length exceeded: {0}")]
    ContextLengthExceeded(String),

    #[error("Rate limit exceeded: {details}")]
    RateLimitExceeded {
        details: String,
        retry_delay: Option<Duration>,
    },

    #[error("Server error: {0}")]
    ServerError(String),

    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("Request failed: {0}")]
    RequestFailed(String),

    #[error("Execution error: {0}")]
    ExecutionError(String),

    #[error("Usage data error: {0}")]
    UsageError(String),

    #[error("Unsupported operation: {0}")]
    NotImplemented(String),

    #[error("Endpoint not found (404): {0}")]
    EndpointNotFound(String),

    #[error("Credits exhausted: {details}")]
    CreditsExhausted {
        details: String,
        top_up_url: Option<String>,
    },
}

impl ProviderError {
    pub fn telemetry_type(&self) -> &'static str {
        match self {
            ProviderError::Authentication(_) => "auth",
            ProviderError::ContextLengthExceeded(_) => "context_length",
            ProviderError::RateLimitExceeded { .. } => "rate_limit",
            ProviderError::ServerError(_) => "server",
            ProviderError::NetworkError(_) => "network",
            ProviderError::RequestFailed(_) => "request",
            ProviderError::ExecutionError(_) => "execution",
            ProviderError::UsageError(_) => "usage",
            ProviderError::NotImplemented(_) => "not_implemented",
            ProviderError::EndpointNotFound(_) => "endpoint_not_found",
            ProviderError::CreditsExhausted { .. } => "credits_exhausted",
        }
    }

    pub fn is_endpoint_not_found(&self) -> bool {
        matches!(self, ProviderError::EndpointNotFound(_))
    }
}

fn is_network_error(err: &reqwest::Error) -> bool {
    err.is_connect() || err.is_timeout() || (err.status().is_none() && err.is_request())
}

fn provider_error_from_reqwest(error: &reqwest::Error) -> ProviderError {
    if is_network_error(error) {
        let msg = if error.is_timeout() {
            "Request timed out - check your network connection and try again.".to_string()
        } else if error.is_connect() {
            if let Some(url) = error.url() {
                if let Some(host) = url.host_str() {
                    let port_info = url.port().map(|p| format!(":{p}")).unwrap_or_default();
                    format!(
                        "Could not connect to {host}{port_info} - check your network connection and try again."
                    )
                } else {
                    "Could not connect to the provider - check your network connection and try again.".to_string()
                }
            } else {
                "Could not connect to the provider - check your network connection and try again."
                    .to_string()
            }
        } else {
            "Network error - check your network connection and try again.".to_string()
        };
        return ProviderError::NetworkError(msg);
    }

    let mut details = vec![];
    if let Some(status) = error.status() {
        details.push(format!("status: {status}"));
    }
    let msg = if details.is_empty() {
        error.to_string()
    } else {
        format!("{} ({})", error, details.join(", "))
    };
    ProviderError::RequestFailed(msg)
}

impl From<anyhow::Error> for ProviderError {
    fn from(error: anyhow::Error) -> Self {
        if let Some(reqwest_err) = error.downcast_ref::<reqwest::Error>() {
            return provider_error_from_reqwest(reqwest_err);
        }
        ProviderError::ExecutionError(error.to_string())
    }
}

impl From<reqwest::Error> for ProviderError {
    fn from(error: reqwest::Error) -> Self {
        provider_error_from_reqwest(&error)
    }
}

#[derive(Debug)]
pub enum GoogleErrorCode {
    BadRequest = 400,
    Unauthorized = 401,
    Forbidden = 403,
    NotFound = 404,
    TooManyRequests = 429,
    InternalServerError = 500,
    ServiceUnavailable = 503,
}

impl GoogleErrorCode {
    pub fn to_status_code(&self) -> reqwest::StatusCode {
        match self {
            Self::BadRequest => reqwest::StatusCode::BAD_REQUEST,
            Self::Unauthorized => reqwest::StatusCode::UNAUTHORIZED,
            Self::Forbidden => reqwest::StatusCode::FORBIDDEN,
            Self::NotFound => reqwest::StatusCode::NOT_FOUND,
            Self::TooManyRequests => reqwest::StatusCode::TOO_MANY_REQUESTS,
            Self::InternalServerError => reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            Self::ServiceUnavailable => reqwest::StatusCode::SERVICE_UNAVAILABLE,
        }
    }

    pub fn from_code(code: u64) -> Option<Self> {
        match code {
            400 => Some(Self::BadRequest),
            401 => Some(Self::Unauthorized),
            403 => Some(Self::Forbidden),
            404 => Some(Self::NotFound),
            429 => Some(Self::TooManyRequests),
            500 => Some(Self::InternalServerError),
            503 => Some(Self::ServiceUnavailable),
            _ => Some(Self::InternalServerError),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct TestModelConfig {
        model_name: String,
        context_limit: Option<usize>,
        max_tokens: Option<i32>,
        reasoning: Option<bool>,
    }

    impl ProviderModelConfigTarget for TestModelConfig {
        fn model_name(&self) -> &str {
            &self.model_name
        }

        fn context_limit(&self) -> Option<usize> {
            self.context_limit
        }

        fn max_tokens(&self) -> Option<i32> {
            self.max_tokens
        }

        fn reasoning(&self) -> Option<bool> {
            self.reasoning
        }

        fn set_context_limit(&mut self, context_limit: Option<usize>) {
            self.context_limit = context_limit;
        }

        fn set_max_tokens(&mut self, max_tokens: Option<i32>) {
            self.max_tokens = max_tokens;
        }

        fn set_reasoning(&mut self, reasoning: Option<bool>) {
            self.reasoning = reasoning;
        }
    }

    #[test]
    fn retry_config_defaults_to_goose_provider_backoff() {
        let config = RetryConfig::default();

        assert_eq!(config.max_retries(), DEFAULT_MAX_RETRIES);
        assert!(!config.is_transient_only());
        assert_eq!(config.delay_for_attempt(0), Duration::from_millis(0));
    }

    #[test]
    fn retry_config_can_be_transient_only() {
        let config = RetryConfig::new(5, 100, 2.0, 1_000).transient_only();

        assert_eq!(config.max_retries(), 5);
        assert!(config.is_transient_only());
    }

    #[test]
    fn retry_delay_is_jittered_and_capped() {
        let config = RetryConfig::new(3, 1_000, 2.0, 3_000);

        let delay = config.delay_for_attempt(3);

        assert!(delay >= Duration::from_millis(2_400));
        assert!(delay <= Duration::from_millis(3_600));
    }

    #[test]
    fn runtime_config_exposes_model_spec_and_policies() {
        let streaming = ProviderStreamingPolicy::disabled();
        let retry = ProviderRetryPolicy {
            max_attempts: 4,
            retry_transient_errors: true,
            timeout_seconds: Some(30),
        };
        let config = ProviderRuntimeConfig::new("openai", "gpt-4.1")
            .with_max_tokens(Some(2048))
            .with_streaming(streaming.clone())
            .with_retry(retry.clone());

        assert_eq!(
            config.model_spec(),
            ProviderModelSpec {
                provider_name: "openai".to_string(),
                model_name: "gpt-4.1".to_string(),
                max_tokens: Some(2048),
            }
        );
        assert!(!config.streaming_policy().enabled);
        assert_eq!(config.retry_policy().max_attempts, 4);
    }

    #[test]
    fn provider_model_config_snapshot_exposes_model_spec() {
        let mut snapshot = ProviderModelConfigSnapshot::new("gpt-4.1");
        snapshot.max_tokens = Some(2048);
        snapshot.context_limit = Some(128_000);
        snapshot.request_params = Some(serde_json::Map::from_iter([(
            "reasoning_effort".to_string(),
            serde_json::json!("low"),
        )]));

        let spec = snapshot.model_spec("openai");

        assert_eq!(
            spec,
            ProviderModelSpec {
                provider_name: "openai".to_string(),
                model_name: "gpt-4.1".to_string(),
                max_tokens: Some(2048),
            }
        );
        assert_eq!(snapshot.context_limit, Some(128_000));
        assert_eq!(
            snapshot
                .request_params
                .as_ref()
                .unwrap()
                .get("reasoning_effort"),
            Some(&serde_json::json!("low"))
        );
    }

    #[test]
    fn applies_provider_model_spec_and_hints_to_model_config_target() {
        let mut target = TestModelConfig {
            model_name: "gpt-4.1".to_string(),
            ..Default::default()
        };
        let spec = ProviderModelSpec {
            provider_name: "openai".to_string(),
            model_name: "gpt-4.1".to_string(),
            max_tokens: Some(2048),
        };
        let hints = ProviderModelHints {
            context_limit: 128_000,
            output_limit: Some(16_384),
            reasoning: Some(false),
        };

        apply_provider_model_spec(&mut target, &spec, Some(&hints));

        assert_eq!(target.context_limit, Some(128_000));
        assert_eq!(target.max_tokens, Some(2048));
        assert_eq!(target.reasoning, Some(false));
    }

    #[test]
    fn provider_model_hints_preserve_explicit_values() {
        let mut target = TestModelConfig {
            model_name: "gpt-4.1".to_string(),
            context_limit: Some(64_000),
            max_tokens: Some(4096),
            reasoning: Some(true),
        };
        let hints = ProviderModelHints {
            context_limit: 128_000,
            output_limit: Some(16_384),
            reasoning: Some(false),
        };

        apply_provider_model_hints(&mut target, Some(&hints));

        assert_eq!(target.context_limit, Some(64_000));
        assert_eq!(target.max_tokens, Some(4096));
        assert_eq!(target.reasoning, Some(true));
    }

    #[test]
    fn provider_model_hints_skip_output_limit_equal_to_context() {
        let mut target = TestModelConfig {
            model_name: "moonshotai/kimi-k2.6".to_string(),
            ..Default::default()
        };
        let hints = ProviderModelHints {
            context_limit: 128_000,
            output_limit: Some(128_000),
            reasoning: None,
        };

        apply_provider_model_hints(&mut target, Some(&hints));

        assert_eq!(target.context_limit, Some(128_000));
        assert_eq!(target.max_tokens, None);
    }

    #[test]
    fn retry_policy_converts_to_provider_retry_config() {
        let policy = ProviderRetryPolicy {
            max_attempts: 3,
            retry_transient_errors: true,
            timeout_seconds: None,
        };

        let retry = policy.to_retry_config();

        assert_eq!(retry.max_retries(), 2);
        assert!(retry.is_transient_only());
    }

    #[test]
    fn provider_failure_can_be_built_with_source() {
        let failure =
            ProviderFailure::new(ProviderErrorKind::Network, "offline", true).with_source("openai");

        assert_eq!(failure.kind, ProviderErrorKind::Network);
        assert_eq!(failure.message, "offline");
        assert_eq!(failure.source.as_deref(), Some("openai"));
        assert!(failure.retryable);
    }

    #[test]
    fn builds_provider_create_request_from_model_spec() {
        let spec = ProviderModelSpec {
            provider_name: "openai".to_string(),
            model_name: "gpt-4.1".to_string(),
            max_tokens: Some(2048),
        };

        let request =
            ProviderCreateRequest::from_model_spec(&spec, "model-config", vec!["extension"])
                .with_working_dir(PathBuf::from("/tmp/workspace"));

        assert_eq!(request.provider_name, "openai");
        assert_eq!(request.model_config, "model-config");
        assert_eq!(request.extensions, vec!["extension"]);
        assert_eq!(request.working_dir, Some(PathBuf::from("/tmp/workspace")));
    }

    #[test]
    fn provider_error_reports_telemetry_type() {
        assert_eq!(
            ProviderError::Authentication("bad key".to_string()).telemetry_type(),
            "auth"
        );
        assert_eq!(
            ProviderError::CreditsExhausted {
                details: "empty".to_string(),
                top_up_url: None,
            }
            .telemetry_type(),
            "credits_exhausted"
        );
    }

    #[test]
    fn google_error_code_maps_status_codes() {
        assert_eq!(
            GoogleErrorCode::from_code(429).unwrap().to_status_code(),
            reqwest::StatusCode::TOO_MANY_REQUESTS
        );
        assert_eq!(
            GoogleErrorCode::from_code(599).unwrap().to_status_code(),
            reqwest::StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[test]
    fn provider_runtime_metadata_exposes_model_names() {
        let metadata = ProviderRuntimeMetadata {
            name: "openai".to_string(),
            display_name: "OpenAI".to_string(),
            description: "OpenAI models".to_string(),
            default_model: "gpt-4.1".to_string(),
            known_models: vec![ProviderRuntimeModelInfo {
                name: "gpt-4.1".to_string(),
                resolved_model: None,
                context_limit: 128_000,
                input_token_cost: None,
                output_token_cost: None,
                currency: None,
                supports_cache_control: None,
                reasoning: false,
            }],
            model_doc_link: "https://example.com".to_string(),
            config_keys: vec![ProviderRuntimeConfigKey {
                name: "OPENAI_API_KEY".to_string(),
                required: true,
                secret: true,
                default: None,
                oauth_flow: false,
                device_code_flow: false,
                primary: true,
            }],
            setup_steps: Vec::new(),
            model_selection_hint: None,
            provider_type: ProviderRuntimeType::Preferred,
        };

        assert_eq!(metadata.model_names(), vec!["gpt-4.1"]);
    }

    #[test]
    fn provider_usage_combines_token_counts() {
        let first = ProviderUsage::new(
            "model-a".to_string(),
            Usage::new(Some(10), Some(5), None).with_cache_tokens(Some(3), None),
        );
        let second = ProviderUsage::new(
            "model-b".to_string(),
            Usage::new(Some(2), Some(4), None).with_cache_tokens(Some(1), Some(2)),
        );

        let combined = first.combine_with(&second);

        assert_eq!(combined.model, "model-a");
        assert_eq!(combined.usage.input_tokens, Some(12));
        assert_eq!(combined.usage.output_tokens, Some(9));
        assert_eq!(combined.usage.total_tokens, Some(21));
        assert_eq!(combined.usage.cache_read_input_tokens, Some(4));
        assert_eq!(combined.usage.cache_write_input_tokens, Some(2));
    }

    #[tokio::test]
    async fn collect_stream_coalesces_text_chunks() {
        let stream: MessageStream = Box::pin(futures::stream::iter(vec![
            Ok::<_, ProviderError>((Some(Message::assistant().with_text("Hello")), None)),
            Ok::<_, ProviderError>((Some(Message::assistant().with_text(" world")), None)),
        ]));

        let (message, usage) = collect_stream(stream).await.unwrap();

        assert_eq!(usage.model, "unknown");
        assert_eq!(message.content.len(), 1);
        assert!(matches!(
            &message.content[0],
            MessageContent::Text(text) if text.text == "Hello world"
        ));
    }

    #[tokio::test]
    async fn stream_from_single_message_returns_message_and_usage() {
        let usage = ProviderUsage::new("model-a".to_string(), Usage::new(Some(1), Some(2), None));
        let stream = stream_from_single_message(Message::assistant().with_text("done"), usage);

        let (message, usage) = collect_stream(stream).await.unwrap();

        assert_eq!(usage.model, "model-a");
        assert_eq!(usage.usage.total_tokens, Some(3));
        assert!(matches!(
            &message.content[0],
            MessageContent::Text(text) if text.text == "done"
        ));
    }

    struct TestProviderRuntime;

    #[async_trait]
    impl ProviderRuntime for TestProviderRuntime {
        type ModelConfig = String;

        fn get_name(&self) -> &str {
            "test"
        }

        fn get_model_config(&self) -> Self::ModelConfig {
            "model-a".to_string()
        }

        async fn stream(
            &self,
            model_config: &Self::ModelConfig,
            _session_id: &str,
            _system: &str,
            _messages: &[Message],
            _tools: &[Tool],
        ) -> Result<MessageStream, ProviderError> {
            Ok(stream_from_single_message(
                Message::assistant().with_text(format!("using {model_config}")),
                ProviderUsage::new(model_config.clone(), Usage::new(Some(1), Some(1), None)),
            ))
        }
    }

    #[tokio::test]
    async fn provider_runtime_default_complete_collects_stream() {
        let runtime = TestProviderRuntime;
        let model_config = runtime.get_model_config();

        let (message, usage) = runtime
            .complete(&model_config, "session-a", "system", &[], &[])
            .await
            .unwrap();

        assert_eq!(runtime.get_name(), "test");
        assert_eq!(usage.model, "model-a");
        assert!(matches!(
            &message.content[0],
            MessageContent::Text(text) if text.text == "using model-a"
        ));
    }

    #[derive(Clone)]
    struct TestRuntimeEntry {
        metadata: ProviderRuntimeMetadata,
    }

    struct TestRuntimeRegistry {
        entries: Vec<TestRuntimeEntry>,
    }

    #[async_trait]
    impl ProviderRuntimeEntry for TestRuntimeEntry {
        type Provider = String;
        type ExtensionConfig = String;
        type ModelConfig = String;

        fn runtime_metadata(&self) -> ProviderRuntimeMetadata {
            self.metadata.clone()
        }

        async fn create(
            &self,
            model_config: Self::ModelConfig,
            _extensions: Vec<Self::ExtensionConfig>,
        ) -> Result<Self::Provider> {
            Ok(format!("{}:{model_config}", self.metadata.name))
        }

        async fn create_with_working_dir(
            &self,
            model_config: Self::ModelConfig,
            _extensions: Vec<Self::ExtensionConfig>,
            working_dir: PathBuf,
        ) -> Result<Self::Provider> {
            Ok(format!(
                "{}:{model_config}:{}",
                self.metadata.name,
                working_dir.display()
            ))
        }
    }

    #[async_trait]
    impl ProviderRuntimeRegistry for TestRuntimeRegistry {
        type Entry = TestRuntimeEntry;
        type Provider = String;
        type ExtensionConfig = String;
        type ModelConfig = String;

        async fn runtime_entries(&self) -> Result<Vec<Self::Entry>> {
            Ok(self.entries.clone())
        }

        async fn runtime_entry(&self, provider_name: &str) -> Result<Self::Entry> {
            self.entries
                .iter()
                .find(|entry| entry.metadata.name == provider_name)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("missing provider"))
        }
    }

    #[tokio::test]
    async fn runtime_entry_creates_from_request() {
        let entry = TestRuntimeEntry {
            metadata: ProviderRuntimeMetadata {
                name: "test".to_string(),
                display_name: "Test".to_string(),
                description: "Test provider".to_string(),
                default_model: "test-model".to_string(),
                known_models: Vec::new(),
                model_doc_link: "https://example.com".to_string(),
                config_keys: Vec::new(),
                setup_steps: Vec::new(),
                model_selection_hint: None,
                provider_type: ProviderRuntimeType::Builtin,
            },
        };
        let request = ProviderCreateRequest::new("test", "model-a".to_string(), Vec::new())
            .with_working_dir(PathBuf::from("/tmp/work"));

        let provider = entry.create_from_request(request).await.unwrap();

        assert_eq!(provider, "test:model-a:/tmp/work");
    }

    #[tokio::test]
    async fn registry_metadata_helper_projects_entries() {
        let registry = TestRuntimeRegistry {
            entries: vec![TestRuntimeEntry {
                metadata: ProviderRuntimeMetadata {
                    name: "test".to_string(),
                    display_name: "Test".to_string(),
                    description: "Test provider".to_string(),
                    default_model: "test-model".to_string(),
                    known_models: Vec::new(),
                    model_doc_link: "https://example.com".to_string(),
                    config_keys: Vec::new(),
                    setup_steps: Vec::new(),
                    model_selection_hint: None,
                    provider_type: ProviderRuntimeType::Builtin,
                },
            }],
        };

        let metadata = runtime_metadata_from_registry(&registry).await.unwrap();

        assert_eq!(metadata.len(), 1);
        assert_eq!(metadata[0].name, "test");
    }
}
