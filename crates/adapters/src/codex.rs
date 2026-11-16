use super::*;
use sessiondb_types::{ToolInput, ToolOutputBlock, ToolStatus};

pub(super) fn parse(records: &[Record]) -> Result<NativeSession, AdapterError> {
    let header = records
        .iter()
        .find(|r| str_at(&r.value, "type") == Some("session_meta"))
        .ok_or(AdapterError::Unsupported)?;
    let p = &header.value["payload"];
    let id = str_at(p, "id").ok_or(AdapterError::Unsupported)?;
    let mut session = native_session(id, str_at(p, "cwd"), p.get("timestamp").and_then(timestamp));
    if let Some(parent) = str_at(p, "forked_from_id") {
        session.relations.push(NativeRelation {
            relation_type: "fork".into(),
            native_reference: parent.into(),
            history_boundary: None,
        });
    }
    if let Some(base) = p.get("history_base") {
        if let (Some(rollout_id), Some(end_ordinal_exclusive), Some(end_byte_offset)) = (
            str_at(base, "thread_id"),
            base["end_ordinal_exclusive"].as_u64(),
            base["end_byte_offset"].as_u64(),
        ) {
            session.relations.push(NativeRelation {
                relation_type: "inherited_context".into(),
                native_reference: rollout_id.into(),
                history_boundary: Some(HistoryBoundary {
                    rollout_id: rollout_id.into(),
                    end_ordinal_exclusive,
                    end_byte_offset,
                }),
            });
        } else {
            session.warnings.push("invalid_history_boundary".into());
        }
    }
    let mut parent = None;
    for record in records {
        let p = &record.value["payload"];
        match str_at(&record.value, "type").unwrap_or("") {
            "session_meta" => continue,
            "response_item" => {
                let kind = str_at(p, "type").unwrap_or("unknown");
                let mut warning = None;
                let (r, content) = match kind {
                    "message" => (
                        role(str_at(p, "role").unwrap_or("unknown")),
                        blocks(&p["content"]),
                    ),
                    "function_call" | "custom_tool_call" => {
                        let input = if kind == "custom_tool_call" {
                            ToolInput::Text(str_at(p, "input").unwrap_or("").into())
                        } else if let Some(args) = str_at(p, "arguments") {
                            match serde_json::from_str(args) {
                                Ok(v) => ToolInput::Json(v),
                                Err(_) => {
                                    warning = Some("invalid_tool_arguments_json".into());
                                    ToolInput::Text(args.into())
                                }
                            }
                        } else {
                            ToolInput::Json(p["arguments"].clone())
                        };
                        (
                            Role::Assistant,
                            vec![FullContentBlock::ToolCall {
                                call_id: str_at(p, "call_id").map(str::to_owned),
                                name: str_at(p, "name").unwrap_or("unknown").into(),
                                input,
                            }],
                        )
                    }
                    "function_call_output" | "custom_tool_call_output" => {
                        let output = if let Some(text) = p["output"].as_str() {
                            vec![ToolOutputBlock::Text { text: text.into() }]
                        } else {
                            vec![ToolOutputBlock::Json {
                                value: p["output"].clone(),
                            }]
                        };
                        (
                            Role::Tool,
                            vec![FullContentBlock::ToolResult {
                                call_id: str_at(p, "call_id").map(str::to_owned),
                                status: ToolStatus::Unknown,
                                content: output,
                            }],
                        )
                    }
                    "reasoning" => {
                        let mut content = blocks(&p["summary"]);
                        content.extend(blocks(&p["content"]));
                        for block in &mut content {
                            if let FullContentBlock::Text { text } = block {
                                *block = FullContentBlock::Reasoning {
                                    text: text.clone(),
                                    redacted: false,
                                };
                            }
                        }
                        if let Some(data) = p.get("encrypted_content").filter(|v| !v.is_null()) {
                            content.push(FullContentBlock::Unknown {
                                native_type: "encrypted_reasoning".into(),
                                data: data.clone(),
                            });
                        }
                        (Role::Assistant, content)
                    }
                    _ => {
                        session.events.push(event(record, kind));
                        session
                            .warnings
                            .push(format!("unknown_response_item:{kind}"));
                        continue;
                    }
                };
                let mut m = message(record, r, origin(r), parent.clone(), content);
                m.native_role = str_at(p, "role").unwrap_or(r.as_str()).into();
                if let Some(w) = warning {
                    m.warnings.push(w);
                }
                parent = Some(m.key.clone());
                session.messages.push(m);
            }
            "event_msg" => {
                let kind = str_at(p, "type").unwrap_or("unknown");
                if !matches!(kind, "agent_message" | "user_message" | "agent_reasoning") {
                    session.events.push(event(record, kind));
                }
            }
            kind => session.events.push(event(record, kind)),
        }
    }
    Ok(session)
}

fn blocks(value: &Value) -> Vec<FullContentBlock> {
    let Some(items) = value.as_array() else {
        return vec![];
    };
    items
        .iter()
        .map(|b| {
            let kind = str_at(b, "type").unwrap_or("unknown");
            match kind {
                "input_text" | "output_text" | "text" | "summary_text" | "reasoning_text" => {
                    FullContentBlock::text(str_at(b, "text").unwrap_or(""))
                }
                _ => FullContentBlock::Unknown {
                    native_type: kind.into(),
                    data: b.clone(),
                },
            }
        })
        .collect()
}
