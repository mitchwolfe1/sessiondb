use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmbeddingProvider {
    OpenaiCompatible,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingProfileCreate {
    pub name: String,
    pub provider: EmbeddingProvider,
    pub base_url: String,
    pub model: String,
    pub model_revision: String,
    pub dimensions: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_ref: Option<String>,
    #[serde(default)]
    pub document_prefix: String,
    #[serde(default)]
    pub query_prefix: String,
    #[serde(default = "batch_size")]
    pub batch_size: u32,
    #[serde(default = "timeout_seconds")]
    pub timeout_seconds: u32,
    #[serde(default)]
    pub include_tools: bool,
    #[serde(default)]
    pub enabled: bool,
}
fn batch_size() -> u32 {
    8
}
fn timeout_seconds() -> u32 {
    60
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingProfile {
    pub id: Uuid,
    pub name: String,
    pub provider: EmbeddingProvider,
    pub base_url: String,
    pub model: String,
    pub model_revision: String,
    pub dimensions: u32,
    pub chunking_version: u32,
    pub normalization: String,
    pub document_prefix: String,
    pub query_prefix: String,
    pub batch_size: u32,
    pub timeout_seconds: u32,
    pub include_tools: bool,
    pub enabled: bool,
    pub credential_configured: bool,
    /// Distinct provider inputs in this profile's cache. Included on a detail read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<EmbeddingProgress>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingProfilePatch {
    pub enabled: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileListOptions {
    pub limit: Option<u32>,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingProgress {
    pub ready_inputs: u64,
    pub pending_inputs: u64,
    pub retrying_inputs: u64,
}
