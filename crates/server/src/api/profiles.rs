use super::*;
use crate::{config::validate_http_url, embeddings};
use sessiondb_types::{
    EmbeddingProfile, EmbeddingProfileCreate, EmbeddingProfilePatch, ProfileListOptions,
};
use sqlx::{Postgres, Transaction};
use url::Url;

pub(super) async fn list(
    State(state): State<Arc<AppState>>,
    ApiQuery(options): ApiQuery<ProfileListOptions>,
) -> ApiResult<Json<Page<EmbeddingProfile>>> {
    let scope = scope("embedding_profiles")?;
    let cursor = decode_cursor(options.cursor.as_deref(), &scope)?;
    let size = limit(options.limit)?;
    let rows = sqlx::query(
        "SELECT * FROM embedding_profiles WHERE $1::uuid IS NULL OR id > $1 ORDER BY id LIMIT $2",
    )
    .bind(cursor.map(|c| c.id))
    .bind(size as i64 + 1)
    .fetch_all(&state.pool)
    .await?;
    let data = rows
        .iter()
        .map(embeddings::profile)
        .collect::<sqlx::Result<Vec<_>>>()?;
    Ok(Json(page(data, size, scope, |p| p.id)?))
}

pub(super) async fn get(
    State(state): State<Arc<AppState>>,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<Json<EmbeddingProfile>> {
    let row = sqlx::query("SELECT * FROM embedding_profiles WHERE id=$1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let mut profile = embeddings::profile(&row)?;
    if sqlx::query_scalar::<_, bool>("SELECT to_regclass('embedding_cache') IS NOT NULL")
        .fetch_one(&state.pool)
        .await?
    {
        let (ready,pending,retrying): (i64,i64,i64) = sqlx::query_as("SELECT count(*) FILTER (WHERE embedding IS NOT NULL),count(*) FILTER (WHERE embedding IS NULL),count(*) FILTER (WHERE embedding IS NULL AND attempts>0) FROM embedding_cache WHERE profile_id=$1")
            .bind(id).fetch_one(&state.pool).await?;
        profile.progress = Some(sessiondb_types::EmbeddingProgress {
            ready_inputs: ready as u64,
            pending_inputs: pending as u64,
            retrying_inputs: retrying as u64,
        });
    }
    Ok(Json(profile))
}

fn validate(input: &EmbeddingProfileCreate) -> ApiResult<String> {
    let invalid = || {
        ApiError::bad(
            "invalid_profile",
            "Provide a valid HTTP provider URL, model identity, and supported profile settings.",
        )
    };
    let url = Url::parse(&input.base_url).map_err(|_| invalid())?;
    validate_http_url(&url).map_err(|_| invalid())?;
    if [&input.name, &input.model, &input.model_revision]
        .iter()
        .any(|v| v.trim().is_empty() || v.len() > 512 || v.chars().any(char::is_control))
        || [&input.query_prefix, &input.document_prefix]
            .iter()
            .any(|v| v.len() > 1024 || v.contains('\0'))
        || !(1..=16000).contains(&input.dimensions)
        || !(1..=256).contains(&input.batch_size)
        || !(1..=300).contains(&input.timeout_seconds)
        || input.base_url.len() > 2048
    {
        return Err(invalid());
    }
    let base = url.as_str().trim_end_matches('/').to_owned();
    if let Some(reference) = &input.credential_ref
        && (reference != "env:SESSIONDB_EMBEDDING_API_KEY"
            || embeddings::credential(&base).is_none())
    {
        return Err(ApiError::bad(
            "invalid_credential_ref",
            "Configure the embedding credential for this provider URL on the server.",
        ));
    }
    Ok(base)
}

async fn check_activation(tx: &mut Transaction<'_, Postgres>, id: Uuid) -> ApiResult<()> {
    let available: bool = sqlx::query_scalar("SELECT to_regclass('embedding_cache') IS NOT NULL")
        .fetch_one(&mut **tx)
        .await?;
    if !available {
        return Err(ApiError::conflict(
            "vector_storage_unavailable",
            "Install the PostgreSQL vector extension and run sessiondb-server migrate.",
        ));
    }
    let other: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT FROM embedding_profiles WHERE enabled AND id<>$1)",
    )
    .bind(id)
    .fetch_one(&mut **tx)
    .await?;
    if other {
        return Err(ApiError::conflict(
            "embedding_profile_conflict",
            "Disable the current profile before enabling another.",
        ));
    }
    Ok(())
}

pub(super) async fn create(
    State(state): State<Arc<AppState>>,
    ApiJson(input): ApiJson<EmbeddingProfileCreate>,
) -> ApiResult<Response> {
    let base = validate(&input)?;
    let id = Uuid::new_v4();
    let mut tx = state.pool.begin().await?;
    sqlx::query("LOCK TABLE embedding_profiles IN EXCLUSIVE MODE")
        .execute(&mut *tx)
        .await?;
    if input.enabled {
        check_activation(&mut tx, id).await?;
    }
    let row = sqlx::query("INSERT INTO embedding_profiles(id,name,base_url,model,model_revision,dimensions,document_prefix,query_prefix,batch_size,timeout_seconds,include_tools,use_credential,enabled) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13) RETURNING *")
        .bind(id).bind(input.name).bind(base).bind(input.model).bind(input.model_revision)
        .bind(input.dimensions as i32).bind(input.document_prefix).bind(input.query_prefix)
        .bind(input.batch_size as i32).bind(input.timeout_seconds as i32).bind(input.include_tools)
        .bind(input.credential_ref.is_some()).bind(input.enabled).fetch_one(&mut *tx).await?;
    if input.enabled {
        sqlx::query("SELECT sessiondb_enqueue_embeddings(NULL)")
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    let location = format!(
        "{}/v1/embedding-profiles/{id}",
        state.config.public_url.as_str().trim_end_matches('/')
    );
    Ok((
        StatusCode::CREATED,
        [(header::LOCATION, location)],
        Json(embeddings::profile(&row)?),
    )
        .into_response())
}

pub(super) async fn update(
    State(state): State<Arc<AppState>>,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(patch): ApiJson<EmbeddingProfilePatch>,
) -> ApiResult<Json<EmbeddingProfile>> {
    let mut tx = state.pool.begin().await?;
    sqlx::query("LOCK TABLE embedding_profiles IN EXCLUSIVE MODE")
        .execute(&mut *tx)
        .await?;
    let previous: bool = sqlx::query_scalar("SELECT enabled FROM embedding_profiles WHERE id=$1")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(ApiError::not_found)?;
    if patch.enabled && !previous {
        check_activation(&mut tx, id).await?;
    }
    let row = sqlx::query("UPDATE embedding_profiles SET enabled=$2 WHERE id=$1 RETURNING *")
        .bind(id)
        .bind(patch.enabled)
        .fetch_one(&mut *tx)
        .await?;
    if patch.enabled && !previous {
        sqlx::query("SELECT sessiondb_enqueue_embeddings(NULL)")
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(Json(embeddings::profile(&row)?))
}
