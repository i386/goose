use crate::config::ExtensionConfig;
use crate::model::{ConfigError, ModelConfig};
use crate::providers::base::{ConfigKey, ModelInfo, Provider, ProviderMetadata, ProviderType};
use crate::providers::errors::ProviderError;
use crate::providers::provider_registry::{ProviderEntry, ProviderRegistry};
use anyhow::Result;
use async_trait::async_trait;
use goose_provider_runtime::{
    ProviderCreateRequest, ProviderCreator, ProviderErrorKind, ProviderFailure, ProviderInventory,
    ProviderModelSpec, ProviderRuntime, ProviderRuntimeConfig, ProviderRuntimeConfigKey,
    ProviderRuntimeEntry, ProviderRuntimeFactory, ProviderRuntimeMetadata,
    ProviderRuntimeModelInfo, ProviderRuntimeRegistry, ProviderRuntimeType,
    ProviderStreamingPolicy,
};
use goose_providers::conversation::message::Message;
use rmcp::model::Tool;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone)]
pub struct GooseProviderRuntimeAdapter {
    provider: Arc<dyn Provider>,
}

impl GooseProviderRuntimeAdapter {
    pub fn new(provider: Arc<dyn Provider>) -> Self {
        Self { provider }
    }

    pub fn provider(&self) -> &Arc<dyn Provider> {
        &self.provider
    }

    pub fn into_provider(self) -> Arc<dyn Provider> {
        self.provider
    }
}

#[async_trait]
impl ProviderRuntime for GooseProviderRuntimeAdapter {
    type ModelConfig = ModelConfig;

    fn get_name(&self) -> &str {
        self.provider.get_name()
    }

    fn get_model_config(&self) -> Self::ModelConfig {
        self.provider.get_model_config()
    }

    fn retry_config(&self) -> goose_provider_runtime::RetryConfig {
        self.provider.retry_config()
    }

    async fn stream(
        &self,
        model_config: &Self::ModelConfig,
        session_id: &str,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
    ) -> Result<goose_provider_runtime::MessageStream, ProviderError> {
        self.provider
            .stream(model_config, session_id, system, messages, tools)
            .await
    }
}

pub struct GooseProviderRuntimeFactory;

#[async_trait]
impl ProviderRuntimeFactory for GooseProviderRuntimeFactory {
    type Provider = Arc<dyn Provider>;
    type ExtensionConfig = ExtensionConfig;
    type ModelConfig = ModelConfig;

    fn model_config_from_runtime_config(
        &self,
        config: &ProviderRuntimeConfig,
    ) -> Result<Self::ModelConfig> {
        Ok(model_config_from_runtime_config(config)?)
    }

    async fn create_provider(
        &self,
        config: &ProviderRuntimeConfig,
        extensions: Vec<Self::ExtensionConfig>,
    ) -> Result<Self::Provider> {
        create_provider_from_runtime_config(config, extensions).await
    }

    async fn create_provider_with_working_dir(
        &self,
        config: &ProviderRuntimeConfig,
        extensions: Vec<Self::ExtensionConfig>,
        working_dir: PathBuf,
    ) -> Result<Self::Provider> {
        create_provider_with_working_dir_from_runtime_config(config, extensions, working_dir).await
    }
}

#[async_trait]
impl ProviderCreator for GooseProviderRuntimeFactory {
    type Provider = Arc<dyn Provider>;
    type ExtensionConfig = ExtensionConfig;
    type ModelConfig = ModelConfig;

    async fn create_from_request(
        &self,
        request: ProviderCreateRequest<Self::ModelConfig, Self::ExtensionConfig>,
    ) -> Result<Self::Provider> {
        create_provider_from_request(request).await
    }
}

#[async_trait]
impl ProviderInventory for GooseProviderRuntimeFactory {
    async fn runtime_providers(&self) -> Result<Vec<ProviderRuntimeMetadata>> {
        Ok(runtime_providers().await)
    }
}

#[async_trait]
impl ProviderRuntimeEntry for ProviderEntry {
    type Provider = Arc<dyn Provider>;
    type ExtensionConfig = ExtensionConfig;
    type ModelConfig = ModelConfig;

    fn runtime_metadata(&self) -> ProviderRuntimeMetadata {
        runtime_metadata_from_provider_metadata(self.metadata().clone(), self.provider_type())
    }

    fn supports_inventory_refresh(&self) -> bool {
        ProviderEntry::supports_inventory_refresh(self)
    }

