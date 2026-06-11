use crate::*;
use goose_providers::conversation::message::Message;
use rmcp::model::CallToolRequestParams;
use std::path::PathBuf;
use uuid::Uuid;

#[test]
fn builds_text_message_from_legacy_history_entry() {
    let entry = RuntimeHistoryEntry {
        id: Uuid::new_v4(),
        external_session_id: Uuid::new_v4(),
        external_task_id: None,
        source_message_id: Some("external-msg-1".to_string()),
        runtime: "test".to_string(),
        role: "user".to_string(),
        content: "hello".to_string(),
        message_json: None,
    };

    let message = message_from_runtime_history(&entry).unwrap();

    assert_eq!(message.id.as_deref(), Some("external-msg-1"));
    assert_eq!(message.as_concat_text(), "hello");
}

#[test]
fn builds_messages_from_runtime_history_batch() {
    let external_session_id = Uuid::new_v4();
    let entries = vec![
        RuntimeHistoryEntry {
            id: Uuid::new_v4(),
            external_session_id,
            external_task_id: None,
            source_message_id: Some("msg-1".to_string()),
            runtime: "test".to_string(),
            role: "user".to_string(),
            content: "hello".to_string(),
            message_json: None,
        },
        RuntimeHistoryEntry {
            id: Uuid::new_v4(),
            external_session_id,
            external_task_id: None,
            source_message_id: Some("msg-2".to_string()),
            runtime: "test".to_string(),
            role: "assistant".to_string(),
            content: "hi".to_string(),
            message_json: None,
        },
    ];

    let messages = messages_from_runtime_history(&entries).unwrap();

    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].id.as_deref(), Some("msg-1"));
    assert_eq!(messages[0].as_concat_text(), "hello");
    assert_eq!(messages[1].id.as_deref(), Some("msg-2"));
    assert_eq!(messages[1].as_concat_text(), "hi");
}

#[test]
fn preserves_structured_tool_request_history() {
    let external_session_id = Uuid::new_v4();
    let message = Message::assistant()
        .with_id("assistant-msg-1")
        .with_tool_request("tool-1", Ok(CallToolRequestParams::new("developer__shell")));

    let entry = runtime_history_from_message(external_session_id, &message).unwrap();
    let restored = message_from_runtime_history(&entry).unwrap();

    assert_eq!(entry.external_session_id, external_session_id);
    assert_eq!(entry.source_message_id.as_deref(), Some("assistant-msg-1"));
    assert_eq!(entry.role, "assistant");
    assert!(entry.content.is_empty());
    assert!(entry.message_json.is_some());
    assert_eq!(restored.id.as_deref(), Some("assistant-msg-1"));
    assert_eq!(restored.content.len(), 1);
}

#[test]
fn runtime_session_record_converts_local_session_link() {
    let external_session_id = Uuid::new_v4();
    let link = SessionLink {
        external_session_id,
        goose_session_id: "goose-session-1".to_string(),
        working_dir: PathBuf::from("/tmp/workspace"),
    };

    let record = RuntimeSessionRecord::from_session_link(link.clone()).with_title("Demo");
    let restored = record.try_into_session_link().unwrap();

    assert_eq!(record.external_session_id, external_session_id);
    assert_eq!(record.runtime_session_id, "goose-session-1");
    assert_eq!(record.title.as_deref(), Some("Demo"));
    assert_eq!(restored.external_session_id, link.external_session_id);
    assert_eq!(restored.goose_session_id, link.goose_session_id);
    assert_eq!(restored.working_dir, link.working_dir);
}

#[test]
fn remote_runtime_workspace_requires_host_binding_before_goose_link() {
    let record = RuntimeSessionRecord::new(
        Uuid::new_v4(),
        "goose-session-1",
        RuntimeWorkspaceRef::remote_uri("s3://tenant/session"),
    );

    let error = record.try_into_session_link().unwrap_err();

    assert!(error.to_string().contains("no local path binding"));
}

#[test]
fn remote_runtime_workspace_can_be_bound_to_local_goose_workspace() {
    let external_session_id = Uuid::new_v4();
    let record = RuntimeSessionRecord::new(
        external_session_id,
        "goose-session-1",
        RuntimeWorkspaceRef::remote_uri("s3://tenant/session")
            .with_owner("alice")
            .with_mount_name("alice-workspace"),
    )
    .bind_local_workspace("/tmp/alice-workspace");

    assert!(record.workspace.is_remote());
    assert!(record.workspace.is_local_bound());
    assert_eq!(record.workspace.owner_id.as_deref(), Some("alice"));
    assert_eq!(
        record.workspace.remote_uri.as_deref(),
        Some("s3://tenant/session")
    );

    let link = record.try_into_session_link().unwrap();

    assert_eq!(link.external_session_id, external_session_id);
    assert_eq!(link.goose_session_id, "goose-session-1");
    assert_eq!(link.working_dir, PathBuf::from("/tmp/alice-workspace"));
}
