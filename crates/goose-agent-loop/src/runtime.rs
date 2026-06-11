use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;
use goose_runtime_policy::PromptPolicy;
use goose_tool_runtime::ToolRuntimePolicy;
use tokio_util::sync::CancellationToken;

use crate::{
    source_event_stream_to_loop_event_stream, AgentLoopEvent, AgentLoopEventStream,
    AgentLoopOptions, AgentLoopSessionSpec, AgentLoopSourceEventStream,
};

pub struct AgentLoopRequest<UserMessage> {
    pub user_message: UserMessage,
    pub runtime_session_id: String,
    pub options: AgentLoopOptions,
    pub prompt_policy: PromptPolicy,
    pub tool_policy: Option<ToolRuntimePolicy>,
    pub cancellation_token: Option<CancellationToken>,
}

#[async_trait]
pub trait AgentLoopRuntime: Send + Sync {
    type UserMessage: Send + 'static;

    async fn run_loop<'a>(
        &'a self,
        request: AgentLoopRequest<Self::UserMessage>,
    ) -> Result<AgentLoopEventStream<'a>>;
}

#[async_trait]
pub trait AgentLoopSourceRuntime: Send + Sync {
    type UserMessage: Send + 'static;

    async fn apply_prompt_policy(&self, policy: &PromptPolicy) -> Result<()>;

    async fn enforce_tool_policy(
        &self,
        runtime_session_id: &str,
        policy: Option<&ToolRuntimePolicy>,
    ) -> Result<()>;

    async fn run_source_loop<'a>(
        &'a self,
        user_message: Self::UserMessage,
        session_spec: AgentLoopSessionSpec,
        cancellation_token: Option<CancellationToken>,
    ) -> Result<AgentLoopSourceEventStream<'a>>;
}

pub async fn run_agent_loop_with_source_runtime<'a, Runtime>(
    runtime: &'a Runtime,
    request: AgentLoopRequest<Runtime::UserMessage>,
) -> Result<AgentLoopEventStream<'a>>
where
    Runtime: AgentLoopSourceRuntime + ?Sized,
{
    let AgentLoopRequest {
        user_message,
        runtime_session_id,
        options,
        prompt_policy,
        tool_policy,
        cancellation_token,
    } = request;

    runtime.apply_prompt_policy(&prompt_policy).await?;
    runtime
        .enforce_tool_policy(&runtime_session_id, tool_policy.as_ref())
        .await?;

    let session_spec = options.session_spec(runtime_session_id);
    let source_events = runtime
        .run_source_loop(user_message, session_spec, cancellation_token)
        .await?;

    Ok(source_event_stream_to_loop_event_stream(source_events))
}

pub async fn run_agent_loop_with_lifecycle_source_runtime<'a, Runtime>(
    runtime: &'a Runtime,
    request: AgentLoopRequest<Runtime::UserMessage>,
) -> Result<AgentLoopEventStream<'a>>
where
    Runtime: AgentLoopSourceRuntime + ?Sized,
{
    let runtime_session_id = request.runtime_session_id.clone();
    let inner = run_agent_loop_with_source_runtime(runtime, request).await?;
    Ok(with_lifecycle_events(runtime_session_id, inner))
}

pub fn with_lifecycle_events<'a>(
    runtime_session_id: String,
    mut stream: AgentLoopEventStream<'a>,
) -> AgentLoopEventStream<'a> {
    let stream = async_stream::try_stream! {
        yield AgentLoopEvent::RunStarted {
            runtime_session_id: runtime_session_id.clone(),
        };

        while let Some(event) = stream.next().await {
            yield event?;
        }

        yield AgentLoopEvent::RunCompleted {
            runtime_session_id,
        };
    };

    Box::pin(stream)
}
