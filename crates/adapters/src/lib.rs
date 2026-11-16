//! Parse native session snapshots without reading other paths or executing content.
mod codex;
mod tree;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sessiondb_types::{ArtifactKind, FullContentBlock, Harness, HistoryBoundary, Origin, Role};
use std::collections::{HashMap, HashSet};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum AdapterError {
    #[error("native session header is missing or unsupported")]
    Unsupported,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Anchor {
    pub record_id: String,
    pub line: u64,
    pub byte_offset: u64,
    pub byte_length: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeMessage {
    pub key: String,
    pub native_ids: Vec<String>,
    pub parent_key: Option<String>,
    pub role: Role,
    pub native_role: String,
    pub origin: Origin,
    pub timestamp: Option<DateTime<Utc>>,
    pub content: Vec<FullContentBlock>,
    pub provider_response_id: Option<String>,
    pub model: Option<String>,
    pub provider_usage: Option<Value>,
    pub warnings: Vec<String>,
    pub anchors: Vec<Anchor>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeEvent {
    pub event_type: String,
    pub timestamp: Option<DateTime<Utc>>,
    pub data: Value,
    pub anchor: Anchor,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeRelation {
    pub relation_type: String,
    pub native_reference: String,
    pub history_boundary: Option<HistoryBoundary>,
}
#[derive(Debug, Clone)]
pub struct NativeArtifact {
    pub id: Uuid,
    pub media_type: String,
    pub kind: ArtifactKind,
    pub bytes: Vec<u8>,
    pub native_entry_id: String,
    pub content_index: usize,
}
#[derive(Debug, Clone)]
pub struct NativeSession {
    pub native_key: String,
    pub native_session_id: String,
    pub native_agent_id: Option<String>,
    pub recorded_cwd: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub messages: Vec<NativeMessage>,
    pub events: Vec<NativeEvent>,
    pub relations: Vec<NativeRelation>,
    pub artifacts: Vec<NativeArtifact>,
    pub warnings: Vec<String>,
}
impl NativeSession {
    pub fn title(&self) -> String {
        self.messages
            .iter()
            .filter(|m| m.origin == Origin::NativeUser)
            .flat_map(|m| &m.content)
            .find_map(|b| match b {
                FullContentBlock::Text { text } => Some(
                    text.lines()
                        .next()
                        .unwrap_or("")
                        .chars()
                        .take(160)
                        .collect(),
                ),
                _ => None,
            })
            .unwrap_or_else(|| "Untitled session".into())
    }
}
#[derive(Debug, Clone)]
pub struct ParsedFile {
    pub sessions: Vec<NativeSession>,
    pub accepted_bytes: usize,
    pub deferred_bytes: usize,
    pub warnings: Vec<String>,
}

/// Only complete newline-terminated records are captured from an active JSONL file.
pub fn complete_prefix(bytes: &[u8]) -> &[u8] {
    let end = bytes.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
    &bytes[..end]
}

/// The adapter boundary is independent of the catalog and object store.
pub trait HarnessAdapter: Send + Sync {
    fn harness(&self) -> Harness;
    fn parse(&self, path: &str, bytes: &[u8]) -> Result<ParsedFile, AdapterError>;
}

pub struct CodexAdapter;
pub struct ClaudeAdapter;
pub struct PiAdapter;
macro_rules! builtin_adapter {
    ($adapter:ident,$harness:ident) => {
        impl HarnessAdapter for $adapter {
            fn harness(&self) -> Harness {
                Harness::$harness
            }
            fn parse(&self, path: &str, bytes: &[u8]) -> Result<ParsedFile, AdapterError> {
                parse_native(self.harness(), path, bytes)
            }
        }
    };
}
builtin_adapter!(CodexAdapter, Codex);
builtin_adapter!(ClaudeAdapter, Claude);
builtin_adapter!(PiAdapter, Pi);

pub fn adapter(harness: Harness) -> &'static dyn HarnessAdapter {
    match harness {
        Harness::Codex => &CodexAdapter,
        Harness::Claude => &ClaudeAdapter,
        Harness::Pi => &PiAdapter,
    }
}

pub fn parse(harness: Harness, path: &str, bytes: &[u8]) -> Result<ParsedFile, AdapterError> {
    adapter(harness).parse(path, bytes)
}
fn parse_native(harness: Harness, path: &str, bytes: &[u8]) -> Result<ParsedFile, AdapterError> {
    let prefix = complete_prefix(bytes);
    let (records, warnings) = records(prefix);
    let mut session = match harness {
        Harness::Codex => codex::parse(&records)?,
        Harness::Claude => tree::claude(path, &records)?,
        Harness::Pi => tree::pi(&records)?,
    };
    session.warnings.extend(warnings.clone());
    resolve_calls(&mut session);
    Ok(ParsedFile {
        sessions: vec![session],
        accepted_bytes: prefix.len(),
        deferred_bytes: bytes.len() - prefix.len(),
        warnings,
    })
}

struct Record {
    value: Value,
    anchor: Anchor,
}
fn records(bytes: &[u8]) -> (Vec<Record>, Vec<String>) {
    let mut records = Vec::new();
    let mut warnings = Vec::new();
    let mut offset = 0;
    for (index, line) in bytes.split_inclusive(|b| *b == b'\n').enumerate() {
        if line.iter().all(u8::is_ascii_whitespace) {
            offset += line.len();
            continue;
        }
        match serde_json::from_slice::<Value>(line) {
            Ok(value) if value.is_object() => {
                let record_id = str_at(&value, "uuid")
                    .or_else(|| str_at(&value, "id"))
                    .or_else(|| value.get("payload").and_then(|p| str_at(p, "id")))
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("line:{}", index + 1));
                records.push(Record {
                    value,
                    anchor: Anchor {
                        record_id,
                        line: index as u64 + 1,
                        byte_offset: offset as u64,
                        byte_length: line.len() as u64,
                    },
                });
            }
            _ => {
                if warnings.len() < 32 {
                    warnings.push(format!("malformed_json_line:{}", index + 1));
                }
            }
        }
        offset += line.len();
    }
    (records, warnings)
}
fn str_at<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}
fn timestamp(v: &Value) -> Option<DateTime<Utc>> {
    v.as_str()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|t| t.with_timezone(&Utc))
        .or_else(|| v.as_i64().and_then(DateTime::from_timestamp_millis))
}
fn native_session(id: &str, cwd: Option<&str>, time: Option<DateTime<Utc>>) -> NativeSession {
    NativeSession {
        native_key: id.into(),
        native_session_id: id.into(),
        native_agent_id: None,
        recorded_cwd: cwd.map(str::to_owned),
        started_at: time,
        messages: vec![],
        events: vec![],
        relations: vec![],
        artifacts: vec![],
        warnings: vec![],
    }
}
fn message(
    record: &Record,
    role: Role,
    origin: Origin,
    parent_key: Option<String>,
    content: Vec<FullContentBlock>,
) -> NativeMessage {
    let id = record.anchor.record_id.clone();
    NativeMessage {
        key: id.clone(),
        native_ids: vec![id],
        parent_key,
        role,
        native_role: role.as_str().into(),
        origin,
        timestamp: record.value.get("timestamp").and_then(timestamp),
        content,
        provider_response_id: None,
        model: None,
        provider_usage: None,
        warnings: vec![],
        anchors: vec![record.anchor.clone()],
    }
}
fn event(record: &Record, event_type: &str) -> NativeEvent {
    NativeEvent {
        event_type: event_type.into(),
        timestamp: record.value.get("timestamp").and_then(timestamp),
        data: record.value.clone(),
        anchor: record.anchor.clone(),
    }
}
fn role(v: &str) -> Role {
    v.parse().unwrap_or(Role::Unknown)
}
fn origin(role: Role) -> Origin {
    match role {
        Role::Assistant => Origin::Model,
        Role::Tool => Origin::Tool,
        Role::User => Origin::NativeUser,
        Role::System => Origin::Harness,
        Role::Unknown => Origin::Unknown,
    }
}
fn resolve_calls(session: &mut NativeSession) {
    let mut counts = HashMap::<String, usize>::new();
    for block in session.messages.iter().flat_map(|m| &m.content) {
        if let FullContentBlock::ToolCall {
            call_id: Some(id), ..
        } = block
        {
            *counts.entry(id.clone()).or_default() += 1;
        }
    }
    for m in &mut session.messages {
        for b in &mut m.content {
            if let FullContentBlock::ToolResult { call_id, .. } = b
                && call_id.as_ref().is_none_or(|id| counts.get(id) != Some(&1))
            {
                *call_id = None;
                m.warnings.push("unresolved_tool_call".into());
            }
        }
    }
}
fn resolve_parent<'a>(
    mut id: Option<&'a str>,
    parents: &'a HashMap<String, Option<String>>,
    messages: &HashMap<String, String>,
) -> Option<String> {
    let mut seen = HashSet::new();
    while let Some(current) = id {
        if !seen.insert(current) {
            return None;
        }
        if let Some(key) = messages.get(current) {
            return Some(key.clone());
        }
        id = parents.get(current).and_then(|p| p.as_deref());
    }
    None
}
