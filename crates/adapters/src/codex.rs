use super::*;
use sessiondb_types::{ToolInput, ToolOutputBlock, ToolStatus};

pub(super) fn parse(records: &[Record]) -> Result<NativeSession, AdapterError> {
    let header = records
        .first()
        .filter(|r| str_at(&r.value, "type") == Some("session_meta"))
        .ok_or(AdapterError::Unsupported)?;
    let p = object(required(&header.value, "payload", header)?, header)?;
    let id = required_str(p, "id", header)?;
    if id.is_empty() {
        return Err(invalid(header, "empty native session identifier"));
    }
    let mut session = native_session(
        id,
        optional_str(p, "cwd", header)?,
        p.get("timestamp").and_then(timestamp),
    );
    validate_timestamp(p, header)?;
    if let Some(parent) = optional_str(p, "forked_from_id", header)? {
        session.relations.push(NativeRelation {
            relation_type: "fork".into(),
            native_reference: parent.into(),
            history_boundary: None,
        });
    }
    if let Some(base) = p.get("history_base") {
        let base = object(base, header)?;
        let rollout_id = required_str(base, "thread_id", header)?;
        if rollout_id.is_empty() {
            return Err(invalid(header, "invalid inherited history reference"));
        }
        let end_ordinal_exclusive = base["end_ordinal_exclusive"]
            .as_u64()
            .ok_or_else(|| invalid(header, "invalid inherited history cutoff"))?;
        let end_byte_offset = base["end_byte_offset"]
            .as_u64()
            .ok_or_else(|| invalid(header, "invalid inherited history cutoff"))?;
        session.relations.push(NativeRelation {
            relation_type: "inherited_context".into(),
            native_reference: rollout_id.into(),
            history_boundary: Some(HistoryBoundary {
                rollout_id: rollout_id.into(),
                end_ordinal_exclusive,
                end_byte_offset,
            }),
        });
    }
    let mut parent = None;
    for record in records.iter().skip(1) {
        let p = object(required(&record.value, "payload", record)?, record)?;
        let kind = required_str(&record.value, "type", record)?;
        match kind {
            "response_item" => {
                let kind = required_str(p, "type", record)?;
                if kind == "additional_tools" {
                    validate_ordinal(record)?;
                    validate_additional_tools(p, record)?;
                    session.events.push(event(record, kind));
                    continue;
                }
                let (r, content) = match kind {
                    "message" => (
                        role(required_str(p, "role", record)?, record)?,
                        blocks(required(p, "content", record)?, record)?,
                    ),
                    "function_call" | "custom_tool_call" => {
                        let input = if kind == "custom_tool_call" {
                            ToolInput::Text(required_str(p, "input", record)?.into())
                        } else {
                            let arguments = required(p, "arguments", record)?;
                            ToolInput::Json(if let Some(args) = arguments.as_str() {
                                serde_json::from_str(args).map_err(|_| {
                                    invalid(record, "invalid function arguments JSON")
                                })?
                            } else {
                                arguments.clone()
                            })
                        };
                        (
                            Role::Assistant,
                            vec![FullContentBlock::ToolCall {
                                call_id: optional_str(p, "call_id", record)?.map(str::to_owned),
                                name: required_str(p, "name", record)?.into(),
                                input,
                            }],
                        )
                    }
                    "function_call_output" | "custom_tool_call_output" => {
                        let value = required(p, "output", record)?;
                        let output = outputs(value, record)?;
                        (
                            Role::Tool,
                            vec![FullContentBlock::ToolResult {
                                call_id: optional_str(p, "call_id", record)?.map(str::to_owned),
                                status: ToolStatus::Unknown,
                                content: output,
                            }],
                        )
                    }
                    "web_search_call" => {
                        validate_ordinal(record)?;
                        required_str(p, "status", record)?;
                        let action = object(required(p, "action", record)?, record)?;
                        if required_str(action, "type", record)? != "search" {
                            return Err(invalid(record, "unsupported web search action kind"));
                        }
                        required_str(action, "query", record)?;
                        string_array(required(action, "queries", record)?, record)?;
                        optional_str(p, "id", record)?;
                        let call_id = optional_str(p, "call_id", record)?.map(str::to_owned);
                        session.events.push(event(record, kind));
                        (
                            Role::Assistant,
                            vec![FullContentBlock::ToolCall {
                                call_id,
                                name: "web_search".into(),
                                input: ToolInput::Json(action.clone()),
                            }],
                        )
                    }
                    "reasoning" => {
                        let mut content = Vec::new();
                        for key in ["summary", "content"] {
                            if let Some(value) = p.get(key).filter(|v| !v.is_null()) {
                                for block in blocks(value, record)? {
                                    if let FullContentBlock::Text { text } = block {
                                        content.push(FullContentBlock::Reasoning {
                                            text,
                                            redacted: false,
                                        });
                                    }
                                }
                            }
                        }
                        if optional_str(p, "encrypted_content", record)?.is_some() {
                            content.push(FullContentBlock::Reasoning {
                                text: String::new(),
                                redacted: true,
                            });
                            session.events.push(event(record, "encrypted_reasoning"));
                        }
                        (Role::Assistant, content)
                    }
                    _ => return Err(invalid(record, "unsupported response item kind")),
                };
                let mut m = message(record, r, origin(r), parent.clone(), content);
                m.native_role = str_at(p, "role").unwrap_or(r.as_str()).into();
                if r == Role::User && injected_user_content(&m.content) {
                    m.origin = Origin::Harness;
                }
                parent = Some(m.key.clone());
                session.messages.push(m);
            }
            "event_msg" => {
                let kind = required_str(p, "type", record)?;
                session.events.push(event(record, kind));
            }
            "turn_context" | "compacted" => session.events.push(event(record, kind)),
            "world_state" => {
                validate_ordinal(record)?;
                if !required(p, "full", record)?.is_boolean() {
                    return Err(invalid(record, "expected a boolean field"));
                }
                object(required(p, "state", record)?, record)?;
                session.events.push(event(record, kind));
            }
            "token_usage_record" => {
                validate_ordinal(record)?;
                for key in [
                    "thread_id",
                    "turn_id",
                    "session_id",
                    "root_turn_id",
                    "response_id",
                ] {
                    required_str(p, key, record)?;
                }
                for key in ["usage", "turn_token_usage", "thread_token_usage"] {
                    let usage = object(required(p, key, record)?, record)?;
                    for counter in [
                        "input_tokens",
                        "cached_input_tokens",
                        "cache_write_input_tokens",
                        "output_tokens",
                        "reasoning_output_tokens",
                        "total_tokens",
                    ] {
                        unsigned_integer(required(usage, counter, record)?, record)?;
                    }
                }
                session.events.push(event(record, kind));
            }
            _ => return Err(invalid(record, "unsupported outer record kind")),
        }
    }
    Ok(session)
}

