use serde_json::{Value, json};
use sessiondb_adapters::{AdapterError, NativeSession, parse};
use sessiondb_types::{FullContentBlock, Harness, Origin, Role, ToolStatus};

fn snapshot(records: &[Value]) -> String {
    records.iter().map(|record| format!("{record}\n")).collect()
}

fn session(records: &[Value], path: &str) -> NativeSession {
    parse(Harness::Claude, path, snapshot(records).as_bytes())
        .unwrap()
        .sessions
        .remove(0)
}

fn user() -> Value {
    json!({
        "type":"user", "uuid":"user", "parentUuid":null, "sessionId":"fixture",
        "cwd":"/synthetic/project", "timestamp":"2026-01-01T00:00:00Z",
        "message":{"role":"user","content":"Explain the synthetic example."}
    })
}

#[test]
fn recorded_session_and_request_metadata_stays_out_of_the_message_sequence() {
    let events = [
        json!({"type":"session-owner","sessionId":"fixture","accountUuid":"account","organizationUuid":"organization"}),
        json!({"type":"atis-latch","sessionId":"fixture","atis":"synthetic"}),
        json!({"type":"tools-used","sessionId":"fixture","tools":["lookup"]}),
        json!({"type":"bridge-session","sessionId":"fixture","bridgeSessionId":"bridge","lastSequenceNum":7}),
        json!({"type":"file-history-delta","messageId":"native","snapshotMessageId":"snapshot","trackingPath":"example","backup":{"version":1}}),
        json!({"type":"cost-state","sessionId":"fixture","modelUsage":{"synthetic":{"inputTokens":7}},
            "totalCostUSD":0.25,"hasUnknownModelCost":false,"startTime":10,"totalAPIDuration":4,
            "totalDuration":6,"totalLinesAdded":1,"totalLinesRemoved":0,"totalToolDuration":1}),
        json!({"type":"api-request-blob","sessionId":"fixture","hash":"digest","message":{"content":[]}}),
        json!({"type":"api-request-shape","sessionId":"fixture","shapeHash":"shape","shape":{"system":[],"tools":[]}}),
        json!({"type":"api-request","sessionId":"fixture","version":"1","id":"req","timestamp":"2026-01-01T00:00:00Z",
            "querySource":"fixture","keep":0,"tail":["digest"],"params":{},"cacheControl":[],"shapeHash":"shape","requestDigest":"req-digest"}),
    ];
    let result = json!({"type":"user","uuid":"result","parentUuid":"user","sessionId":"fixture","message":{
        "role":"user","content":[{"type":"tool_result","content":[{"type":"text","text":"Found"},
        {"type":"tool_reference","tool_name":"lookup","extension":{"recorded":true}}]}]}});
    let mut records = vec![user()];
    records.extend_from_slice(&events);
    records.push(result.clone());
    let parsed = session(&records, "fixture.jsonl");
    assert_eq!(parsed.messages.len(), 2);
    assert_eq!(
        parsed.events.iter().map(|e| &e.data).collect::<Vec<_>>(),
        events.iter().collect::<Vec<_>>()
    );
    let content = serde_json::to_value(&parsed.messages[1].content).unwrap();
    assert_eq!(
        content[0]["content"][1]["value"],
        result["message"]["content"][0]["content"][1]
    );
}

#[test]
fn malformed_session_metadata_and_tool_references_reject_the_snapshot() {
    for record in [
        json!({"type":"session-owner","sessionId":"fixture","accountUuid":"account"}),
        json!({"type":"tools-used","sessionId":"fixture","tools":[5]}),
        json!({"type":"cost-state","sessionId":"fixture","totalCostUSD":"invalid"}),
        json!({"type":"api-request-blob","sessionId":"fixture","hash":"digest","message":false}),
        json!({"type":"api-request-shape","sessionId":"fixture","shapeHash":"s","shape":{"system":[],"tools":false}}),
        json!({"type":"user","uuid":"result","sessionId":"fixture","message":{"role":"user","content":[
            {"type":"tool_result","content":[{"type":"tool_reference","tool_name":7}]}]}}),
    ] {
        let input = snapshot(&[user(), record]);
        let error = parse(Harness::Claude, "fixture.jsonl", input.as_bytes()).unwrap_err();
        assert!(matches!(error, AdapterError::InvalidRecord { line: 2, .. }));
    }
}

