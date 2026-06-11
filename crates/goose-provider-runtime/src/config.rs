use serde::{Deserialize, Serialize};

use crate::ProviderRetryPolicy;

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
