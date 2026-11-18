use serde_json::{Value, json};
use sessiondb_adapters::{AdapterError, complete_prefix, parse};
use sessiondb_types::{FullContentBlock, Harness, Origin, Role, ToolStatus};

const CODEX: &str = concat!(
    "{\"type\":\"session_meta\",\"payload\":{\"id\":\"fixture\"}}\n",
    "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"Synthetic fixture.\"}]}}\n"
);
const CLAUDE: &str = "{\"type\":\"user\",\"uuid\":\"user\",\"sessionId\":\"fixture\",\"message\":{\"role\":\"user\",\"content\":\"Synthetic fixture.\"}}\n";
const PI: &str = concat!(
    "{\"type\":\"session\",\"id\":\"fixture\",\"version\":3}\n",
    "{\"type\":\"message\",\"id\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"Synthetic fixture.\"}}\n"
);

fn snapshot(harness: Harness) -> &'static str {
    match harness {
        Harness::Codex => CODEX,
        Harness::Claude => CLAUDE,
        Harness::Pi => PI,
    }
}

fn reject(harness: Harness, tail: &str) {
    let input = format!("{}{tail}\n", snapshot(harness));
    let error = parse(harness, "fixture.jsonl", input.as_bytes())
        .expect_err("an invalid record after valid messages must reject the whole snapshot");
    assert!(matches!(error, AdapterError::InvalidRecord { .. }));
    assert!(!error.to_string().contains("private-fixture-marker"));
}

#[test]
fn malformed_and_unrecognized_records_never_return_a_prefix() {
    for harness in [Harness::Codex, Harness::Claude, Harness::Pi] {
        for tail in [
            "{private-fixture-marker",
            "[]",
            "null",
            "\"private-fixture-marker\"",
            "{}",
            "{\"type\":7}",
            "{\"type\":\"private-fixture-marker\",\"id\":\"future\",\"payload\":{}}",
        ] {
            reject(harness, tail);
        }
        let unfinished = format!("{}{{\"type\":", snapshot(harness));
        assert!(parse(harness, "fixture.jsonl", unfinished.as_bytes()).is_err());
    }
}

#[test]
fn invalid_native_content_is_rejected_in_each_harness() {
    let cases = [
        (
            Harness::Codex,
            json!({"type":"response_item","payload":{"type":"private-fixture-marker"}}),
        ),
        (
            Harness::Codex,
            json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"private-fixture-marker"}]}}),
        ),
        (
            Harness::Codex,
            json!({"type":"response_item","payload":{"type":"message","role":"private-fixture-marker","content":[]}}),
        ),
        (
            Harness::Codex,
            json!({"type":"response_item","payload":{"type":"message","role":"unknown","content":[]}}),
        ),
        (
            Harness::Codex,
            json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":"not an array"}}),
        ),
        (
            Harness::Codex,
            json!({"type":"response_item","payload":null}),
        ),
        (
            Harness::Codex,
            json!({"type":"response_item","payload":{"type":"function_call","name":"fixture","arguments":"{private-fixture-marker"}}),
        ),
        (
            Harness::Codex,
            json!({"type":"response_item","payload":{"type":"function_call","arguments":"{}"}}),
        ),
        (
            Harness::Codex,
            json!({"type":"response_item","payload":{"type":"function_call_output","output":[{"type":"private-fixture-marker"}]}}),
        ),
        (
            Harness::Codex,
            json!({"type":"response_item","payload":{"type":"function_call_output","output":{"type":"private-fixture-marker"}}}),
        ),
        (
            Harness::Codex,
            json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":7}]}}),
        ),
        (
            Harness::Claude,
            json!({"type":"assistant","uuid":"bad","message":{"role":"assistant","content":[{"type":"private-fixture-marker"}]}}),
        ),
        (
            Harness::Claude,
            json!({"type":"assistant","uuid":"bad","message":{"role":"assistant","content":null}}),
        ),
        (
            Harness::Claude,
            json!({"type":"assistant","uuid":"bad","message":{"role":"assistant"}}),
        ),
        (
            Harness::Claude,
            json!({"type":"assistant","uuid":"bad","message":{"role":"private-fixture-marker","content":[]}}),
        ),
        (
            Harness::Claude,
            json!({"type":"user","uuid":"bad","message":{"role":"user","content":[{"type":"tool_result","content":[{"type":"private-fixture-marker"}]}]}}),
        ),
        (
            Harness::Claude,
            json!({"type":"user","uuid":"bad","message":{"role":"user","content":[{"type":"tool_result","content":"fixture","is_error":"false"}]}}),
        ),
        (
            Harness::Claude,
            json!({"type":"user","uuid":"bad","message":{"role":"user","content":[{"type":"image","source":{"type":"url","url":"private-fixture-marker"}}]}}),
        ),
        (
            Harness::Claude,
            json!({"type":"assistant","uuid":"bad","message":{"role":"assistant","content":[{"type":"tool_use","name":"fixture"}]}}),
        ),
        (
            Harness::Claude,
            json!({"type":"assistant","uuid":"bad","message":{"role":"assistant","content":[{"type":"tool_use","name":"fixture","input":"{private-fixture-marker"}]}}),
        ),
        (
            Harness::Pi,
            json!({"type":"message","id":"bad","message":{"role":"assistant","content":[{"type":"private-fixture-marker"}]}}),
        ),
        (
            Harness::Pi,
            json!({"type":"message","id":"bad","message":{"role":"private-fixture-marker","content":[]}}),
        ),
        (
            Harness::Pi,
            json!({"type":"message","id":"bad","message":{"role":"assistant","content":{}}}),
        ),
        (
            Harness::Pi,
            json!({"type":"message","id":"bad","message":{"role":"toolResult","content":[{"type":"private-fixture-marker"}]}}),
        ),
        (
            Harness::Pi,
            json!({"type":"message","id":"bad","message":{"role":"toolResult","content":{"type":"private-fixture-marker"}}}),
        ),
        (
            Harness::Pi,
            json!({"type":"message","id":"bad","message":{"role":"toolResult","content":[],"isError":7}}),
        ),
        (
            Harness::Pi,
            json!({"type":"message","id":"bad","message":{"role":"user","content":[{"type":"image","mimeType":"image/png","data":"private-fixture-marker"}]}}),
        ),
        (
            Harness::Pi,
            json!({"type":"message","id":"bad","message":{"role":"assistant","content":[{"type":"toolCall","name":"fixture"}]}}),
        ),
        (
            Harness::Pi,
            json!({"type":"message","id":"bad","message":{"role":"assistant","content":[{"type":"toolCall","name":"fixture","arguments":"{private-fixture-marker"}]}}),
        ),
    ];
    for (harness, record) in cases {
        reject(harness, &record.to_string());
    }
}