#[test]
fn control_records_retain_complete_native_metadata_and_message_ancestry() {
    let records = [
        json!({"type":"mode","mode":"plan","sessionId":"fixture","extension":{"enabled":true}}),
        json!({"type":"permission-mode","permissionMode":"default","sessionId":"fixture"}),
        user(),
        json!({
            "type":"attachment", "uuid":"attachment-one", "parentUuid":"user",
            "isSidechain":false, "timestamp":"2026-01-01T00:00:01Z", "userType":"external",
            "entrypoint":"cli", "cwd":"/synthetic/project", "sessionId":"fixture",
            "version":"synthetic", "gitBranch":"fixture",
            "attachment":{
                "type":"deferred_tools_delta", "addedNames":["synthetic_tool"],
                "addedLines":["Synthetic tool description."], "removedNames":[],
                "readdedNames":[], "pendingMcpServers":[], "extension":{"n":1}
            }
        }),
        json!({
            "type":"attachment", "uuid":"attachment-two", "parentUuid":"attachment-one",
            "attachment":{"type":"synthetic_native_attachment","path":"/does/not/exist","command":"do not execute"}
        }),
        json!({
            "type":"assistant", "uuid":"assistant", "parentUuid":"attachment-two",
            "sessionId":"fixture", "message":{"role":"assistant","content":"Synthetic answer."}
        }),
        json!({"type":"ai-title","aiTitle":"Synthetic title","sessionId":"fixture","extension":{"n":2}}),
    ];
    let session = session(&records, "fixture.jsonl");
    assert_eq!(session.native_session_id, "fixture");
    assert_eq!(session.recorded_cwd.as_deref(), Some("/synthetic/project"));
    assert!(session.started_at.is_some());
    assert_eq!(session.messages.len(), 2);
    assert_eq!(session.messages[0].role, Role::User);
    assert_eq!(session.messages[0].origin, Origin::NativeUser);
    assert_eq!(session.messages[1].role, Role::Assistant);
    assert_eq!(session.messages[1].parent_key.as_deref(), Some("user"));
    assert!(session.artifacts.is_empty());
    for (event, index) in session.events.iter().zip([0, 1, 3, 4, 6]) {
        assert_eq!(event.event_type, records[index]["type"].as_str().unwrap());
        assert_eq!(event.data, records[index]);
        assert_eq!(event.anchor.line, index as u64 + 1);
        assert_eq!(
            event.anchor.byte_offset,
            snapshot(&records[..index]).len() as u64
        );
        assert_eq!(
            event.anchor.byte_length,
            format!("{}\n", records[index]).len() as u64
        );
    }
    assert_eq!(session.events.len(), 5);
    assert_eq!(session.events[0].anchor.record_id, "line:1");
    assert_eq!(session.events[2].anchor.record_id, "attachment-one");
    assert!(session.events[2].timestamp.is_some());
    assert_eq!(session.events[4].data["aiTitle"], "Synthetic title");
    assert_eq!(session.title(), "Synthetic title");
}

#[test]
fn control_nodes_preserve_assistant_coalescing_and_tool_links() {
    let records = [
        user(),
        json!({
            "type":"assistant", "uuid":"assistant-one", "parentUuid":"user",
            "message":{"role":"assistant","id":"response","content":[{"type":"text","text":"Synthetic preface."}]}
        }),
        json!({
            "type":"attachment", "uuid":"attachment", "parentUuid":"assistant-one",
            "attachment":{"type":"deferred_tools_delta","addedNames":[],"addedLines":[],"removedNames":[],"readdedNames":[],"pendingMcpServers":[]}
        }),
        json!({
            "type":"assistant", "uuid":"assistant-two", "parentUuid":"attachment",
            "message":{"role":"assistant","id":"response","content":[{"type":"tool_use","id":"call","name":"synthetic_tool","input":{"n":9007199254740993_u64}}]}
        }),
        json!({
            "type":"user", "uuid":"result", "parentUuid":"assistant-two",
            "message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"call","is_error":false,"content":[{"type":"image","source":{"type":"base64","media_type":"image/png","data":"AA=="},"alt":"Synthetic image."}]}]}
        }),
    ];
    let session = session(&records, "fixture.jsonl");
    assert_eq!(session.messages.len(), 3);
    let assistant = &session.messages[1];
    assert_eq!(assistant.native_ids, ["assistant-one", "assistant-two"]);
    assert_eq!(assistant.anchors.len(), 2);
    assert_eq!(assistant.content.len(), 2);
    assert_eq!(assistant.parent_key.as_deref(), Some("user"));
    let tool = &session.messages[2];
    assert_eq!(tool.role, Role::Tool);
    assert_eq!(tool.native_role, "user");
    assert_eq!(tool.parent_key.as_deref(), Some("assistant-one"));
    assert!(matches!(
        &tool.content[0],
        FullContentBlock::ToolResult { call_id: Some(id), status: ToolStatus::Success, .. } if id == "call"
    ));
    assert_eq!(session.artifacts.len(), 1);
    assert_eq!(session.artifacts[0].bytes, [0]);
    assert_eq!(session.artifacts[0].native_entry_id, "result");
}

