use serde_json::{Value, json};
use sessiondb_adapters::{AdapterError, NativeSession, parse};
use sessiondb_types::{FullContentBlock, Harness, Origin, Role, ToolInput};

fn header() -> Value {
    json!({"type":"session_meta","payload":{"id":"synthetic-codex","cwd":"/fixture"}})
}

fn user(id: &str, texts: &[&str]) -> Value {
    json!({"type":"response_item","payload":{
        "type":"message","id":id,"role":"user",
        "content":texts.iter().map(|text| json!({"type":"input_text","text":text})).collect::<Vec<_>>()
    }})
}

fn bytes(records: &[Value]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for record in records {
        serde_json::to_writer(&mut bytes, record).unwrap();
        bytes.push(b'\n');
    }
    bytes
}

fn session(records: &[Value]) -> NativeSession {
    parse(Harness::Codex, "synthetic.jsonl", &bytes(records))
        .unwrap()
        .sessions
        .remove(0)
}

fn web_search() -> Value {
    json!({"timestamp":"2026-01-01T00:00:00Z","type":"response_item","payload":{
        "type":"web_search_call","status":"completed",
        "action":{"type":"search","query":"synthetic query","queries":["synthetic query","another query"]}
    }})
}

fn additional_tools() -> Value {
    json!({"type":"response_item","ordinal":2,"payload":{
        "type":"additional_tools","id":"definitions","role":"developer","tools":[{
            "type":"namespace","name":"fixture","description":"Synthetic tools","tools":[
                {"type":"function","name":"lookup","description":"Look up a fixture",
                 "parameters":{"type":"object","properties":{"key":{"type":"integer"}}},"strict":false},
                {"type":"custom","name":"patch","description":"Patch a fixture",
                 "format":{"type":"grammar","syntax":"lark","definition":"start: /fixture/"}}
            ]
        }]
    },"metadata":{"client_authored":true,"mcp_attribution":{"status":"unknown"}}})
}

fn world_state() -> Value {
    json!({"type":"world_state","ordinal":3,"payload":{"full":true,"state":{
        "agents_md":{"directory":"/fixture","text":"Synthetic instructions"},
        "environments":{"current_date":"2026-01-01","environments":{"local":{"cwd":"/fixture"}}},
        "context_window_guidance":"Synthetic guidance"
    }}})
}

fn usage() -> Value {
    let counters = json!({"input_tokens":12,"cached_input_tokens":3,"cache_write_input_tokens":0,
        "output_tokens":5,"reasoning_output_tokens":2,"total_tokens":17});
    json!({"type":"token_usage_record","ordinal":5,"payload":{
        "thread_id":"thread","turn_id":"turn","session_id":"synthetic-codex",
        "root_turn_id":"root","response_id":"response",
        "usage":counters,"turn_token_usage":counters,"thread_token_usage":counters
    }})
}

#[test]
fn known_native_records_preserve_raw_events_precision_anchors_and_message_chain() {
    let huge: Value = serde_json::from_str("184467440737095516171234567890123456789").unwrap();
    let mut tools = additional_tools();
    tools["payload"]["tools"][0]["tools"][0]["parameters"]["maximum"] = huge.clone();
    let mut state = world_state();
    state["payload"]["state"]["recorded_capacity"] = huge.clone();
    let mut search = web_search();
    search["payload"]["action"]["recorded_limit"] = huge.clone();
    search["metadata"] = json!({"synthetic_native_metadata":{"kept":true}});
    let mut tokens = usage();
    tokens["payload"]["usage"]["input_tokens"] = huge.clone();
    let records = [
        header(),
        user("request", &["Find the synthetic fixture"]),
        tools,
        state,
        search,
        tokens,
        json!({"type":"response_item","payload":{"type":"message","id":"answer","role":"assistant",
            "content":[{"type":"output_text","text":"Synthetic answer"}]}}),
    ];
    let input = bytes(&records);
    let parsed = parse(Harness::Codex, "synthetic.jsonl", &input).unwrap();
    let session = &parsed.sessions[0];
    assert_eq!(session.title(), "Find the synthetic fixture");
    assert_eq!(session.messages.len(), 3);
    assert_eq!(session.events.len(), 4);
    let search = &session.messages[1];
    assert_eq!(search.role, Role::Assistant);
    assert_eq!(search.origin, Origin::Model);
    assert_eq!(search.native_role, "assistant");
    assert_eq!(search.parent_key.as_deref(), Some("request"));
    assert_eq!(search.key, "line:5");
    assert_eq!(search.native_ids, ["line:5"]);
    assert_eq!(session.messages[2].parent_key.as_deref(), Some("line:5"));
    match &search.content[..] {
        [
            FullContentBlock::ToolCall {
                call_id,
                name,
                input: ToolInput::Json(action),
            },
        ] => {
            assert!(
                call_id.is_none(),
                "a native item without a call ID must remain unresolved"
            );
            assert_eq!(name, "web_search");
            assert_eq!(action, &records[4]["payload"]["action"]);
            assert_eq!(action["recorded_limit"], huge);
        }
        other => panic!("unexpected web search projection: {other:?}"),
    }
    assert!(
        !session
            .messages
            .iter()
            .flat_map(|m| &m.content)
            .any(|b| matches!(b, FullContentBlock::ToolResult { .. }))
    );
    for (event, index) in session.events.iter().zip([2, 3, 4, 5]) {
        assert_eq!(event.data, records[index]);
        assert_eq!(event.anchor.line, index as u64 + 1);
        let offset: usize = records[..index]
            .iter()
            .map(|r| r.to_string().len() + 1)
            .sum();
        assert_eq!(event.anchor.byte_offset, offset as u64);
        let source = &input[offset..offset + event.anchor.byte_length as usize];
        assert_eq!(serde_json::from_slice::<Value>(source).unwrap(), event.data);
    }
    assert_eq!(
        session
            .events
            .iter()
            .map(|e| e.event_type.as_str())
            .collect::<Vec<_>>(),
        [
            "additional_tools",
            "world_state",
            "web_search_call",
            "token_usage_record"
        ]
    );
    assert_eq!(session.events[0].anchor.record_id, "definitions");
    assert_eq!(
        session.events[2].anchor.record_id,
        search.anchors[0].record_id
    );
    assert_eq!(
        session.events[2].anchor.byte_offset,
        search.anchors[0].byte_offset
    );
    assert!(search.timestamp.is_some());
}

