use super::*;
use axum::http::HeaderMap;
use sessiondb_types::{Import, ImportManifest, ImportStatus, Job, JobStatus};
use sqlx::postgres::PgRow;

fn staging_key(import: Uuid, index: usize) -> String {
    format!("staging/{import}/{index}")
}
// Read the staged object locations reserved for this import.
pub(crate) async fn staging_keys(
    state: &AppState,
    import: Uuid,
) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar("SELECT object_key FROM objects WHERE import_id=$1 AND state='staging' ORDER BY substring(object_key from '[^/]+$')::integer")
        .bind(import)
        .fetch_all(&state.pool)
        .await
}
pub(crate) fn valid_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 4096
        && !value.starts_with('/')
        && !value.contains('\\')
        && !value.chars().any(char::is_control)
        && value
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != "..")
}
fn validate(manifest: &ImportManifest, state: &AppState) -> ApiResult<()> {
    let limits = &state.config.limits;
    if manifest.files.is_empty() || manifest.files.len() > limits.max_files_per_import {
        return Err(ApiError::bad(
            "invalid_manifest",
            "The manifest must contain an allowed number of files.",
        ));
    }
    let mut paths = std::collections::HashSet::new();
    for f in &manifest.files {
        if !valid_path(&f.path) || !paths.insert(&f.path) {
            return Err(ApiError::bad(
                "invalid_path",
                "File paths must be unique, relative, and contain no traversal components.",
            ));
        }
        if f.byte_length > limits.max_file_bytes {
            return Err(ApiError::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                "file_too_large",
                "A file exceeds the server's upload limit.",
            ));
        }
        if f.sha256.len() != 64
            || !f
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(ApiError::bad(
                "invalid_digest",
                "SHA-256 must contain 64 lowercase hexadecimal digits.",
            ));
        }
        if f.media_type.len() > 200
            || !f.media_type.contains('/')
            || http::HeaderValue::from_str(&f.media_type).is_err()
        {
            return Err(ApiError::bad(
                "invalid_media_type",
                "The media type is invalid.",
            ));
        }
        if f.project_ids.len() > 128 {
            return Err(ApiError::bad(
                "invalid_labels",
                "Too many initial project labels.",
            ));
        }
    }
    Ok(())
}
fn idempotency_key(headers: &HeaderMap) -> ApiResult<&str> {
    headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty() && s.len() <= 128 && s.is_ascii())
        .ok_or_else(|| {
            ApiError::bad(
                "idempotency_key_required",
                "Supply an Idempotency-Key of 1..128 ASCII characters.",
            )
        })
}
pub(super) async fn create(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    ApiJson(manifest): ApiJson<ImportManifest>,
) -> ApiResult<Response> {
    let source = super::sources::read(&state, manifest.source_id).await?;
    if source.paused {
        return Err(ApiError::conflict("source_paused", "The source is paused."));
    }
    validate(&manifest, &state)?;
    let key = idempotency_key(&headers)?;
    let hash = scope(&manifest)?;
    let mut tx = state.pool.begin().await?;
    sqlx::query("SELECT id FROM sources WHERE id=$1 FOR UPDATE")
        .bind(source.id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE imports SET create_key=NULL,complete_key=NULL WHERE source_id=$1 AND created_at<now()-interval '30 days'").bind(source.id).execute(&mut *tx).await?;
    if let Some(existing) =
        sqlx::query("SELECT id,request_hash FROM imports WHERE source_id=$1 AND create_key=$2")
            .bind(source.id)
            .bind(key)
            .fetch_optional(&mut *tx)
            .await?
    {
        if existing.get::<String, _>("request_hash") != hash {
            return Err(ApiError::conflict(
                "idempotency_conflict",
                "This key was used for a different manifest.",
            ));
        }
        let id: Uuid = existing.get("id");
        tx.commit().await?;
        return created(&state, id).await;
    }
    for f in &manifest.files {
        let version: Option<String> =
            sqlx::query_scalar("SELECT version FROM source_files WHERE source_id=$1 AND path=$2")
                .bind(source.id)
                .bind(&f.path)
                .fetch_optional(&mut *tx)
                .await?;
        if version != f.expected_version {
            return Err(ApiError::conflict(
                "version_conflict",
                "Refresh the source file versions before creating a new manifest.",
            ));
        }
    }
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO imports(id,source_id,manifest,request_hash,create_key) VALUES($1,$2,$3,$4,$5)",
    )
    .bind(id)
    .bind(source.id)
    .bind(sqlx::types::Json(&manifest))
    .bind(hash)
    .bind(key)
    .execute(&mut *tx)
    .await?;
    for (i, _) in manifest.files.iter().enumerate() {
        sqlx::query("INSERT INTO objects(object_key,import_id,state) VALUES($1,$2,'staging')")
            .bind(staging_key(id, i))
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    created(&state, id).await
}
async fn created(state: &AppState, id: Uuid) -> ApiResult<Response> {
    let import = read(state, id).await?;
    Ok((
        StatusCode::CREATED,
        [(
            header::LOCATION,
            format!(
                "{}/v1/imports/{id}",
                state.config.public_url.as_str().trim_end_matches('/')
            ),
        )],
        Json(import),
    )
        .into_response())
}
pub(super) async fn get(
    State(state): State<Arc<AppState>>,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<Json<Import>> {
    Ok(Json(read(&state, id).await?))
}
pub(crate) async fn read(state: &AppState, id: Uuid) -> ApiResult<Import> {
    sqlx::query("UPDATE imports SET status='expired',closed_at=now() WHERE id=$1 AND status='awaiting_upload' AND upload_expires_at<=now()").bind(id).execute(&state.pool).await?;
    let row = sqlx::query("SELECT * FROM imports WHERE id=$1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let mut value = import(&row)?;
    if value.status == ImportStatus::AwaitingUpload {
        let manifest: sqlx::types::Json<ImportManifest> = row.get("manifest");
        let ttl = (value.upload_expires_at - Utc::now()).num_seconds().max(1) as u64;
        let keys = staging_keys(state, id).await?;
        for (i, f) in manifest.files.iter().enumerate() {
            value.uploads.push(
                state
                    .objects
                    .sign_upload(
                        keys.get(i).ok_or_else(ApiError::unavailable)?,
                        f,
                        ttl.min(state.config.objects.upload_url_ttl_seconds),
                    )
                    .await
                    .map_err(super::sources::storage_error)?,
            );
        }
    }
    Ok(value)
}
fn import(row: &PgRow) -> ApiResult<Import> {
    Ok(Import {
        id: row.get("id"),
        source_id: row.get("source_id"),
        status: row
            .get::<String, _>("status")
            .parse()
            .map_err(ApiError::internal)?,
        uploads: vec![],
        results: row.get::<sqlx::types::Json<_>, _>("results").0,
        created_at: row.get("created_at"),
        accepted_at: row.get("accepted_at"),
        upload_expires_at: row.get("upload_expires_at"),
        job_id: row.get("job_id"),
        errors: row.get::<sqlx::types::Json<_>, _>("errors").0,
    })
}
pub(super) async fn complete(
    State(state): State<Arc<AppState>>,
    ApiPath(id): ApiPath<Uuid>,
    headers: HeaderMap,
    ApiQuery(params): ApiQuery<BTreeMap<String, String>>,
) -> ApiResult<Response> {
    no_options(&params)?;
    let key = idempotency_key(&headers)?;
    let mut tx = state.pool.begin().await?;
    let source_id: Uuid = sqlx::query_scalar("SELECT source_id FROM imports WHERE id=$1")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(ApiError::not_found)?;
    sqlx::query("SELECT id FROM sources WHERE id=$1 FOR UPDATE")
        .bind(source_id)
        .execute(&mut *tx)
        .await?;
    let row = sqlx::query("SELECT * FROM imports WHERE id=$1 FOR UPDATE")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    if let Some(other) = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM imports WHERE source_id=$1 AND complete_key=$2",
    )
    .bind(source_id)
    .bind(key)
    .fetch_optional(&mut *tx)
    .await?
        && other != id
    {
        return Err(ApiError::conflict(
            "idempotency_conflict",
            "This key was used to complete another import.",
        ));
    }
    let status = row.get::<String, _>("status");
    if status == "expired"
        || (status == "awaiting_upload"
            && row.get::<DateTime<Utc>, _>("upload_expires_at") <= Utc::now())
    {
        return Err(ApiError::conflict(
            "import_expired",
            "Create a new manifest; this upload lease expired.",
        ));
    }
    if status == "awaiting_upload" {
        let job_id = Uuid::new_v4();
        let manifest: sqlx::types::Json<ImportManifest> = row.get("manifest");
        sqlx::query("INSERT INTO jobs(id,source_id,import_id,kind,total_items) VALUES($1,$2,$3,'verify_import',$4)").bind(job_id).bind(source_id).bind(id).bind(manifest.files.len() as i64).execute(&mut *tx).await?;
        sqlx::query("UPDATE imports SET status='verifying',job_id=$2,complete_key=$3 WHERE id=$1")
            .bind(id)
            .bind(job_id)
            .bind(key)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok((StatusCode::ACCEPTED, Json(read(&state, id).await?)).into_response())
}
pub(super) async fn job(
    State(state): State<Arc<AppState>>,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<Json<Job>> {
    let row = sqlx::query("SELECT * FROM jobs WHERE id=$1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(Job {
        id,
        kind: row.get("kind"),
        status: row
            .get::<String, _>("status")
            .parse::<JobStatus>()
            .map_err(ApiError::internal)?,
        completed_items: row.get::<i64, _>("completed_items") as u64,
        total_items: row.get::<Option<i64>, _>("total_items").map(|v| v as u64),
        error_code: row.get("error_code"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        source_id: Some(row.get("source_id")),
        project_ids: vec![],
        session_ids: vec![],
    }))
}
