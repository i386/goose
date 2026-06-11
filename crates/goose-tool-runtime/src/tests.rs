use crate::*;
use goose_session_runtime::ExternalSessionId;
use std::path::{Path, PathBuf};

fn policy(available_tools: ToolAvailability, approval_mode: ToolApprovalMode) -> ToolRuntimePolicy {
    ToolRuntimePolicy {
        external_session_id: ExternalSessionId::new_v4(),
        workspace_bindings: Vec::new(),
        available_tools,
        approval_mode,
    }
}

#[test]
fn denies_unavailable_tools() {
    let policy = policy(ToolAvailability::None, ToolApprovalMode::Auto);

    let error = ensure_tool_allowed(&policy, "developer__shell").unwrap_err();

    assert_eq!(
        error,
        ToolRuntimePolicyError::Unavailable {
            tool_name: "developer__shell".to_string()
        }
    );
}

#[test]
fn denies_tools_missing_from_allowlist() {
    let policy = policy(
        ToolAvailability::Only(vec!["developer__read".to_string()]),
        ToolApprovalMode::Auto,
    );

    let error = ensure_tool_allowed(&policy, "developer__shell").unwrap_err();

    assert_eq!(
        error,
        ToolRuntimePolicyError::NotAllowed {
            tool_name: "developer__shell".to_string()
        }
    );
}

#[test]
fn ask_mode_requires_host_approval() {
    let policy = policy(ToolAvailability::All, ToolApprovalMode::Ask);

    let error = ensure_dispatch_approved(&policy, "developer__shell").unwrap_err();

    assert_eq!(
        error,
        ToolRuntimePolicyError::RequiresApproval {
            tool_name: "developer__shell".to_string()
        }
    );
}

#[test]
fn ask_mode_can_be_approved_by_host_decision() {
    let policy = policy(ToolAvailability::All, ToolApprovalMode::Ask);

    let result = authorize_tool_dispatch_with_decision(
        &policy,
        "developer__shell",
        Some(&ToolApprovalDecision::Approved),
    );

    assert!(result.is_ok());
}

#[test]
fn ask_mode_can_be_denied_by_host_decision() {
    let policy = policy(ToolAvailability::All, ToolApprovalMode::Ask);

    let error = authorize_tool_dispatch_with_decision(
        &policy,
        "developer__shell",
        Some(&ToolApprovalDecision::Denied {
            reason: Some("too risky".to_string()),
        }),
    )
    .unwrap_err();

    assert_eq!(
        error,
        ToolRuntimePolicyError::Denied {
            tool_name: "developer__shell".to_string()
        }
    );
}

#[test]
fn authorizes_allowed_auto_dispatch() {
    let policy = policy(
        ToolAvailability::Only(vec!["developer__shell".to_string()]),
        ToolApprovalMode::Auto,
    );

    let result = authorize_tool_dispatch(&policy, "developer__shell");

    assert!(result.is_ok());
}

#[test]
fn prepares_tool_dispatch_with_bound_workspace() {
    let requested = PathBuf::from("/tmp/alice");
    let fallback = PathBuf::from("/tmp/fallback");
    let mut policy = policy(ToolAvailability::All, ToolApprovalMode::Auto);
    policy.workspace_bindings.push(WorkspaceBinding {
        owner_id: "alice".to_string(),
        local_path: requested.clone(),
        mount_name: "alice".to_string(),
        writable: true,
    });
    let session = ToolRuntimeSession {
        external_session_id: policy.external_session_id,
        runtime_session_id: "goose-session-1".to_string(),
        working_dir: Some(requested.clone()),
    };

    let prepared = prepare_tool_dispatch_with_request_id(
        &policy,
        &session,
        ToolInvocation {
            id: None,
            name: "developer__shell".to_string(),
            arguments: Some(serde_json::json!({ "command": "pwd" })),
        },
        "request-1".to_string(),
        &fallback,
    )
    .unwrap();

    assert_eq!(prepared.request_id, "request-1");
    assert_eq!(prepared.name, "developer__shell");
    assert_eq!(prepared.working_dir, requested);
    assert_eq!(
        prepared.arguments.unwrap().get("command").unwrap(),
        &serde_json::json!("pwd")
    );
}

#[test]
fn prepares_ask_mode_dispatch_after_host_approval() {
    let fallback = PathBuf::from("/tmp/fallback");
    let policy = policy(ToolAvailability::All, ToolApprovalMode::Ask);
    let session = ToolRuntimeSession {
        external_session_id: policy.external_session_id,
        runtime_session_id: "goose-session-1".to_string(),
        working_dir: None,
    };

    let prepared = prepare_tool_dispatch_with_approval_decision(
        &policy,
        &session,
        ToolInvocation {
            id: None,
            name: "developer__shell".to_string(),
            arguments: Some(serde_json::json!({ "command": "pwd" })),
        },
        "request-1".to_string(),
        &fallback,
        Some(&ToolApprovalDecision::Approved),
    )
    .unwrap();

    assert_eq!(prepared.request_id, "request-1");
    assert_eq!(prepared.working_dir, fallback);
}

