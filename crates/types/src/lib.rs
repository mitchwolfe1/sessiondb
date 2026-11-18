//! Public, transport-independent types for the SessionDB v1 API.

mod content;
mod resources;

pub use content::*;
pub use resources::*;

/// Maximum encoded content returned in a message preview.
pub const PREVIEW_BYTES: usize = 64 * 1024;

/// API operations supported by the first implementation.
pub const FIRST_BUILD_OPERATIONS: &[&str] = &[
    "getLiveness",
    "getReadiness",
    "getServerInfo",
    "listSources",
    "createSource",
    "getSource",
    "listSourceFiles",
    "getOriginalContent",
    "uploadSourceFile",
    "listSessions",
    "getSession",
    "listSessionMessages",
    "getMessage",
    "getMessageContent",
    "exportSession",
];