#[test]
fn inherited_history_cutoffs_must_be_complete_unsigned_integers() {
    for base in [
        Value::Null,
        json!([]),
        json!({}),
        json!({"thread_id":"parent","end_ordinal_exclusive":-1,"end_byte_offset":0}),
        json!({"thread_id":"parent","end_ordinal_exclusive":1.5,"end_byte_offset":0}),
        json!({"thread_id":"parent","end_ordinal_exclusive":1,"end_byte_offset":"private-fixture-marker"}),
    ] {
        let input = json!({"type":"session_meta","payload":{"id":"fixture","history_base":base}})
            .to_string();
        let error = parse(Harness::Codex, "fixture.jsonl", input.as_bytes()).unwrap_err();
        assert!(!error.to_string().contains("private-fixture-marker"));
    }
}

#[test]
fn a_complete_final_object_does_not_require_a_newline() {
    for harness in [Harness::Codex, Harness::Claude, Harness::Pi] {
        let input = snapshot(harness).trim_end();
        let parsed = parse(harness, "fixture.jsonl", input.as_bytes()).unwrap();
        assert_eq!(parsed.sessions[0].messages.len(), 1);
    }
}

#[test]
fn capture_helper_is_separate_from_strict_parsing() {
    let input = format!("{CODEX}{{\"type\":");
    assert_eq!(complete_prefix(input.as_bytes()), CODEX.as_bytes());
    assert!(parse(Harness::Codex, "fixture.jsonl", input.as_bytes()).is_err());
    assert!(
        parse(
            Harness::Codex,
            "fixture.jsonl",
            complete_prefix(input.as_bytes())
        )
        .is_ok()
    );
    assert!(complete_prefix(b"{}").is_empty());
}

#[test]
fn developer_instructions_keep_the_native_role() {
    let input = format!(
        "{CODEX}{}",
        json!({"type":"response_item","payload":{"type":"message","role":"developer","content":[{"type":"input_text","text":"Synthetic instructions."}]}})
    );
    let parsed = parse(Harness::Codex, "fixture.jsonl", input.as_bytes()).unwrap();
    let message = &parsed.sessions[0].messages[1];
    assert_eq!(message.role, Role::System);
    assert_eq!(message.origin, Origin::Harness);
    assert_eq!(message.native_role, "developer");
}