#[test]
fn prepare_tool_dispatch_rejects_unavailable_tool() {
    let policy = policy(ToolAvailability::None, ToolApprovalMode::Auto);
    let session = ToolRuntimeSession {
        external_session_id: policy.external_session_id,
        runtime_session_id: "goose-session-1".to_string(),
        working_dir: None,
    };

    let error = prepare_tool_dispatch(
        &policy,
        &session,
        ToolInvocation {
            id: Some("request-1".to_string()),
            name: "developer__shell".to_string(),
            arguments: None,
        },
        Path::new("/tmp/fallback"),
    )
    .unwrap_err();

    assert_eq!(
        error,
        ToolDispatchPreparationError::Policy(ToolRuntimePolicyError::Unavailable {
            tool_name: "developer__shell".to_string()
        })
    );
}

#[test]
fn prepare_tool_dispatch_rejects_non_object_arguments() {
    let policy = policy(ToolAvailability::All, ToolApprovalMode::Auto);
    let session = ToolRuntimeSession {
        external_session_id: policy.external_session_id,
        runtime_session_id: "goose-session-1".to_string(),
        working_dir: None,
    };

    let error = prepare_tool_dispatch(
        &policy,
        &session,
        ToolInvocation {
            id: Some("request-1".to_string()),
            name: "developer__shell".to_string(),
            arguments: Some(serde_json::json!("nope")),
        },
        Path::new("/tmp/fallback"),
    )
    .unwrap_err();

    assert!(matches!(error, ToolDispatchPreparationError::Arguments(_)));
}

#[test]
fn validates_active_tools_against_allowlist() {
    let policy = policy(
        ToolAvailability::Only(vec![
            "developer__read".to_string(),
            "developer__shell".to_string(),
        ]),
        ToolApprovalMode::Auto,
    );

    let result = validate_active_tools_for_policy(&policy, ["developer__read", "developer__shell"]);

    assert!(result.is_ok());
}

#[test]
fn reports_missing_and_disallowed_active_tools() {
    let policy = policy(
        ToolAvailability::Only(vec![
            "developer__read".to_string(),
            "developer__shell".to_string(),
        ]),
        ToolApprovalMode::Auto,
    );

    let violation =
        validate_active_tools_for_policy(&policy, ["developer__read", "developer__write"])
            .unwrap_err();

    assert_eq!(violation.missing_allowed, vec!["developer__shell"]);
    assert_eq!(violation.disallowed_active, vec!["developer__write"]);
    assert!(violation.to_string().contains("unavailable tools"));
    assert!(violation.to_string().contains("disallowed tools"));
}

#[test]
fn describes_visible_and_blocked_tool_inventory() {
    let policy = policy(
        ToolAvailability::Only(vec![
            "developer__read".to_string(),
            "developer__shell".to_string(),
        ]),
        ToolApprovalMode::Ask,
    );

    let inventory = describe_tool_inventory(&policy, ["developer__read", "developer__write"]);

    assert_eq!(
        inventory.tools,
        vec![
            ToolVisibility {
                name: "developer__read".to_string(),
                visible: true,
                reason: ToolVisibilityReason::Available,
                approval_mode: ToolApprovalMode::Ask,
            },
            ToolVisibility {
                name: "developer__write".to_string(),
                visible: false,
                reason: ToolVisibilityReason::NotAllowedByPolicy,
                approval_mode: ToolApprovalMode::Ask,
            },
        ]
    );
    assert_eq!(inventory.missing_allowed_tools, vec!["developer__shell"]);
}

#[test]
fn describes_none_policy_as_unavailable_inventory() {
    let policy = policy(ToolAvailability::None, ToolApprovalMode::Auto);

    let inventory = describe_tool_inventory(&policy, ["developer__read"]);

    assert_eq!(
        inventory.tools,
        vec![ToolVisibility {
            name: "developer__read".to_string(),
            visible: false,
            reason: ToolVisibilityReason::UnavailableByPolicy,
            approval_mode: ToolApprovalMode::Auto,
        }]
    );
    assert!(inventory.missing_allowed_tools.is_empty());
}

#[test]
fn accepts_missing_or_object_invocation_arguments() {
    assert!(take_invocation_argument_object(None).unwrap().is_none());

    let arguments = serde_json::json!({ "command": "ls" });
    let arguments = take_invocation_argument_object(Some(arguments)).unwrap();

    assert_eq!(
        arguments.unwrap().get("command"),
        Some(&serde_json::Value::String("ls".to_string()))
    );
}

#[test]
fn rejects_non_object_invocation_arguments() {
    let error = take_invocation_argument_object(Some(serde_json::json!("not object"))).unwrap_err();

    assert_eq!(error.message, "Tool arguments must be a JSON object.");
}

#[test]
fn selects_requested_bound_workspace() {
    let requested = PathBuf::from("/tmp/alice");
    let fallback = PathBuf::from("/tmp/fallback");
    let bindings = vec![WorkspaceBinding {
        owner_id: "alice".to_string(),
        local_path: requested.clone(),
        mount_name: "alice".to_string(),
        writable: true,
    }];

    let selected = select_working_dir(Some(&requested), &bindings, &fallback);

    assert_eq!(selected, requested);
}

#[test]
fn ignores_requested_unbound_workspace() {
    let requested = PathBuf::from("/tmp/other");
    let allowed = PathBuf::from("/tmp/alice");
    let fallback = PathBuf::from("/tmp/fallback");
    let bindings = vec![WorkspaceBinding {
        owner_id: "alice".to_string(),
        local_path: allowed.clone(),
        mount_name: "alice".to_string(),
        writable: true,
    }];

    let selected = select_working_dir(Some(&requested), &bindings, &fallback);

    assert_eq!(selected, allowed);
}
