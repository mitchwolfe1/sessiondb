use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "encoding", content = "value", rename_all = "snake_case")]
pub enum ToolInput {
    Json(Value),
    Text(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    Success,
    Error,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    Image,
    File,
    Audio,
    Video,
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolOutputBlock {
    Text {
        text: String,
    },
    Json {
        value: Value,
    },
    Artifact {
        artifact_id: Uuid,
        media_type: String,
        kind: ArtifactKind,
        #[serde(skip_serializing_if = "Option::is_none")]
        alt: Option<String>,
    },
    Unknown {
        native_type: String,
        data: Value,
    },
    #[serde(untagged)]
    Future(Value),
}

/// Complete canonical content, without preview references.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FullContentBlock {
    Text {
        text: String,
    },
    Reasoning {
        text: String,
        #[serde(default)]
        redacted: bool,
    },
    ToolCall {
        call_id: Option<String>,
        name: String,
        input: ToolInput,
    },
    ToolResult {
        call_id: Option<String>,
        status: ToolStatus,
        content: Vec<ToolOutputBlock>,
    },
    Json {
        value: Value,
    },
    Artifact {
        artifact_id: Uuid,
        media_type: String,
        kind: ArtifactKind,
        #[serde(skip_serializing_if = "Option::is_none")]
        alt: Option<String>,
    },
    Unknown {
        native_type: String,
        data: Value,
    },
    #[serde(untagged)]
    Future(Value),
}

impl FullContentBlock {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }

    pub fn kind(&self) -> &str {
        match self {
            Self::Text { .. } => "text",
            Self::Reasoning { .. } => "reasoning",
            Self::ToolCall { .. } => "tool_call",
            Self::ToolResult { .. } => "tool_result",
            Self::Json { .. } => "json",
            Self::Artifact { .. } => "artifact",
            Self::Unknown { .. } => "unknown",
            Self::Future(value) => value
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("unknown"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContentReference {
    #[serde(rename = "type")]
    pub block_type: ReferenceType,
    pub content_url: String,
    pub revision: String,
    pub block_index: usize,
    pub original_type: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ReferenceType {
    #[serde(rename = "content_reference")]
    ContentReference,
}

/// The fallback preserves new response variants for forward-compatible clients.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ContentBlock {
    Reference(ContentReference),
    Full(FullContentBlock),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeRecordRef {
    pub file_id: Uuid,
    pub generation_id: Uuid,
    pub record_id: Option<String>,
    pub line: u64,
    pub byte_offset: u64,
    pub byte_length: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullContent {
    pub message_id: Uuid,
    pub revision: String,
    pub content: Vec<FullContentBlock>,
}
