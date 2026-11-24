mod provider;
mod worker;

pub(crate) use provider::{EmbeddingClient, credential};
use sessiondb_types::{EmbeddingProfile, EmbeddingProvider};
use sqlx::{PgPool, Row, postgres::PgRow};
pub(crate) use worker::run;

pub(crate) fn profile(row: &PgRow) -> sqlx::Result<EmbeddingProfile> {
    Ok(EmbeddingProfile {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        provider: EmbeddingProvider::OpenaiCompatible,
        base_url: row.try_get("base_url")?,
        model: row.try_get("model")?,
        model_revision: row.try_get("model_revision")?,
        dimensions: row.try_get::<i32, _>("dimensions")? as u32,
        chunking_version: row.try_get::<i32, _>("chunking_version")? as u32,
        normalization: row.try_get("normalization")?,
        document_prefix: row.try_get("document_prefix")?,
        query_prefix: row.try_get("query_prefix")?,
        batch_size: row.try_get::<i32, _>("batch_size")? as u32,
        timeout_seconds: row.try_get::<i32, _>("timeout_seconds")? as u32,
        include_tools: row.try_get("include_tools")?,
        enabled: row.try_get("enabled")?,
        credential_configured: row.try_get("use_credential")?,
        progress: None,
    })
}

pub(crate) async fn enabled(pool: &PgPool) -> sqlx::Result<Option<EmbeddingProfile>> {
    sqlx::query("SELECT * FROM embedding_profiles WHERE enabled")
        .fetch_optional(pool)
        .await?
        .as_ref()
        .map(profile)
        .transpose()
}
