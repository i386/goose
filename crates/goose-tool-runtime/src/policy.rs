use std::collections::HashSet;

use crate::{
    ToolApprovalDecision, ToolApprovalMode, ToolAvailability, ToolInventoryVisibility,
    ToolRegistryPolicyViolation, ToolRuntimePolicy, ToolRuntimePolicyError, ToolVisibility,
    ToolVisibilityReason,
};

pub fn is_tool_available(policy: &ToolRuntimePolicy, tool_name: &str) -> bool {
    match &policy.available_tools {
        ToolAvailability::All => true,
        ToolAvailability::None => false,
        ToolAvailability::Only(allowed) => allowed.iter().any(|name| name == tool_name),
    }
}

pub fn ensure_tool_allowed(
    policy: &ToolRuntimePolicy,
    tool_name: &str,
) -> Result<(), ToolRuntimePolicyError> {
    match &policy.available_tools {
        ToolAvailability::All => Ok(()),
        ToolAvailability::None => Err(ToolRuntimePolicyError::Unavailable {
            tool_name: tool_name.to_string(),
        }),
        ToolAvailability::Only(allowed) if allowed.iter().any(|name| name == tool_name) => Ok(()),
        ToolAvailability::Only(_) => Err(ToolRuntimePolicyError::NotAllowed {
            tool_name: tool_name.to_string(),
        }),
    }
}

pub fn tool_visibility_for_policy(policy: &ToolRuntimePolicy, tool_name: &str) -> ToolVisibility {
    let (visible, reason) = match &policy.available_tools {
        ToolAvailability::All => (true, ToolVisibilityReason::Available),
        ToolAvailability::None => (false, ToolVisibilityReason::UnavailableByPolicy),
        ToolAvailability::Only(allowed) if allowed.iter().any(|name| name == tool_name) => {
            (true, ToolVisibilityReason::Available)
        }
        ToolAvailability::Only(_) => (false, ToolVisibilityReason::NotAllowedByPolicy),
    };

    ToolVisibility {
        name: tool_name.to_string(),
        visible,
        reason,
        approval_mode: policy.approval_mode.clone(),
    }
}

pub fn describe_tool_inventory<I, S>(
    policy: &ToolRuntimePolicy,
    active_tools: I,
) -> ToolInventoryVisibility
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let active_tool_names = active_tools
        .into_iter()
        .map(|tool| tool.as_ref().to_string())
        .collect::<Vec<_>>();
    let active_tool_set = active_tool_names.iter().cloned().collect::<HashSet<_>>();

    let tools = active_tool_names
        .iter()
        .map(|tool_name| tool_visibility_for_policy(policy, tool_name))
        .collect();

    let mut missing_allowed_tools = match &policy.available_tools {
        ToolAvailability::Only(allowed_tools) => allowed_tools
            .iter()
            .filter(|tool_name| !active_tool_set.contains(*tool_name))
            .cloned()
            .collect::<Vec<_>>(),
        ToolAvailability::All | ToolAvailability::None => Vec::new(),
    };
    missing_allowed_tools.sort();

    ToolInventoryVisibility {
        tools,
        missing_allowed_tools,
    }
}

pub fn validate_active_tools_for_policy<I, S>(
    policy: &ToolRuntimePolicy,
    active_tools: I,
) -> Result<(), ToolRegistryPolicyViolation>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let ToolAvailability::Only(allowed_tools) = &policy.available_tools else {
        return Ok(());
    };

    let active_tool_names = active_tools
        .into_iter()
        .map(|tool| tool.as_ref().to_string())
        .collect::<HashSet<_>>();
    let allowed_tool_names = allowed_tools.iter().cloned().collect::<HashSet<_>>();

    let mut missing_allowed = allowed_tool_names
        .difference(&active_tool_names)
        .cloned()
        .collect::<Vec<_>>();
    missing_allowed.sort();

    let mut disallowed_active = active_tool_names
        .difference(&allowed_tool_names)
        .cloned()
        .collect::<Vec<_>>();
    disallowed_active.sort();

    let violation = ToolRegistryPolicyViolation {
        missing_allowed,
        disallowed_active,
    };

    if violation.is_empty() {
        Ok(())
    } else {
        Err(violation)
    }
}

pub fn ensure_dispatch_approved(
    policy: &ToolRuntimePolicy,
    tool_name: &str,
) -> Result<(), ToolRuntimePolicyError> {
    ensure_dispatch_approved_with_decision(policy, tool_name, None)
}

pub fn ensure_dispatch_approved_with_decision(
    policy: &ToolRuntimePolicy,
    tool_name: &str,
    decision: Option<&ToolApprovalDecision>,
) -> Result<(), ToolRuntimePolicyError> {
    match policy.approval_mode {
        ToolApprovalMode::Auto => Ok(()),
        ToolApprovalMode::Ask => match decision {
            Some(ToolApprovalDecision::Approved) => Ok(()),
            Some(ToolApprovalDecision::Denied { .. }) => Err(ToolRuntimePolicyError::Denied {
                tool_name: tool_name.to_string(),
            }),
            None => Err(ToolRuntimePolicyError::RequiresApproval {
                tool_name: tool_name.to_string(),
            }),
        },
        ToolApprovalMode::Deny => Err(ToolRuntimePolicyError::Denied {
            tool_name: tool_name.to_string(),
        }),
    }
}

pub fn authorize_tool_dispatch(
    policy: &ToolRuntimePolicy,
    tool_name: &str,
) -> Result<(), ToolRuntimePolicyError> {
    authorize_tool_dispatch_with_decision(policy, tool_name, None)
}

pub fn authorize_tool_dispatch_with_decision(
    policy: &ToolRuntimePolicy,
    tool_name: &str,
    decision: Option<&ToolApprovalDecision>,
) -> Result<(), ToolRuntimePolicyError> {
    ensure_tool_allowed(policy, tool_name)?;
    ensure_dispatch_approved_with_decision(policy, tool_name, decision)
}
