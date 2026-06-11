use anyhow::Result;
use async_trait::async_trait;
use goose_providers::conversation::message::Message;
use rmcp::model::Tool;
use std::path::PathBuf;

use crate::{
    collect_stream, MessageStream, ProviderError, ProviderModelSpec, ProviderRuntimeConfig,
    ProviderRuntimeMetadata, ProviderStreamingPolicy, ProviderUsage, RetryConfig,
};

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