#[test]
fn only_recorded_web_search_call_ids_are_used() {
    let mut search = web_search();
    search["payload"]["id"] = json!("provider-item-id");
    let parsed = session(&[header(), search.clone()]);
    assert_eq!(parsed.messages[0].key, "provider-item-id");
    assert!(matches!(
        &parsed.messages[0].content[0],
        FullContentBlock::ToolCall { call_id: None, .. }
    ));
    search["payload"]["call_id"] = json!("recorded-call");
    let parsed = session(&[header(), search]);
    assert!(
        matches!(&parsed.messages[0].content[0], FullContentBlock::ToolCall { call_id: Some(id), .. } if id == "recorded-call")
    );
}

#[test]
fn injected_envelopes_keep_native_content_but_do_not_supply_default_titles() {
    let agents =
        "# AGENTS.md instructions for /fixture\n\n<INSTRUCTIONS>\nSynthetic rules\n</INSTRUCTIONS>";
    let environment = "<environment_context>\n<cwd>/fixture</cwd>\n</environment_context>";
    let parsed = session(&[
        header(),
        user("instructions", &[agents]),
        user("environment", &[environment]),
        user("empty", &["", " \n\t"]),
        user(
            "human",
            &["", " \n  Implement the synthetic feature  \nMore detail"],
        ),
    ]);
    assert_eq!(parsed.title(), "Implement the synthetic feature");
    assert_eq!(parsed.messages.len(), 4);
    for (message, text) in parsed.messages.iter().take(2).zip([agents, environment]) {
        assert_eq!(message.origin, Origin::Harness);
        assert_eq!(message.role, Role::User);
        assert_eq!(message.native_role, "user");
        assert_eq!(message.content, [FullContentBlock::text(text)]);
    }
    assert_eq!(
        parsed.messages[1].parent_key.as_deref(),
        Some("instructions")
    );
    assert_eq!(parsed.messages[3].origin, Origin::NativeUser);
    assert_eq!(parsed.messages[3].parent_key.as_deref(), Some("empty"));
    assert_eq!(
        parsed.messages[3].content[1],
        FullContentBlock::text(" \n  Implement the synthetic feature  \nMore detail")
    );
    let envelopes = session(&[header(), user("both", &[" ", agents, environment])]);
    assert_eq!(envelopes.messages[0].origin, Origin::Harness);
    assert_eq!(envelopes.title(), "Untitled session");
}

#[test]
fn human_mentions_and_mixed_messages_are_not_classified_as_harness() {
    let agents =
        "# AGENTS.md instructions for /fixture\n<INSTRUCTIONS>\nSynthetic rules\n</INSTRUCTIONS>";
    for texts in [
        vec!["Why does AGENTS.md mention <environment_context> and <INSTRUCTIONS>?"],
        vec!["# AGENTS.md instructions for /fixture\nWhat do these instructions mean?"],
        vec!["<environment_context>fixture</environment_context> Is this correct?"],
        vec![agents, "Please explain these instructions."],
        vec!["# AGENTS.md instructions for \n<INSTRUCTIONS>empty directory</INSTRUCTIONS>"],
    ] {
        let parsed = session(&[header(), user("human", &texts)]);
        assert_eq!(parsed.messages[0].origin, Origin::NativeUser);
        assert_eq!(parsed.messages[0].role, Role::User);
        assert_eq!(parsed.messages[0].content.len(), texts.len());
        assert_ne!(parsed.title(), "Untitled session");
    }
    let title = "é".repeat(170);
    assert_eq!(
        session(&[header(), user("human", &[&title])]).title(),
        "é".repeat(160)
    );
}

