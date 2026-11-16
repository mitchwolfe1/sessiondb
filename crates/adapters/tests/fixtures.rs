use serde_json::Value;
use sessiondb_adapters::parse;
use sessiondb_types::Harness;
use std::{collections::HashMap, fs, path::PathBuf};

#[test]
fn native_contract_fixtures() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    let manifest: Value =
        serde_json::from_slice(&fs::read(root.join("manifest.json")).unwrap()).unwrap();
    for case in manifest["cases"].as_array().unwrap() {
        let name = case["id"].as_str().unwrap();
        let harness: Harness = serde_json::from_value(case["harness"].clone()).unwrap();
        let expected: Value = serde_json::from_slice(
            &fs::read(root.join(case["expected"].as_str().unwrap())).unwrap(),
        )
        .unwrap();
        let mut sessions = Vec::new();
        for input in case["inputs"].as_array().unwrap() {
            let input = input.as_str().unwrap();
            let bytes = fs::read(root.join(input)).unwrap();
            let native_path = case["native_paths"][input].as_str().unwrap();
            let parsed =
                parse(harness, native_path, &bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
            if let Some(tail) = expected["capture"]["pending_tail_bytes"].as_u64() {
                assert_eq!(parsed.deferred_bytes as u64, tail, "{name}");
            }
            if let Some(lines) = expected["capture"]["malformed_lines"].as_array() {
                for line in lines {
                    assert!(
                        parsed
                            .warnings
                            .contains(&format!("malformed_json_line:{}", line)),
                        "{name}"
                    );
                }
            }
            sessions.extend(parsed.sessions);
        }
        assert_eq!(
            sessions.len(),
            expected["sessions"].as_array().unwrap().len(),
            "{name}"
        );
        for es in expected["sessions"].as_array().unwrap() {
            let session = sessions
                .iter()
                .find(|s| {
                    s.native_session_id == es["native_session_id"].as_str().unwrap()
                        && s.native_agent_id.as_deref() == es["native_agent_id"].as_str()
                })
                .unwrap();
            assert_eq!(
                session.recorded_cwd.as_deref(),
                es["recorded_cwd"].as_str(),
                "{name}"
            );
            let expected_messages = es["messages"].as_array().unwrap();
            assert_eq!(session.messages.len(), expected_messages.len(), "{name}");
            let key_map: HashMap<&str, &str> = expected_messages
                .iter()
                .zip(&session.messages)
                .map(|(e, a)| (e["key"].as_str().unwrap(), a.key.as_str()))
                .collect();
            let mut artifact_map = HashMap::new();
            if let Some(bindings) = case["artifact_bindings"].as_array() {
                for binding in bindings {
                    let a = session
                        .artifacts
                        .iter()
                        .find(|a| {
                            a.native_entry_id == binding["native_entry_id"].as_str().unwrap()
                                && a.content_index as u64
                                    == binding["content_index"].as_u64().unwrap()
                        })
                        .unwrap();
                    assert_eq!(
                        a.bytes.len() as u64,
                        binding["byte_length"].as_u64().unwrap()
                    );
                    artifact_map
                        .insert(a.id.to_string(), binding["id"].as_str().unwrap().to_owned());
                }
            }
            for (e, actual) in expected_messages.iter().zip(&session.messages) {
                assert_eq!(
                    serde_json::to_value(actual.role).unwrap(),
                    e["role"],
                    "{name}"
                );
                assert_eq!(
                    serde_json::to_value(actual.origin).unwrap(),
                    e["origin"],
                    "{name}"
                );
                assert_eq!(
                    serde_json::to_value(&actual.native_ids).unwrap(),
                    e["native_ids"],
                    "{name}"
                );
                assert_eq!(
                    actual.parent_key.as_deref(),
                    e["parent_key"].as_str().map(|key| key_map[key]),
                    "{name}"
                );
                if let Some(r) = e["native_role"].as_str() {
                    assert_eq!(actual.native_role, r, "{name}");
                }
                let mut blocks = serde_json::to_value(&actual.content).unwrap();
                bind_artifacts(&mut blocks, &artifact_map);
                assert_eq!(blocks, e["content"], "{name}: {}", e["key"]);
                if e.get("provider_usage").is_some() {
                    assert_eq!(
                        actual.provider_usage.as_ref(),
                        e.get("provider_usage"),
                        "{name}"
                    );
                }
                if let Some(warnings) = e["warnings"].as_array() {
                    for w in warnings {
                        assert!(
                            actual
                                .warnings
                                .iter()
                                .any(|a| Some(a.as_str()) == w.as_str()),
                            "{name}"
                        );
                    }
                }
            }
            assert_eq!(
                session.events.len(),
                es["events"].as_array().unwrap().len(),
                "{name}"
            );
            for (e, a) in es["events"].as_array().unwrap().iter().zip(&session.events) {
                assert_eq!(a.event_type, e["type"].as_str().unwrap(), "{name}");
                if let Some(target) = e["target_native_id"].as_str() {
                    assert_eq!(a.data["targetId"], target, "{name}");
                }
                if let Some(parent) = e["logical_parent_native_id"].as_str() {
                    assert_eq!(a.data["logicalParentUuid"], parent, "{name}");
                }
            }
            assert_eq!(
                session.relations.len(),
                es["relations"].as_array().unwrap().len(),
                "{name}"
            );
            for (e, a) in es["relations"]
                .as_array()
                .unwrap()
                .iter()
                .zip(&session.relations)
            {
                assert_eq!(a.relation_type, e["type"].as_str().unwrap(), "{name}");
                let target = expected["sessions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|s| s["key"] == e["target"])
                    .unwrap();
                assert_eq!(
                    a.native_reference,
                    target["native_session_id"].as_str().unwrap(),
                    "{name}"
                );
                if let Some(b) = e.get("history_boundary") {
                    assert_eq!(
                        serde_json::to_value(&a.history_boundary).unwrap(),
                        *b,
                        "{name}"
                    );
                }
            }
        }
    }
}

fn bind_artifacts(value: &mut Value, ids: &HashMap<String, String>) {
    match value {
        Value::Object(object) => {
            if let Some(Value::String(id)) = object.get_mut("artifact_id")
                && let Some(bound) = ids.get(id)
            {
                *id = bound.clone();
            }
            for v in object.values_mut() {
                bind_artifacts(v, ids);
            }
        }
        Value::Array(items) => {
            for v in items {
                bind_artifacts(v, ids);
            }
        }
        _ => {}
    }
}

#[test]
fn ambiguous_tool_results_are_not_guessed() {
    let bytes = concat!(
        "{\"type\":\"session_meta\",\"payload\":{\"id\":\"fixture\"}}\n",
        "{\"type\":\"response_item\",\"payload\":{\"type\":\"function_call_output\",\"call_id\":\"missing\",\"output\":\"{\\\"looks\\\":true}\"}}\n"
    );
    let parsed = parse(Harness::Codex, "session.jsonl", bytes.as_bytes()).unwrap();
    let message = &parsed.sessions[0].messages[0];
    let content = serde_json::to_value(&message.content).unwrap();
    assert!(content[0]["call_id"].is_null());
    assert_eq!(content[0]["content"][0]["type"], "text");
    assert!(message.warnings.contains(&"unresolved_tool_call".into()));
}