fn injected_user_content(content: &[FullContentBlock]) -> bool {
    let mut envelope = false;
    for block in content {
        let FullContentBlock::Text { text } = block else {
            return false;
        };
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        let agents = text.split_once('\n').is_some_and(|(heading, body)| {
            heading
                .strip_prefix("# AGENTS.md instructions for ")
                .is_some_and(|directory| !directory.trim().is_empty())
                && body
                    .trim()
                    .strip_prefix("<INSTRUCTIONS>")
                    .and_then(|body| body.strip_suffix("</INSTRUCTIONS>"))
                    .is_some()
        });
        let environment = text
            .strip_prefix("<environment_context>")
            .and_then(|body| body.strip_suffix("</environment_context>"))
            .is_some();
        if !agents && !environment {
            return false;
        }
        envelope = true;
    }
    envelope
}

fn validate_additional_tools(value: &Value, record: &Record) -> Result<(), AdapterError> {
    if required_str(value, "id", record)?.is_empty() {
        return Err(invalid(record, "empty native record identifier"));
    }
    role(required_str(value, "role", record)?, record)?;
    let tools = required(value, "tools", record)?
        .as_array()
        .ok_or_else(|| invalid(record, "expected a tools array"))?;
    for namespace in tools {
        if required_str(namespace, "type", record)? != "namespace" {
            return Err(invalid(record, "unsupported tool definition kind"));
        }
        required_str(namespace, "name", record)?;
        required_str(namespace, "description", record)?;
        let definitions = required(namespace, "tools", record)?
            .as_array()
            .ok_or_else(|| invalid(record, "expected a tools array"))?;
        for definition in definitions {
            required_str(definition, "name", record)?;
            required_str(definition, "description", record)?;
            match required_str(definition, "type", record)? {
                "function" => {
                    object(required(definition, "parameters", record)?, record)?;
                    if !required(definition, "strict", record)?.is_boolean() {
                        return Err(invalid(record, "expected a boolean field"));
                    }
                }
                "custom" => {
                    object(required(definition, "format", record)?, record)?;
                }
                _ => return Err(invalid(record, "unsupported tool definition kind")),
            }
        }
    }
    Ok(())
}

fn string_array(value: &Value, record: &Record) -> Result<(), AdapterError> {
    let values = value
        .as_array()
        .ok_or_else(|| invalid(record, "expected a string array"))?;
    if values.iter().any(|value| !value.is_string()) {
        return Err(invalid(record, "expected a string array"));
    }
    Ok(())
}

fn unsigned_integer(value: &Value, record: &Record) -> Result<(), AdapterError> {
    if !value
        .as_number()
        .is_some_and(|number| number.as_str().bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err(invalid(record, "expected an unsigned integer"));
    }
    Ok(())
}

fn validate_ordinal(record: &Record) -> Result<(), AdapterError> {
    if let Some(ordinal) = record.value.get("ordinal") {
        unsigned_integer(ordinal, record)?;
    }
    Ok(())
}

fn blocks(value: &Value, record: &Record) -> Result<Vec<FullContentBlock>, AdapterError> {
    let items = value
        .as_array()
        .ok_or_else(|| invalid(record, "expected a content array"))?;
    items
        .iter()
        .map(|b| {
            let kind = required_str(b, "type", record)?;
            match kind {
                "input_text" | "output_text" | "text" | "summary_text" | "reasoning_text" => {
                    Ok(FullContentBlock::text(required_str(b, "text", record)?))
                }
                _ => Err(invalid(record, "unsupported content block kind")),
            }
        })
        .collect()
}

fn outputs(value: &Value, record: &Record) -> Result<Vec<ToolOutputBlock>, AdapterError> {
    if let Some(text) = value.as_str() {
        return Ok(vec![ToolOutputBlock::Text { text: text.into() }]);
    }
    if let Some(items) = value.as_array()
        && items.iter().any(|item| item.get("type").is_some())
    {
        return items
            .iter()
            .map(|item| match required_str(item, "type", record)? {
                "text" | "input_text" | "output_text" => Ok(ToolOutputBlock::Text {
                    text: required_str(item, "text", record)?.into(),
                }),
                _ => Err(invalid(record, "unsupported tool output block kind")),
            })
            .collect();
    }
    if value.get("type").is_some() {
        return Err(invalid(record, "unsupported tool output shape"));
    }
    Ok(vec![ToolOutputBlock::Json {
        value: value.clone(),
    }])
}