    async fn create(
        &self,
        model_config: Self::ModelConfig,
        extensions: Vec<Self::ExtensionConfig>,
    ) -> Result<Self::Provider> {
        ProviderEntry::create(self, model_config, extensions).await
    }

    async fn create_with_working_dir(
        &self,
        model_config: Self::ModelConfig,
        extensions: Vec<Self::ExtensionConfig>,
        working_dir: PathBuf,
    ) -> Result<Self::Provider> {
        ProviderEntry::create_with_working_dir(self, model_config, extensions, working_dir).await
    }
}

#[async_trait]
impl ProviderRuntimeRegistry for ProviderRegistry {
    type Entry = ProviderEntry;
    type Provider = Arc<dyn Provider>;
    type ExtensionConfig = ExtensionConfig;
    type ModelConfig = ModelConfig;

    async fn runtime_entries(&self) -> Result<Vec<Self::Entry>> {
        Ok(self.entries.values().cloned().collect())
    }

    async fn runtime_entry(&self, provider_name: &str) -> Result<Self::Entry> {
        self.entries
            .get(provider_name)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Unknown provider: {}", provider_name))
    }
}

pub fn model_config_from_runtime_config(
    config: &ProviderRuntimeConfig,
) -> Result<ModelConfig, ConfigError> {
    model_config_from_model_spec(&config.model_spec())
}

pub fn model_config_from_model_spec(spec: &ProviderModelSpec) -> Result<ModelConfig, ConfigError> {
    Ok(ModelConfig::new(&spec.model_name)?
        .with_canonical_limits(&spec.provider_name)
        .with_provider_model_spec(spec, None))
}

pub async fn create_provider_from_runtime_config(
    config: &ProviderRuntimeConfig,
    extensions: Vec<ExtensionConfig>,
) -> Result<Arc<dyn Provider>> {
    let spec = config.model_spec();
    let model_config = model_config_from_model_spec(&spec)?;
    let request = ProviderCreateRequest::from_model_spec(&spec, model_config, extensions);
    GooseProviderRuntimeFactory
        .create_from_request(request)
        .await
}

pub async fn create_runtime_provider_from_runtime_config(
    config: &ProviderRuntimeConfig,
    extensions: Vec<ExtensionConfig>,
) -> Result<GooseProviderRuntimeAdapter> {
    create_provider_from_runtime_config(config, extensions)
        .await
        .map(GooseProviderRuntimeAdapter::new)
}

pub async fn create_provider_with_working_dir_from_runtime_config(
    config: &ProviderRuntimeConfig,
    extensions: Vec<ExtensionConfig>,
    working_dir: PathBuf,
) -> Result<Arc<dyn Provider>> {
    let spec = config.model_spec();
    let model_config = model_config_from_model_spec(&spec)?;
    let request = ProviderCreateRequest::from_model_spec(&spec, model_config, extensions)
        .with_working_dir(working_dir);
    GooseProviderRuntimeFactory
        .create_from_request(request)
        .await
}

pub async fn create_runtime_provider_with_working_dir_from_runtime_config(
    config: &ProviderRuntimeConfig,
    extensions: Vec<ExtensionConfig>,
    working_dir: PathBuf,
) -> Result<GooseProviderRuntimeAdapter> {
    create_provider_with_working_dir_from_runtime_config(config, extensions, working_dir)
        .await
        .map(GooseProviderRuntimeAdapter::new)
}

pub async fn create_provider_from_request(
    request: ProviderCreateRequest<ModelConfig, ExtensionConfig>,
) -> Result<Arc<dyn Provider>> {
    let entry = crate::providers::get_from_registry(&request.provider_name).await?;
    entry.create_from_request(request).await
}

pub async fn create_runtime_provider_from_request(
    request: ProviderCreateRequest<ModelConfig, ExtensionConfig>,
) -> Result<GooseProviderRuntimeAdapter> {
    create_provider_from_request(request)
        .await
        .map(GooseProviderRuntimeAdapter::new)
}

pub fn streaming_policy_from_runtime_config(
    config: &ProviderRuntimeConfig,
) -> ProviderStreamingPolicy {
    config.streaming_policy()
}

