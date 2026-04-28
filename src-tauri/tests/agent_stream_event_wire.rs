//! Wire-format pin for `AgentStreamEvent`.
//!
//! This is the contract the frontend (`use-streaming.ts`) listens to over
//! the Tauri Channel. Every variant of `AgentStreamEvent` is enumerated
//! here and snapshotted in its JSON-serialized form. The state-machine
//! refactor in `agents::streaming` must not change this surface — any
//! drift here is a frontend-breaking change.
//!
//! When intentionally evolving the wire format, the snapshot diff goes
//! through `cargo insta review` and the matching frontend type in
//! `src/lib/api.ts` must be updated in the same PR.

use winthorpe_lib::agents::AgentStreamEvent;
use winthorpe_lib::pipeline::types::ThreadMessageLike;
use insta::assert_yaml_snapshot;
use serde_json::{json, Value};

fn to_value(event: AgentStreamEvent) -> Value {
    serde_json::to_value(event).expect("AgentStreamEvent serializes")
}

fn empty_message() -> ThreadMessageLike {
    serde_json::from_value(json!({
        "id": "msg-1",
        "role": "assistant",
        "content": []
    }))
    .expect("trivial ThreadMessageLike parses")
}

#[test]
fn wire_format_update() {
    assert_yaml_snapshot!(to_value(AgentStreamEvent::Update {
        messages: vec![empty_message()],
    }));
}

#[test]
fn wire_format_streaming_partial() {
    assert_yaml_snapshot!(to_value(AgentStreamEvent::StreamingPartial {
        message: empty_message(),
    }));
}

#[test]
fn wire_format_done() {
    assert_yaml_snapshot!(to_value(AgentStreamEvent::Done {
        provider: "claude".into(),
        model_id: "opus-1m".into(),
        resolved_model: "claude-opus-4-20250514".into(),
        session_id: Some("provider-session-1".into()),
        working_directory: "/tmp/winthorpe".into(),
        persisted: true,
    }));
}

#[test]
fn wire_format_aborted() {
    assert_yaml_snapshot!(to_value(AgentStreamEvent::Aborted {
        provider: "claude".into(),
        model_id: "opus-1m".into(),
        resolved_model: "claude-opus-4-20250514".into(),
        session_id: Some("provider-session-1".into()),
        working_directory: "/tmp/winthorpe".into(),
        persisted: true,
        reason: "user_requested".into(),
    }));
}

#[test]
fn wire_format_permission_request() {
    assert_yaml_snapshot!(to_value(AgentStreamEvent::PermissionRequest {
        permission_id: "permission-1".into(),
        tool_name: "Bash".into(),
        tool_input: json!({ "command": "ls" }),
        title: Some("Run shell command".into()),
        description: Some("Lists the directory".into()),
    }));
}

#[test]
fn wire_format_deferred_tool_use() {
    assert_yaml_snapshot!(to_value(AgentStreamEvent::DeferredToolUse {
        provider: "claude".into(),
        model_id: "opus-1m".into(),
        resolved_model: "claude-opus-4-20250514".into(),
        session_id: Some("provider-session-1".into()),
        working_directory: "/tmp/winthorpe".into(),
        permission_mode: Some("default".into()),
        tool_use_id: "tool-1".into(),
        tool_name: "AskUserQuestion".into(),
        tool_input: json!({ "question": "Pick one" }),
    }));
}

#[test]
fn wire_format_elicitation_request() {
    assert_yaml_snapshot!(to_value(AgentStreamEvent::ElicitationRequest {
        provider: "claude".into(),
        model_id: "opus-1m".into(),
        resolved_model: "claude-opus-4-20250514".into(),
        session_id: Some("provider-session-1".into()),
        working_directory: "/tmp/winthorpe".into(),
        elicitation_id: Some("elicitation-1".into()),
        server_name: "design-server".into(),
        message: "Need structured input".into(),
        mode: Some("form".into()),
        url: None,
        requested_schema: Some(json!({
            "type": "object",
            "properties": { "name": { "type": "string" } },
            "required": ["name"]
        })),
    }));
}

#[test]
fn wire_format_plan_captured() {
    assert_yaml_snapshot!(to_value(AgentStreamEvent::PlanCaptured {}));
}

#[test]
fn wire_format_error() {
    assert_yaml_snapshot!(to_value(AgentStreamEvent::Error {
        message: "Sidecar crashed".into(),
        persisted: false,
        internal: true,
    }));
}
