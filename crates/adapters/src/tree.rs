use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use sessiondb_types::{ToolInput, ToolOutputBlock, ToolStatus};

pub(super) fn claude(path: &str, records: &[Record]) -> Result<NativeSession, AdapterError> {
    let header = records
        .iter()
        .find(|r| str_at(&r.value, "sessionId").is_some())
        .ok_or(AdapterError::Unsupported)?;
    let id = str_at(&header.value, "sessionId").ok_or(AdapterError::Unsupported)?;
    let mut session = native_session(
        id,
        str_at(&header.value, "cwd"),
        header.value.get("timestamp").and_then(timestamp),
    );
    let agent = records
        .iter()
        .find_map(|r| str_at(&r.value, "agentId"))
        .map(str::to_owned)
        .or_else(|| {
            path.rsplit('/')
                .next()
                .and_then(|s| s.strip_prefix("agent-"))
                .and_then(|s| s.strip_suffix(".jsonl"))
                .map(str::to_owned)
        });
    if let Some(agent_id) = agent {
        session.native_key = format!("{id}/agent:{agent_id}");
        session.native_agent_id = Some(agent_id);
        session.relations.push(NativeRelation {
            relation_type: "subagent".into(),
            native_reference: id.into(),
            history_boundary: None,
        });
    }
    let parents = parent_map(records, "parentUuid");
    let mut mapping = HashMap::new();
    for record in records {
        let value = &record.value;
        let kind = str_at(value, "type").unwrap_or("unknown");
        if !matches!(kind, "user" | "assistant") {
            session
                .events
                .push(event(record, str_at(value, "subtype").unwrap_or(kind)));
            continue;
        }
        let parent = resolve_parent(str_at(value, "parentUuid"), &parents, &mapping);
        let p = &value["message"];
        let r = role(str_at(p, "role").unwrap_or(kind));
        let injected = value["isMeta"] == true
            || value["isCompactSummary"] == true
            || session.native_agent_id.is_some();
        let mut parts = Vec::<(Role, Vec<FullContentBlock>)>::new();
        if let Some(text) = p["content"].as_str() {
            parts.push((r, vec![FullContentBlock::text(text)]));
        } else if let Some(blocks) = p["content"].as_array() {
            for (index, b) in blocks.iter().enumerate() {
                let is_result = str_at(b, "type") == Some("tool_result");
                let block_role = if is_result { Role::Tool } else { r };
                let block = if is_result {
                    FullContentBlock::ToolResult {
                        call_id: str_at(b, "tool_use_id").map(str::to_owned),
                        status: status(b.get("is_error")),
                        content: outputs(&b["content"], &mut session, record, index),
                    }
                } else {
                    block(b, &mut session, record, index, Harness::Claude)
                };
                if let Some((_, content)) = parts.last_mut().filter(|(role, _)| *role == block_role)
                {
                    content.push(block);
                } else {
                    parts.push((block_role, vec![block]));
                }
            }
        }
        let mut current_parent = parent;
        for (index, (r, content)) in parts.into_iter().enumerate() {
            let mut m = message(
                record,
                r,
                if r == Role::User && injected {
                    Origin::Harness
                } else {
                    origin(r)
                },
                current_parent.clone(),
                content,
            );
            if index > 0 {
                m.key = format!("{}:{index}", m.key);
            }
            m.native_role = str_at(p, "role").unwrap_or(kind).into();
            m.provider_response_id = str_at(p, "id").map(str::to_owned);
            m.model = str_at(p, "model").map(str::to_owned);
            m.provider_usage = p.get("usage").cloned();
            let can_merge = r == Role::Assistant
                && m.provider_response_id.is_some()
                && session.messages.last().is_some_and(|last| {
                    last.role == r
                        && last.provider_response_id == m.provider_response_id
                        && Some(&last.key) == m.parent_key.as_ref()
                });
            if can_merge {
                let last = session.messages.last_mut().expect("checked above");
                last.content.extend(m.content);
                last.native_ids.extend(m.native_ids);
                last.anchors.extend(m.anchors);
                if m.provider_usage.is_some() {
                    last.provider_usage = m.provider_usage;
                }
                current_parent = Some(last.key.clone());
            } else {
                current_parent = Some(m.key.clone());
                session.messages.push(m);
            }
        }
        if let Some(key) = current_parent {
            mapping.insert(record.anchor.record_id.clone(), key);
        }
    }
    Ok(session)
}

