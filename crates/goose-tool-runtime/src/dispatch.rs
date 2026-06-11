use std::path::{Path, PathBuf};

use crate::{
    authorize_tool_dispatch_with_decision, PreparedToolInvocation, ToolApprovalDecision,
    ToolDispatchPreparationError, ToolInvocation, ToolRuntimePolicy, ToolRuntimeSession,
    WorkspaceBinding,
};

pub fn prepare_tool_dispatch(
    policy: &ToolRuntimePolicy,
    session: &ToolRuntimeSession,
    invocation: ToolInvocation,
    fallback_working_dir: &Path,
) -> Result<PreparedToolInvocation, ToolDispatchPreparationError> {
    let request_id = invocation.request_id();
    prepare_tool_dispatch_with_request_id(
        policy,
        session,
        invocation,
        request_id,
        fallback_working_dir,
    )
}

pub fn prepare_tool_dispatch_with_request_id(
    policy: &ToolRuntimePolicy,
    session: &ToolRuntimeSession,
    invocation: ToolInvocation,
    request_id: String,
    fallback_working_dir: &Path,
) -> Result<PreparedToolInvocation, ToolDispatchPreparationError> {
    prepare_tool_dispatch_with_approval_decision(
        policy,
        session,
        invocation,
        request_id,
        fallback_working_dir,
        None,
    )
}

pub fn prepare_tool_dispatch_with_approval_decision(
    policy: &ToolRuntimePolicy,
    session: &ToolRuntimeSession,
    mut invocation: ToolInvocation,
    request_id: String,
    fallback_working_dir: &Path,
    decision: Option<&ToolApprovalDecision>,
) -> Result<PreparedToolInvocation, ToolDispatchPreparationError> {
    authorize_tool_dispatch_with_decision(policy, &invocation.name, decision)?;
    let arguments = invocation.take_argument_object()?;
    let working_dir = select_working_dir(
        session.working_dir.as_deref(),
        &policy.workspace_bindings,
        fallback_working_dir,
    );

    Ok(PreparedToolInvocation {
        request_id,
        name: invocation.name,
        arguments,
        working_dir,
    })
}

pub fn select_working_dir(
    requested: Option<&Path>,
    bindings: &[WorkspaceBinding],
    fallback: &Path,
) -> PathBuf {
    if let Some(requested) = requested {
        if bindings.is_empty()
            || bindings
                .iter()
                .any(|binding| binding.local_path == requested)
        {
            return requested.to_path_buf();
        }
    }

    bindings
        .iter()
        .find(|binding| binding.writable)
        .or_else(|| bindings.first())
        .map(|binding| binding.local_path.clone())
        .unwrap_or_else(|| fallback.to_path_buf())
}
