use crate::{Harness, MemberHandle, Message, Origin, Role, SearchMode, SearchSort};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SearchFilters {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member: Option<MemberHandle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_label: Option<String>,
    pub project_ids: Vec<Uuid>,
    pub unassigned: bool,
    pub harnesses: Vec<Harness>,
    pub roles: Vec<Role>,
    pub tags: Vec<String>,
    pub include_tools: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchRequest {
    pub query: String,
    #[serde(default)]
    pub mode: SearchMode,
    #[serde(default)]
    pub filters: SearchFilters,
    #[serde(default)]
    pub sort: SearchSort,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchMatch {
    pub session_id: Uuid,
    pub source_id: Option<Uuid>,
    pub member: Option<String>,
    pub device_label: Option<String>,
    pub recorded_cwd: Option<String>,
    pub message_id: Uuid,
    pub leaf_id: Uuid,
    pub block_index: u32,
    pub block_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    pub project_ids: Vec<Uuid>,
    pub title: String,
    pub harness: Harness,
    pub role: Role,
    pub origin: Origin,
    pub timestamp: Option<DateTime<Utc>>,
    pub excerpt: String,
    pub retrieved_by: Vec<SearchMode>,
    pub url: String,
    /// Open this source with two neighboring messages on either side.
    pub show_command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchCoverage {
    pub keyword_ready_messages: u64,
    pub embedding_eligible_passages: u64,
    pub embedded_passages: u64,
    pub embedding_profile_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResponse {
    pub data: Vec<SearchMatch>,
    pub next_cursor: Option<String>,
    pub mode: SearchMode,
    pub coverage: SearchCoverage,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AroundOptions {
    pub before: Option<u32>,
    pub after: Option<u32>,
    pub leaf_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageContext {
    pub anchor_id: Uuid,
    pub data: Vec<Message>,
    pub branch_choices: Vec<Uuid>,
}