#[test]
fn mode_header_does_not_hide_subagent_identity() {
    let records = [
        json!({"type":"mode","mode":"default","sessionId":"fixture"}),
        user(),
        json!({
            "type":"attachment", "uuid":"attachment", "parentUuid":"user", "agentId":"synthetic-agent",
            "attachment":{"type":"deferred_tools_delta"}
        }),
        json!({
            "type":"assistant", "uuid":"assistant", "parentUuid":"attachment", "agentId":"synthetic-agent",
            "message":{"role":"assistant","content":"Synthetic delegated answer."}
        }),
    ];
    let session = session(&records, "subagents/agent-synthetic-agent.jsonl");
    assert_eq!(session.native_key, "fixture/agent:synthetic-agent");
    assert_eq!(session.native_agent_id.as_deref(), Some("synthetic-agent"));
    assert_eq!(session.relations.len(), 1);
    assert_eq!(session.relations[0].relation_type, "subagent");
    assert_eq!(session.relations[0].native_reference, "fixture");
    assert_eq!(session.messages[0].origin, Origin::Harness);
    assert_eq!(session.messages[1].parent_key.as_deref(), Some("user"));
}

#[test]
fn malformed_control_records_and_unknown_outer_kinds_reject_the_whole_snapshot() {
    for bad in [
        json!({"type":"mode","sessionId":"fixture"}),
        json!({"type":"mode","mode":null,"sessionId":"fixture"}),
        json!({"type":"mode","mode":"default"}),
        json!({"type":"permission-mode","permissionMode":false,"sessionId":"fixture"}),
        json!({"type":"permission-mode","sessionId":"fixture"}),
        json!({"type":"permission-mode","permissionMode":"default","sessionId":7}),
        json!({"type":"ai-title","sessionId":"fixture"}),
        json!({"type":"ai-title","aiTitle":[],"sessionId":"fixture"}),
        json!({"type":"ai-title","aiTitle":"Synthetic title","sessionId":"other"}),
        json!({"type":"custom-title"}),
        json!({"type":"custom-title","customTitle":false}),
        json!({"type":"attachment","attachment":{"type":"deferred_tools_delta"}}),
        json!({"type":"attachment","uuid":"bad"}),
        json!({"type":"attachment","uuid":"bad","attachment":null}),
        json!({"type":"attachment","uuid":"bad","attachment":[]}),
        json!({"type":"attachment","uuid":"bad","attachment":{}}),
        json!({"type":"attachment","uuid":"bad","attachment":{"type":7}}),
        json!({"type":"attachment","uuid":"bad","attachment":{"type":""}}),
        json!({"type":"attachment","uuid":"bad","parentUuid":7,"attachment":{"type":"deferred_tools_delta"}}),
        json!({"type":"attachment","uuid":"bad","isSidechain":"false","attachment":{"type":"deferred_tools_delta"}}),
        json!({"type":"attachment","uuid":"bad","sessionId":"other","attachment":{"type":"deferred_tools_delta"}}),
        json!({"type":"synthetic_unknown","uuid":"bad","payload":{}}),
    ] {
        let input = snapshot(&[user(), bad]);
        let error = parse(Harness::Claude, "fixture.jsonl", input.as_bytes()).unwrap_err();
        assert!(matches!(error, AdapterError::InvalidRecord { line: 2, .. }));
    }
}

#[test]
fn custom_title_metadata_remains_structural_and_overrides_ai_title() {
    let records = [
        user(),
        json!({"type":"custom-title","customTitle":"Chosen synthetic title","extension":{"n":1}}),
        json!({"type":"ai-title","aiTitle":"Suggested synthetic title","sessionId":"fixture"}),
    ];
    let session = session(&records, "fixture.jsonl");
    assert_eq!(session.events[0].event_type, "custom-title");
    assert_eq!(session.events[0].data, records[1]);
    assert_eq!(session.events[1].data, records[2]);
    assert_eq!(session.title(), "Chosen synthetic title");
}