pub fn provider_failure_from_error(error: ProviderError) -> ProviderFailure {
    let (kind, retryable) = match &error {
        ProviderError::Authentication(_) => (ProviderErrorKind::Authentication, false),
        ProviderError::ContextLengthExceeded(_) => {
            (ProviderErrorKind::ContextLengthExceeded, false)
        }
        ProviderError::RateLimitExceeded { .. } => (ProviderErrorKind::RateLimited, true),
        ProviderError::CreditsExhausted { .. } => (ProviderErrorKind::CreditsExhausted, false),
        ProviderError::NetworkError(_) => (ProviderErrorKind::Network, true),
        ProviderError::ServerError(_) => (ProviderErrorKind::Server, true),
        ProviderError::EndpointNotFound(_) => (ProviderErrorKind::EndpointNotFound, false),
        ProviderError::NotImplemented(_) => (ProviderErrorKind::Unsupported, false),
        ProviderError::RequestFailed(_) => (ProviderErrorKind::RequestFailed, false),
        ProviderError::ExecutionError(_) => (ProviderErrorKind::Execution, false),
        ProviderError::UsageError(_) => (ProviderErrorKind::Execution, false),
    };

    ProviderFailure::new(kind, error.to_string(), retryable)
}

pub async fn runtime_providers() -> Vec<ProviderRuntimeMetadata> {
    crate::providers::providers()
        .await
        .into_iter()
        .map(|(metadata, provider_type)| {
            runtime_metadata_from_provider_metadata(metadata, provider_type)
        })
        .collect()
}

pub fn runtime_metadata_from_provider_metadata(
    metadata: ProviderMetadata,
    provider_type: ProviderType,
) -> ProviderRuntimeMetadata {
    ProviderRuntimeMetadata {
        name: metadata.name,
        display_name: metadata.display_name,
        description: metadata.description,
        default_model: metadata.default_model,
        known_models: metadata
            .known_models
            .into_iter()
            .map(runtime_model_info_from_model_info)
            .collect(),
        model_doc_link: metadata.model_doc_link,
        config_keys: metadata
            .config_keys
            .into_iter()
            .map(runtime_config_key_from_config_key)
            .collect(),
        setup_steps: metadata.setup_steps,
        model_selection_hint: metadata.model_selection_hint,
        provider_type: runtime_provider_type_from_provider_type(provider_type),
    }
}

pub fn runtime_model_info_from_model_info(model: ModelInfo) -> ProviderRuntimeModelInfo {
    ProviderRuntimeModelInfo {
        name: model.name,
        resolved_model: model.resolved_model,
        context_limit: model.context_limit,
        input_token_cost: model.input_token_cost,
        output_token_cost: model.output_token_cost,
        currency: model.currency,
        supports_cache_control: model.supports_cache_control,
        reasoning: model.reasoning,
    }
}

pub fn runtime_config_key_from_config_key(key: ConfigKey) -> ProviderRuntimeConfigKey {
    ProviderRuntimeConfigKey {
        name: key.name,
        required: key.required,
        secret: key.secret,
        default: key.default,
        oauth_flow: key.oauth_flow,
        device_code_flow: key.device_code_flow,
        primary: key.primary,
    }
}