#[test]
fn recognized_records_still_reject_malformed_and_unknown_native_shapes() {
    let mut invalid = vec![
        json!({"type":"world_state","payload":{"full":"true","state":{}}}),
        json!({"type":"world_state","payload":{"full":true,"state":[]}}),
        json!({"type":"world_state","payload":{"full":false}}),
        json!({"type":"future_world_state","payload":{}}),
        json!({"type":"response_item","payload":{"type":"future_tool_call","action":{}}}),
    ];
    for (path, value) in [
        ("/payload/status", json!(false)),
        ("/payload/action", Value::Null),
        ("/payload/action/type", json!("future_action")),
        ("/payload/action/query", json!(7)),
        ("/payload/action/queries", json!(["valid", 7])),
        ("/payload/call_id", json!(false)),
        ("/payload/id", json!(7)),
    ] {
        let mut record = web_search();
        if path == "/payload/call_id" || path == "/payload/id" {
            let field = path.rsplit('/').next().unwrap();
            record["payload"][field] = value;
        } else {
            *record.pointer_mut(path).unwrap() = value;
        }
        invalid.push(record);
    }
    let mut missing_search = web_search();
    missing_search["payload"]["action"]
        .as_object_mut()
        .unwrap()
        .remove("queries");
    invalid.push(missing_search);
    for (path, value) in [
        ("/payload/id", json!("")),
        ("/payload/role", json!("future_role")),
        ("/payload/tools", json!({})),
        ("/payload/tools/0/type", json!("future_definition")),
        ("/payload/tools/0/tools", json!(false)),
        ("/payload/tools/0/tools/0/type", json!("future_definition")),
        ("/payload/tools/0/tools/0/parameters", Value::Null),
        ("/payload/tools/0/tools/0/strict", json!("false")),
        ("/payload/tools/0/tools/1/format", json!("grammar")),
    ] {
        let mut record = additional_tools();
        *record.pointer_mut(path).unwrap() = value;
        invalid.push(record);
    }
    for (path, value) in [
        ("/payload/thread_id", json!(7)),
        ("/payload/usage", json!([])),
        ("/payload/usage/input_tokens", json!(-1)),
        ("/payload/turn_token_usage/total_tokens", json!(1.5)),
        ("/payload/thread_token_usage/output_tokens", json!("5")),
    ] {
        let mut record = usage();
        *record.pointer_mut(path).unwrap() = value;
        invalid.push(record);
    }
    let mut missing_usage = usage();
    missing_usage["payload"]["usage"]
        .as_object_mut()
        .unwrap()
        .remove("total_tokens");
    invalid.push(missing_usage);
    for mut record in [world_state(), usage(), web_search(), additional_tools()] {
        record["ordinal"] = json!(-1);
        invalid.push(record);
    }
    for record in invalid {
        let error = parse(
            Harness::Codex,
            "synthetic.jsonl",
            &bytes(&[
                header(),
                user("valid-prefix", &["Synthetic request"]),
                record,
            ]),
        )
        .expect_err("invalid native records must reject the whole file");
        assert!(matches!(error, AdapterError::InvalidRecord { line: 3, .. }));
    }
}

#[test]
fn title_events_use_complete_native_fields_and_manual_native_title_precedence() {
    let records = [
        json!({"type":"user","uuid":"human","sessionId":"synthetic-claude",
            "message":{"role":"user","content":" \n Human fallback title"}}),
        json!({"type":"ai-title","sessionId":"synthetic-claude","aiTitle":"AI title"}),
        json!({"type":"custom-title","sessionId":"synthetic-claude","customTitle":" Native custom title "}),
        json!({"type":"ai-title","sessionId":"synthetic-claude","aiTitle":"Later AI title"}),
        json!({"type":"custom-title","sessionId":"synthetic-claude","customTitle":" \n "}),
    ];
    let parsed = parse(Harness::Claude, "synthetic.jsonl", &bytes(&records)).unwrap();
    assert_eq!(parsed.sessions[0].title(), "Native custom title");
    assert_eq!(parsed.sessions[0].events[1].data, records[2]);
    let parsed = parse(Harness::Claude, "synthetic.jsonl", &bytes(&records[..2])).unwrap();
    assert_eq!(parsed.sessions[0].title(), "AI title");
    let parsed = parse(Harness::Claude, "synthetic.jsonl", &bytes(&records[..1])).unwrap();
    assert_eq!(parsed.sessions[0].title(), "Human fallback title");
}