#[test]
fn supported_event_envelopes_retain_variable_payloads() {
    let cases = [
        (
            Harness::Codex,
            json!({"type":"event_msg","payload":{"type":"agent_message","message":"Synthetic UI copy.","extension":{"n":1}}}),
            "agent_message",
        ),
        (
            Harness::Claude,
            json!({"type":"progress","uuid":"event","data":{"type":"fixture-progress","n":1}}),
            "progress",
        ),
        (
            Harness::Pi,
            json!({"type":"custom","id":"event","parentId":"user","customType":"fixture-state","data":{"n":1}}),
            "custom",
        ),
    ];
    for (harness, record, event_type) in cases {
        let input = format!("{}{}\n", snapshot(harness), record);
        let parsed = parse(harness, "fixture.jsonl", input.as_bytes()).unwrap();
        assert_eq!(parsed.sessions[0].messages.len(), 1);
        assert_eq!(parsed.sessions[0].events[0].event_type, event_type);
        assert_eq!(parsed.sessions[0].events[0].data, record);
    }
}

#[test]
fn missing_call_history_is_not_an_unsupported_format() {
    let cases = [
        (
            Harness::Claude,
            json!({"type":"user","uuid":"result","parentUuid":"absent","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"absent","content":"Synthetic result."}]}}),
        ),
        (
            Harness::Pi,
            json!({"type":"message","id":"result","parentId":"absent","message":{"role":"toolResult","toolCallId":"absent","content":[{"type":"text","text":"Synthetic result."}]}}),
        ),
    ];
    for (harness, record) in cases {
        let input = format!("{}{}\n", snapshot(harness), record);
        let parsed = parse(harness, "fixture.jsonl", input.as_bytes()).unwrap();
        let message = &parsed.sessions[0].messages[1];
        assert!(message.parent_key.is_none());
        assert!(matches!(
            message.content[0],
            FullContentBlock::ToolResult {
                call_id: None,
                status: ToolStatus::Unknown,
                ..
            }
        ));
    }
}

#[test]
fn supported_claude_artifacts_keep_their_native_metadata() {
    let record = json!({
        "type":"user", "uuid":"image", "message":{
            "role":"user", "content":[{
                "type":"tool_result", "tool_use_id":"absent", "content":[{
                    "type":"image",
                    "source":{"type":"base64","media_type":"image/png","data":"AA=="},
                    "alt":"Synthetic artifact."
                }]
            }]
        }
    });
    let input = format!("{CLAUDE}{record}\n");
    let parsed = parse(Harness::Claude, "fixture.jsonl", input.as_bytes()).unwrap();
    let session = &parsed.sessions[0];
    let artifact = &session.artifacts[0];
    assert_eq!(artifact.media_type, "image/png");
    assert_eq!(artifact.bytes, [0]);
    assert_eq!(artifact.native_entry_id, "image");
    assert_eq!(artifact.content_index, 0);
    let content = serde_json::to_value(&session.messages[1].content).unwrap();
    assert_eq!(
        content[0]["content"][0]["artifact_id"],
        artifact.id.to_string()
    );
    assert_eq!(content[0]["content"][0]["alt"], "Synthetic artifact.");
}

#[test]
fn encrypted_reasoning_is_redacted_and_retained_as_an_event() {
    let record = json!({
        "type":"response_item",
        "payload":{"type":"reasoning","summary":[{"type":"summary_text","text":"Synthetic summary."}],"encrypted_content":"opaque-fixture"}
    });
    let input = format!("{CODEX}{record}\n");
    let parsed = parse(Harness::Codex, "fixture.jsonl", input.as_bytes()).unwrap();
    let session = &parsed.sessions[0];
    assert!(
        matches!(&session.messages[1].content[1], FullContentBlock::Reasoning { text, redacted: true } if text.is_empty())
    );
    assert_eq!(session.events[0].event_type, "encrypted_reasoning");
    assert_eq!(session.events[0].data, record);
}

#[test]
fn duplicate_native_call_ids_do_not_create_a_guessed_result_link() {
    let call = json!({"type":"response_item","payload":{"type":"function_call","name":"fixture","call_id":"duplicate","arguments":"{}"}});
    let result = json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"duplicate","output":"Synthetic result."}});
    let input = format!("{CODEX}{call}\n{call}\n{result}\n");
    let parsed = parse(Harness::Codex, "fixture.jsonl", input.as_bytes()).unwrap();
    assert!(matches!(
        parsed.sessions[0].messages[3].content[0],
        FullContentBlock::ToolResult { call_id: None, .. }
    ));
}