pub fn runtime_provider_type_from_provider_type(
    provider_type: ProviderType,
) -> ProviderRuntimeType {
    match provider_type {
        ProviderType::Preferred => ProviderRuntimeType::Preferred,
        ProviderType::Builtin => ProviderRuntimeType::Builtin,
        ProviderType::Declarative => ProviderRuntimeType::Declarative,
        ProviderType::Custom => ProviderRuntimeType::Custom,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use goose_provider_runtime::{
        runtime_metadata_from_registry, stream_from_single_message, ProviderInventory,
        ProviderRuntimeEntry, ProviderRuntimeFactory, ProviderRuntimeRegistry, ProviderUsage,
        Usage,
    };
    use goose_providers::conversation::message::MessageContent;

    #[test]
    fn builds_model_config_from_runtime_config() {
        let config = ProviderRuntimeConfig::new("openai", "gpt-4.1").with_max_tokens(Some(2048));

        let model = model_config_from_runtime_config(&config).unwrap();

        assert_eq!(model.model_name, "gpt-4.1");
        assert_eq!(model.max_tokens, Some(2048));
    }

    #[test]
    fn maps_network_provider_errors_to_retryable_failures() {
        let failure = provider_failure_from_error(ProviderError::NetworkError("offline".into()));

        assert_eq!(failure.kind, ProviderErrorKind::Network);
        assert!(failure.retryable);
        assert!(failure.message.contains("offline"));
    }

    #[test]
    fn factory_builds_model_config_from_runtime_config() {
        let config = ProviderRuntimeConfig::new("openai", "gpt-4.1").with_max_tokens(Some(1024));

        let model = GooseProviderRuntimeFactory
            .model_config_from_runtime_config(&config)
            .unwrap();

        assert_eq!(model.model_name, "gpt-4.1");
        assert_eq!(model.max_tokens, Some(1024));
    }

    #[test]
    fn builds_model_config_from_provider_model_spec() {
        let spec = ProviderModelSpec {
            provider_name: "openai".to_string(),
            model_name: "gpt-4.1".to_string(),
            max_tokens: Some(4096),
        };

        let model = model_config_from_model_spec(&spec).unwrap();

        assert_eq!(model.model_name, "gpt-4.1");
        assert_eq!(model.max_tokens, Some(4096));
    }

    #[test]
    fn maps_provider_metadata_to_runtime_metadata() {
        let metadata = ProviderMetadata {
            name: "test".to_string(),
            display_name: "Test".to_string(),
            description: "Test provider".to_string(),
            default_model: "test-model".to_string(),
            known_models: vec![ModelInfo::new("test-model", 1024)],
            model_doc_link: "https://example.com".to_string(),
            config_keys: vec![ConfigKey::new("TEST_API_KEY", true, true, None, true)],
            setup_steps: vec!["Set a key".to_string()],
            model_selection_hint: Some("Managed by provider".to_string()),
        };

        let runtime = runtime_metadata_from_provider_metadata(metadata, ProviderType::Declarative);

        assert_eq!(runtime.name, "test");
        assert_eq!(runtime.provider_type, ProviderRuntimeType::Declarative);
        assert_eq!(runtime.model_names(), vec!["test-model"]);
        assert_eq!(runtime.config_keys[0].name, "TEST_API_KEY");
        assert!(runtime.config_keys[0].secret);
    }

    #[tokio::test]
    async fn factory_lists_runtime_provider_inventory() {
        let providers = GooseProviderRuntimeFactory
            .runtime_providers()
            .await
            .unwrap();

        assert!(!providers.is_empty());
        assert!(providers.iter().any(|provider| !provider.name.is_empty()));
    }

    #[tokio::test]
    async fn provider_entry_exposes_runtime_metadata() {
        let entry = crate::providers::get_from_registry("openai").await.unwrap();

        let metadata = entry.runtime_metadata();

        assert_eq!(metadata.name, "openai");
        assert_eq!(metadata.provider_type, ProviderRuntimeType::Preferred);
        assert!(!metadata.default_model.is_empty());
    }

    struct MockProvider {
        model_config: ModelConfig,
    }

    #[async_trait]
    impl Provider for MockProvider {
        fn get_name(&self) -> &str {
            "mock"
        }

        async fn stream(
            &self,
            model_config: &ModelConfig,
            _session_id: &str,
            _system: &str,
            _messages: &[Message],
            _tools: &[Tool],
        ) -> Result<goose_provider_runtime::MessageStream, ProviderError> {
            Ok(stream_from_single_message(
                Message::assistant().with_text(format!("using {}", model_config.model_name)),
                ProviderUsage::new(
                    model_config.model_name.clone(),
                    Usage::new(Some(3), Some(5), None),
                ),
            ))
        }

        fn get_model_config(&self) -> ModelConfig {
            self.model_config.clone()
        }
    }

    #[tokio::test]
    async fn goose_provider_runtime_adapter_completes_with_runtime_trait() {
        let provider = Arc::new(MockProvider {
            model_config: ModelConfig::new("mock-model").unwrap(),
        });
        let runtime = GooseProviderRuntimeAdapter::new(provider);
        let model_config = runtime.get_model_config();

        let (message, usage) =
            ProviderRuntime::complete(&runtime, &model_config, "session-a", "system", &[], &[])
                .await
                .unwrap();

        assert_eq!(runtime.get_name(), "mock");
        assert_eq!(usage.model, "mock-model");
        assert_eq!(usage.usage.total_tokens, Some(8));
        assert!(matches!(
            &message.content[0],
            MessageContent::Text(text) if text.text == "using mock-model"
        ));
    }

    #[tokio::test]
    async fn provider_registry_exposes_runtime_entries() {
        let mut registry = ProviderRegistry::new();
        registry.register::<crate::providers::openai::OpenAiProvider>(true);

        let entries = registry.runtime_entries().await.unwrap();
        let metadata = runtime_metadata_from_registry(&registry).await.unwrap();
        let openai = registry.runtime_entry("openai").await.unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(metadata[0].name, "openai");
        assert_eq!(openai.runtime_metadata().name, "openai");
        assert!(registry.runtime_entry("missing").await.is_err());
    }
}
