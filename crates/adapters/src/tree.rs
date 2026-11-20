use super::*;
use sessiondb_types::{ToolInput, ToolOutputBlock, ToolStatus};

pub(super) fn claude(path: &str, records: &[Record]) -> Result<NativeSession, AdapterError> {
    let header = records
        .iter()
        .find(|r| str_at(&r.value, "sessionId").is_some())
        .ok_or(AdapterError::Unsupported)?;
    let id = str_at(&header.value, "sessionId").ok_or(AdapterError::Unsupported)?;
    if id.is_empty() {
        return Err(invalid(header, "empty native session identifier"));
    }
    let mut session = native_session(
        id,
        records.iter().find_map(|r| str_at(&r.value, "cwd")),
        records
            .iter()
            .find_map(|r| r.value.get("timestamp").and_then(timestamp)),
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
        optional_str(value, "parentUuid", record)?;
        if optional_str(value, "sessionId", record)?.is_some_and(|value| value != id) {
            return Err(invalid(record, "inconsistent native session identifier"));
        }
        if optional_str(value, "agentId", record)?
            .is_some_and(|value| Some(value) != session.native_agent_id.as_deref())
        {
            return Err(invalid(record, "inconsistent native agent identifier"));
        }
        optional_str(value, "cwd", record)?;
        let kind = required_str(value, "type", record)?;
        if !matches!(kind, "user" | "assistant") {
            let event_type = claude_event_type(kind, value, record)?;
            session.events.push(event(record, event_type));
            continue;
        }
        required_str(value, "uuid", record)?;
        let parent = resolve_parent(str_at(value, "parentUuid"), &parents, &mapping);
        let p = object(required(value, "message", record)?, record)?;
        let native_role = required_str(p, "role", record)?;
        if native_role != kind {
            return Err(invalid(record, "message role does not match record kind"));
        }
        let r = role(native_role, record)?;
        validate_timestamp(p, record)?;
        optional_bool(value, "isMeta", record)?;
        optional_bool(value, "isCompactSummary", record)?;
        optional_str(p, "id", record)?;
        optional_str(p, "model", record)?;
        if let Some(usage) = p.get("usage") {
            object(usage, record)?;
        }
        let injected = value["isMeta"] == true
            || value["isCompactSummary"] == true
            || session.native_agent_id.is_some();
        let mut parts = Vec::<(Role, Vec<FullContentBlock>)>::new();
        let content = required(p, "content", record)?;
        if let Some(text) = content.as_str() {
            parts.push((r, vec![FullContentBlock::text(text)]));
        } else if let Some(blocks) = content.as_array() {
            for (index, b) in blocks.iter().enumerate() {
                let is_result = required_str(b, "type", record)? == "tool_result";
                let block_role = if is_result { Role::Tool } else { r };
                let block = if is_result {
                    FullContentBlock::ToolResult {
                        call_id: optional_str(b, "tool_use_id", record)?.map(str::to_owned),
                        status: status(b.get("is_error"), record)?,
                        content: outputs(
                            required(b, "content", record)?,
                            &mut session,
                            record,
                            index,
                            Harness::Claude,
                        )?,
                    }
                } else {
                    block(b, &mut session, record, index, Harness::Claude)?
                };
                if let Some((_, content)) = parts.last_mut().filter(|(role, _)| *role == block_role)
                {
                    content.push(block);
                } else {
                    parts.push((block_role, vec![block]));
                }
            }
            if parts.is_empty() {
                parts.push((r, Vec::new()));
            }
        } else {
            return Err(invalid(record, "expected string or array message content"));
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

fn claude_event_type<'a>(
    kind: &'a str,
    value: &'a Value,
    record: &Record,
) -> Result<&'a str, AdapterError> {
    Ok(match kind {
        "mode" | "permission-mode" | "ai-title" => {
            required_str(value, "sessionId", record)?;
            let field = match kind {
                "mode" => "mode",
                "permission-mode" => "permissionMode",
                _ => "aiTitle",
            };
            required_str(value, field, record)?;
            kind
        }
        "custom-title" => {
            required_str(value, "customTitle", record)?;
            kind
        }
        "attachment" => {
            required_str(value, "uuid", record)?;
            optional_bool(value, "isSidechain", record)?;
            let attachment = object(required(value, "attachment", record)?, record)?;
            if required_str(attachment, "type", record)?.is_empty() {
                return Err(invalid(record, "empty native attachment type"));
            }
            kind
        }
        "session-owner" => {
            for field in ["sessionId", "accountUuid", "organizationUuid"] {
                required_str(value, field, record)?;
            }
            kind
        }
        "atis-latch" => {
            required_str(value, "sessionId", record)?;
            required_str(value, "atis", record)?;
            kind
        }
        "tools-used" => {
            required_str(value, "sessionId", record)?;
            string_array(required(value, "tools", record)?, record)?;
            kind
        }
        "bridge-session" => {
            required_str(value, "sessionId", record)?;
            required_str(value, "bridgeSessionId", record)?;
            unsigned_integer(required(value, "lastSequenceNum", record)?, record)?;
            optional_str(value, "ownerAccountUuid", record)?;
            optional_str(value, "ownerOrganizationUuid", record)?;
            kind
        }
        "cost-state" => {
            required_str(value, "sessionId", record)?;
            if !required(value, "totalCostUSD", record)?.is_number()
                || !required(value, "hasUnknownModelCost", record)?.is_boolean()
            {
                return Err(invalid(record, "invalid cost state"));
            }
            for field in [
                "startTime",
                "totalAPIDuration",
                "totalDuration",
                "totalLinesAdded",
                "totalLinesRemoved",
                "totalToolDuration",
            ] {
                unsigned_integer(required(value, field, record)?, record)?;
            }
            if let Some(duration) = value.get("totalAPIDurationWithoutRetries") {
                unsigned_integer(duration, record)?;
            }
            object(required(value, "modelUsage", record)?, record)?;
            kind
        }
        "file-history-delta" => {
            for field in ["messageId", "snapshotMessageId", "trackingPath"] {
                required_str(value, field, record)?;
            }
            object(required(value, "backup", record)?, record)?;
            kind
        }
        "api-request-blob" => {
            required_str(value, "sessionId", record)?;
            required_str(value, "hash", record)?;
            object(required(value, "message", record)?, record)?;
            kind
        }
        "api-request-shape" => {
            required_str(value, "sessionId", record)?;
            required_str(value, "shapeHash", record)?;
            let shape = object(required(value, "shape", record)?, record)?;
            for key in ["system", "tools"] {
                if !required(shape, key, record)?.is_array() {
                    return Err(invalid(record, "invalid API request shape"));
                }
            }
            kind
        }
        "api-request" => {
            for field in [
                "id",
                "sessionId",
                "version",
                "timestamp",
                "shapeHash",
                "querySource",
            ] {
                required_str(value, field, record)?;
            }
            unsigned_integer(required(value, "keep", record)?, record)?;
            string_array(required(value, "tail", record)?, record)?;
            object(required(value, "params", record)?, record)?;
            if !required(value, "cacheControl", record)?.is_array() {
                return Err(invalid(record, "invalid API request cache controls"));
            }
            optional_str(value, "base", record)?;
            if let Some(cut) = value.get("cut") {
                object(cut, record)?;
                required_str(value, "cutRequestDigest", record)?;
            } else {
                required_str(value, "requestDigest", record)?;
            }
            kind
        }
        "system"
        | "progress"
        | "queue-operation"
        | "file-history-snapshot"
        | "summary"
        | "last-prompt"
        | "agent-name"
        | "agent-color"
        | "tag"
        | "pr-link" => optional_str(value, "subtype", record)?.unwrap_or(kind),
        _ => return Err(invalid(record, "unsupported outer record kind")),
    })
}

pub(super) fn pi(records: &[Record]) -> Result<NativeSession, AdapterError> {
    let header = records
        .first()
        .filter(|r| str_at(&r.value, "type") == Some("session"))
        .ok_or(AdapterError::Unsupported)?;
    let id = required_str(&header.value, "id", header)?;
    if header.value["version"].as_u64() != Some(3) {
        return Err(AdapterError::Unsupported);
    }
    let mut session = native_session(
        id,
        str_at(&header.value, "cwd"),
        header.value.get("timestamp").and_then(timestamp),
    );
    optional_str(&header.value, "cwd", header)?;
    if let Some(parent) = optional_str(&header.value, "parentSession", header)? {
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
        optional_str(value, "parentId", record)?;
        let kind = required_str(value, "type", record)?;
        if kind == "session" {
            if record.anchor.line != header.anchor.line {
                return Err(invalid(record, "duplicate session header"));
            }
            continue;
        }
        required_str(value, "id", record)?;
        if !matches!(kind, "message" | "custom_message") {
            if !matches!(
                kind,
                "model_change"
                    | "thinking_level_change"
                    | "compaction"
                    | "branch_summary"
                    | "custom"
                    | "label"
                    | "session_info"
                    | "context_edit"
            ) {
                return Err(invalid(record, "unsupported outer record kind"));
            }
            session.events.push(event(record, kind));
            continue;
        }
        let parent = resolve_parent(str_at(value, "parentId"), &parents, &mapping);
        let p = if kind == "custom_message" {
            value
        } else {
            object(required(value, "message", record)?, record)?
        };
        let native_role = if kind == "custom_message" {
            "custom"
        } else {
            required_str(p, "role", record)?
        };
        let r = match native_role {
            "toolResult" => Role::Tool,
            "custom" | "compactionSummary" | "branchSummary" => Role::User,
            other => role(other, record)?,
        };
        let injected = matches!(
            native_role,
            "custom" | "compactionSummary" | "branchSummary"
        );
        validate_timestamp(p, record)?;
        optional_str(p, "model", record)?;
        if let Some(usage) = p.get("usage") {
            object(usage, record)?;
        }
        let native_content = required(p, "content", record)?;
        let content = if r == Role::Tool {
            vec![FullContentBlock::ToolResult {
                call_id: optional_str(p, "toolCallId", record)?.map(str::to_owned),
                status: status(p.get("isError"), record)?,
                content: outputs(native_content, &mut session, record, 0, Harness::Pi)?,
            }]
        } else if let Some(text) = native_content.as_str() {
            vec![FullContentBlock::text(text)]
        } else {
            native_content
                .as_array()
                .ok_or_else(|| invalid(record, "expected string or array message content"))?
                .iter()
                .enumerate()
                .map(|(i, b)| block(b, &mut session, record, i, Harness::Pi))
                .collect::<Result<Vec<_>, _>>()?
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
fn optional_bool(value: &Value, field: &str, record: &Record) -> Result<(), AdapterError> {
    if let Some(value) = value.get(field)
        && !value.is_boolean()
    {
        return Err(invalid(record, "expected a boolean field"));
    }
    Ok(())
}
fn status(value: Option<&Value>, record: &Record) -> Result<ToolStatus, AdapterError> {
    match value {
        Some(Value::Bool(true)) => Ok(ToolStatus::Error),
        Some(Value::Bool(false)) => Ok(ToolStatus::Success),
        None => Ok(ToolStatus::Unknown),
        _ => Err(invalid(record, "expected a boolean tool status")),
    }
}
fn block(
    b: &Value,
    session: &mut NativeSession,
    record: &Record,
    index: usize,
    harness: Harness,
) -> Result<FullContentBlock, AdapterError> {
    let kind = required_str(b, "type", record)?;
    Ok(match kind {
        "text" => FullContentBlock::text(required_str(b, "text", record)?),
        "thinking" => FullContentBlock::Reasoning {
            text: required_str(b, "thinking", record)?.into(),
            redacted: false,
        },
        "redacted_thinking" if harness == Harness::Claude => {
            required_str(b, "data", record)?;
            session.events.push(event(record, "redacted_thinking"));
            FullContentBlock::Reasoning {
                text: String::new(),
                redacted: true,
            }
        }
        "tool_use" if harness == Harness::Claude => FullContentBlock::ToolCall {
            call_id: optional_str(b, "id", record)?.map(str::to_owned),
            name: required_str(b, "name", record)?.into(),
            input: ToolInput::Json(object(required(b, "input", record)?, record)?.clone()),
        },
        "toolCall" if harness == Harness::Pi => FullContentBlock::ToolCall {
            call_id: optional_str(b, "id", record)?.map(str::to_owned),
            name: required_str(b, "name", record)?.into(),
            input: ToolInput::Json(object(required(b, "arguments", record)?, record)?.clone()),
        },
        "image" => {
            let (media_type, data) = if harness == Harness::Claude {
                let source = object(required(b, "source", record)?, record)?;
                if required_str(source, "type", record)? != "base64" {
                    return Err(invalid(record, "unsupported inline artifact source"));
                }
                (
                    required_str(source, "media_type", record)?,
                    required_str(source, "data", record)?,
                )
            } else {
                (
                    required_str(b, "mimeType", record)?,
                    required_str(b, "data", record)?,
                )
            };
            let id = session.add_inline_image(harness, record, index, media_type, data)?;
            FullContentBlock::Artifact {
                artifact_id: id,
                media_type: media_type.into(),
                kind: ArtifactKind::Image,
                alt: optional_str(b, "alt", record)?.map(str::to_owned),
            }
        }
        _ => return Err(invalid(record, "unsupported content block kind")),
    })
}
fn outputs(
    value: &Value,
    session: &mut NativeSession,
    record: &Record,
    base_index: usize,
    harness: Harness,
) -> Result<Vec<ToolOutputBlock>, AdapterError> {
    if let Some(text) = value.as_str() {
        return Ok(vec![ToolOutputBlock::Text { text: text.into() }]);
    }
    let Some(items) = value.as_array() else {
        if value.get("type").is_some() {
            return Err(invalid(record, "unsupported tool output shape"));
        }
        return Ok(vec![ToolOutputBlock::Json {
            value: value.clone(),
        }]);
    };
    items
        .iter()
        .enumerate()
        .map(|(i, b)| {
            if harness == Harness::Claude && str_at(b, "type") == Some("tool_reference") {
                required_str(b, "tool_name", record)?;
                return Ok(ToolOutputBlock::Json { value: b.clone() });
            }
            if !matches!(str_at(b, "type"), Some("text" | "image")) {
                return Err(invalid(record, "unsupported tool output block kind"));
            }
            match block(b, session, record, base_index + i, harness)? {
                FullContentBlock::Text { text } => Ok(ToolOutputBlock::Text { text }),
                FullContentBlock::Artifact {
                    artifact_id,
                    media_type,
                    kind,
                    alt,
                } => Ok(ToolOutputBlock::Artifact {
                    artifact_id,
                    media_type,
                    kind,
                    alt,
                }),
                _ => Err(invalid(record, "unsupported tool output block kind")),
            }
        })
        .collect()
}
