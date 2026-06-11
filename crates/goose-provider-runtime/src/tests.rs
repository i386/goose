use crate::*;
use anyhow::Result;
use async_trait::async_trait;
use goose_providers::conversation::message::{Message, MessageContent};
use rmcp::model::Tool;
use std::path::PathBuf;
use std::time::Duration;

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

    let request = ProviderCreateRequest::from_model_spec(&spec, "model-config", vec!["extension"])
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
