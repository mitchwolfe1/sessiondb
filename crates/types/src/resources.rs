use crate::{ContentBlock, NativeRecordRef};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{fmt, str::FromStr};
use uuid::Uuid;

macro_rules! string_enum {
    ($(#[$meta:meta])* $name:ident { $($(#[$variant_meta:meta])* $variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub enum $name { $($(#[$variant_meta])* #[serde(rename = $text)] $variant),+ }
        impl $name { pub const fn as_str(self) -> &'static str { match self { $(Self::$variant => $text),+ } } }
        impl fmt::Display for $name { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(self.as_str()) } }
        impl FromStr for $name { type Err = String; fn from_str(s: &str) -> Result<Self, String> { match s { $($text => Ok(Self::$variant)),+, _ => Err(format!("unsupported {}: {s}", stringify!($name))) } } }
    };
}
string_enum!(Harness { Codex => "codex", Claude => "claude", Pi => "pi" });
string_enum!(Role { User => "user", Assistant => "assistant", System => "system", Tool => "tool", Unknown => "unknown" });
string_enum!(Origin { NativeUser => "native_user", Model => "model", Tool => "tool", Harness => "harness", Unknown => "unknown" });

/// A team-chosen provenance label, canonicalized for use across devices.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct MemberHandle(String);

impl MemberHandle {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for MemberHandle {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if !(1..=64).contains(&s.len())
            || !s.as_bytes()[0].is_ascii_alphanumeric()
            || !s
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        {
            return Err(
                "use 1 to 64 ASCII letters, digits, hyphens or underscores, starting with a letter or digit",
            );
        }
        Ok(Self(s.to_ascii_lowercase()))
    }
}

impl<'de> Deserialize<'de> for MemberHandle {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

string_enum!(#[derive(Default)] SearchMode {
    #[default] Keyword => "keyword", Semantic => "semantic", Hybrid => "hybrid"
});
string_enum!(#[derive(Default)] SearchSort {
    #[default] Relevance => "relevance", Newest => "newest"
});

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page<T> {
    pub data: Vec<T>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    pub api_version: String,
    pub server_version: String,
    pub capabilities: Vec<String>,
    pub limits: Limits,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    pub max_file_bytes: u64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_file_bytes: 256 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceCreate {
    pub harness: Harness,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member: Option<MemberHandle>,
    pub device_label: String,
    pub root: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceMemberUpdate {
    pub member: MemberHandle,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_member: Option<MemberHandle>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    pub id: Uuid,
    pub member: Option<String>,
    pub harness: Harness,
    pub device_label: String,
    pub root: String,
    pub last_synced_at: Option<DateTime<Utc>>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceFile {
    pub id: Uuid,
    pub source_id: Uuid,
    pub path: String,
    pub generation_id: Uuid,
    pub byte_length: u64,
    pub sha256: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileUpload {
    pub path: String,
    pub byte_length: u64,
    pub sha256: String,
    #[serde(deserialize_with = "required_version")]
    pub expected_version: Option<String>,
    #[serde(default)]
    pub project_ids: Vec<Uuid>,
}
fn required_version<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    Option::<String>::deserialize(deserializer)
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileUploadResult {
    pub file: SourceFile,
    pub session_ids: Vec<Uuid>,
    pub unchanged: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryBoundary {
    pub rollout_id: String,
    pub end_ordinal_exclusive: u64,
    pub end_byte_offset: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRelation {
    #[serde(rename = "type")]
    pub relation_type: String,
    pub session_id: Option<Uuid>,
    pub native_reference: Option<String>,
    pub resolved: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history_boundary: Option<HistoryBoundary>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: Uuid,
    /// Older servers omit collecting source and device from their responses.
    pub source_id: Option<Uuid>,
    pub device_label: Option<String>,
    pub member: Option<String>,
    pub harness: Harness,
    pub native_session_id: String,
    pub title: String,
    pub tags: Vec<String>,
    pub project_ids: Vec<Uuid>,
    pub recorded_cwd: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
    pub message_count: u64,
    pub relations: Vec<SessionRelation>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: Uuid,
    pub session_id: Uuid,
    pub role: Role,
    pub native_role: Option<String>,
    pub origin: Origin,
    pub sequence: u64,
    pub timestamp: Option<DateTime<Utc>>,
    pub conversation_parent_id: Option<Uuid>,
    pub content: Vec<ContentBlock>,
    pub truncated: bool,
    pub content_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_response_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_usage: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_refs: Vec<NativeRecordRef>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEvent {
    pub id: Uuid,
    pub session_id: Uuid,
    pub sequence: u64,
    #[serde(rename = "type")]
    pub event_type: String,
    pub timestamp: Option<DateTime<Utc>>,
    pub native_type: String,
    pub data: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub id: Uuid,
    pub session_id: Uuid,
    pub media_type: String,
    pub kind: String,
    pub byte_length: u64,
    pub content_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Problem {
    #[serde(rename = "type")]
    pub problem_type: String,
    pub title: String,
    pub status: u16,
    pub detail: String,
    pub code: String,
    pub request_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListOptions {
    pub limit: Option<u32>,
    pub cursor: Option<String>,
    pub harness: Option<Harness>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionFilter {
    pub limit: Option<u32>,
    pub cursor: Option<String>,
    pub project_id: Option<Uuid>,
    pub unassigned: Option<bool>,
    pub harness: Option<Harness>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member: Option<MemberHandle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_label: Option<String>,
    pub tag: Option<String>,
    pub after: Option<DateTime<Utc>>,
    pub before: Option<DateTime<Utc>>,
    pub sort: Option<String>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessageFilter {
    pub limit: Option<u32>,
    pub cursor: Option<String>,
    pub leaf_id: Option<Uuid>,
}