pub(super) fn pi(records: &[Record]) -> Result<NativeSession, AdapterError> {
    let header = records
        .iter()
        .find(|r| str_at(&r.value, "type") == Some("session"))
        .ok_or(AdapterError::Unsupported)?;
    let id = str_at(&header.value, "id").ok_or(AdapterError::Unsupported)?;
    if header.value["version"].as_u64() != Some(3) {
        return Err(AdapterError::Unsupported);
    }
    let mut session = native_session(
        id,
        str_at(&header.value, "cwd"),
        header.value.get("timestamp").and_then(timestamp),
    );
    if let Some(parent) = str_at(&header.value, "parentSession") {
        session.relations.push(NativeRelation {
            relation_type: "fork".into(),
            native_reference: parent.into(),
            history_boundary: None,
        });
    }
    let parents = parent_map(records, "parentId");
    let mut mapping = HashMap::new();
    for record in records {
        let value = &record.value;
        let kind = str_at(value, "type").unwrap_or("unknown");
        if kind == "session" {
            continue;
        }
        if !matches!(kind, "message" | "custom_message") {
            session.events.push(event(record, kind));
            continue;
        }
        let parent = resolve_parent(str_at(value, "parentId"), &parents, &mapping);
        let p = if kind == "custom_message" {
            value
        } else {
            &value["message"]
        };
        let native_role = if kind == "custom_message" {
            "custom"
        } else {
            str_at(p, "role").unwrap_or("unknown")
        };
        let r = match native_role {
            "toolResult" => Role::Tool,
            "custom" | "compactionSummary" | "branchSummary" => Role::User,
            other => role(other),
        };
        let injected = matches!(
            native_role,
            "custom" | "compactionSummary" | "branchSummary"
        );
        let content = if r == Role::Tool {
            vec![FullContentBlock::ToolResult {
                call_id: str_at(p, "toolCallId").map(str::to_owned),
                status: status(p.get("isError")),
                content: outputs(&p["content"], &mut session, record, 0),
            }]
        } else if let Some(text) = p["content"].as_str() {
            vec![FullContentBlock::text(text)]
        } else {
            p["content"]
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .enumerate()
                        .map(|(i, b)| block(b, &mut session, record, i, Harness::Pi))
                        .collect()
                })
                .unwrap_or_default()
        };
        let mut m = message(
            record,
            r,
            if injected { Origin::Harness } else { origin(r) },
            parent,
            content,
        );
        m.native_role = native_role.into();
        m.model = str_at(p, "model").map(str::to_owned);
        m.provider_usage = p.get("usage").cloned();
        m.timestamp = p.get("timestamp").and_then(timestamp).or(m.timestamp);
        mapping.insert(record.anchor.record_id.clone(), m.key.clone());
        session.messages.push(m);
    }
    Ok(session)
}

fn parent_map(records: &[Record], field: &str) -> HashMap<String, Option<String>> {
    records
        .iter()
        .map(|r| {
            (
                r.anchor.record_id.clone(),
                str_at(&r.value, field).map(str::to_owned),
            )
        })
        .collect()
}
fn status(value: Option<&Value>) -> ToolStatus {
    match value.and_then(Value::as_bool) {
        Some(true) => ToolStatus::Error,
        Some(false) => ToolStatus::Success,
        None => ToolStatus::Unknown,
    }
}
fn block(
    b: &Value,
    session: &mut NativeSession,
    record: &Record,
    index: usize,
    harness: Harness,
) -> FullContentBlock {
    let kind = str_at(b, "type").unwrap_or("unknown");
    match kind {
        "text" => FullContentBlock::text(str_at(b, "text").unwrap_or("")),
        "thinking" => FullContentBlock::Reasoning {
            text: str_at(b, "thinking").unwrap_or("").into(),
            redacted: false,
        },
        "tool_use" | "toolCall" => FullContentBlock::ToolCall {
            call_id: str_at(b, "id").map(str::to_owned),
            name: str_at(b, "name").unwrap_or("unknown").into(),
            input: ToolInput::Json(if harness == Harness::Claude {
                b["input"].clone()
            } else {
                b["arguments"].clone()
            }),
        },
        "image" => {
            let media = str_at(b, "mimeType").or_else(|| str_at(&b["source"], "media_type"));
            let data = str_at(b, "data").or_else(|| str_at(&b["source"], "data"));
            if let (Some(media_type), Some(Ok(bytes))) = (media, data.map(|s| STANDARD.decode(s))) {
                let id = Uuid::new_v5(
                    &Uuid::NAMESPACE_URL,
                    format!(
                        "sessiondb:{}:{}:{}:{index}",
                        harness, session.native_key, record.anchor.record_id
                    )
                    .as_bytes(),
                );
                session.artifacts.push(NativeArtifact {
                    id,
                    media_type: media_type.into(),
                    kind: ArtifactKind::Image,
                    bytes,
                    native_entry_id: record.anchor.record_id.clone(),
                    content_index: index,
                });
                FullContentBlock::Artifact {
                    artifact_id: id,
                    media_type: media_type.into(),
                    kind: ArtifactKind::Image,
                    alt: None,
                }
            } else {
                session.warnings.push("unavailable_inline_artifact".into());
                FullContentBlock::Unknown {
                    native_type: kind.into(),
                    data: b.clone(),
                }
            }
        }
        _ => FullContentBlock::Unknown {
            native_type: kind.into(),
            data: b.clone(),
        },
    }
}
fn outputs(
    value: &Value,
    session: &mut NativeSession,
    record: &Record,
    base_index: usize,
) -> Vec<ToolOutputBlock> {
    if let Some(text) = value.as_str() {
        return vec![ToolOutputBlock::Text { text: text.into() }];
    }
    let Some(items) = value.as_array() else {
        return vec![ToolOutputBlock::Json {
            value: value.clone(),
        }];
    };
    items
        .iter()
        .enumerate()
        .map(
            |(i, b)| match block(b, session, record, base_index + i, Harness::Pi) {
                FullContentBlock::Text { text } => ToolOutputBlock::Text { text },
                FullContentBlock::Artifact {
                    artifact_id,
                    media_type,
                    kind,
                    alt,
                } => ToolOutputBlock::Artifact {
                    artifact_id,
                    media_type,
                    kind,
                    alt,
                },
                FullContentBlock::Json { value } => ToolOutputBlock::Json { value },
                _ => ToolOutputBlock::Unknown {
                    native_type: str_at(b, "type").unwrap_or("unknown").into(),
                    data: b.clone(),
                },
            },
        )
        .collect()
}
