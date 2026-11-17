use crate::{ContentBlock, NativeRecordRef};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, fmt, str::FromStr};
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
string_enum!(ImportStatus { AwaitingUpload => "awaiting_upload", Verifying => "verifying", Queued => "queued", Processing => "processing", Ready => "ready", Partial => "partial", Failed => "failed", Expired => "expired" });
string_enum!(JobStatus { Queued => "queued", Running => "running", Succeeded => "succeeded", Partial => "partial", Failed => "failed", Cancelled => "cancelled" });
impl ImportStatus {
    pub fn terminal(self) -> bool {
        matches!(
            self,
            Self::Ready | Self::Partial | Self::Failed | Self::Expired
        )
    }
}
impl JobStatus {
    pub fn terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Partial | Self::Failed | Self::Cancelled
        )
    }
}

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
    pub max_files_per_import: usize,
    pub max_manifest_bytes: usize,
    pub max_file_bytes: u64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_files_per_import: 128,
            max_manifest_bytes: 2 * 1024 * 1024,
            max_file_bytes: 256 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceCreate {
    pub harness: Harness,
    pub device_label: String,
    pub root: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    pub id: Uuid,
    pub harness: Harness,
    pub device_label: String,
    pub root: String,
    pub paused: bool,
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
pub struct FileDescriptor {
    pub path: String,
    pub byte_length: u64,
    pub sha256: String,
    pub media_type: String,
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
#[serde(deny_unknown_fields)]
pub struct ImportManifest {
    pub source_id: Uuid,
    pub files: Vec<FileDescriptor>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadTarget {
    pub path: String,
    pub method: String,
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub expires_at: DateTime<Utc>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportFileResult {
    pub path: String,
    pub status: String,
    pub version: Option<String>,
    #[serde(default)]
    pub session_ids: Vec<Uuid>,
    #[serde(default)]
    pub warnings: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileError {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub code: String,
    pub detail: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Import {
    pub id: Uuid,
    pub source_id: Uuid,
    pub status: ImportStatus,
    pub uploads: Vec<UploadTarget>,
    pub results: Vec<ImportFileResult>,
    pub created_at: DateTime<Utc>,
    pub accepted_at: Option<DateTime<Utc>>,
    pub upload_expires_at: DateTime<Utc>,
    pub job_id: Option<Uuid>,
    #[serde(default)]
    pub errors: Vec<FileError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessingState {
    pub parse: String,
    pub keyword: String,
    pub embedding: String,
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
    pub harness: Harness,
    pub native_session_id: String,
    pub title: String,
    pub tags: Vec<String>,
    pub project_ids: Vec<Uuid>,
    pub recorded_cwd: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
    pub message_count: u64,
    pub processing: ProcessingState,
    pub relations: Vec<SessionRelation>,
    pub warnings: Vec<String>,
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
    pub revision: String,
    pub truncated: bool,
    pub content_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_response_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_usage: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
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
pub struct Job {
    pub id: Uuid,
    pub kind: String,
    pub status: JobStatus,
    pub completed_items: u64,
    pub total_items: Option<u64>,
    pub error_code: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub source_id: Option<Uuid>,
    #[serde(default)]
    pub project_ids: Vec<Uuid>,
    #[serde(default)]
    pub session_ids: Vec<Uuid>,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<FileError>,
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
