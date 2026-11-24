use crate::{
    AppState,
    api::{ApiPath, sources},
    error::{ApiError, ApiResult},
    native_json::NativeJson,
};
use axum::{
    Json,
    extract::{Multipart, State, multipart::MultipartError},
    http::{HeaderMap, StatusCode, header},
};
use chrono::Utc;
use sessiondb_adapters::{NativeSession, parse};
use sessiondb_types::{
    ArtifactKind, FileUpload, FileUploadResult, FullContentBlock, Harness, NativeRecordRef,
    SessionRelation, ToolOutputBlock,
};
use sha2::{Digest, Sha256};
use sqlx::{Postgres, Row, Transaction, postgres::PgRow};
use std::{collections::HashMap, sync::Arc};
use uuid::Uuid;

const MAX_METADATA_BYTES: usize = 64 * 1024;
const FILE_SELECT: &str = "SELECT f.*,o.byte_length,o.sha256 FROM source_files f JOIN file_generations g ON g.id=f.generation_id JOIN objects o ON o.object_key=g.object_key WHERE f.source_id=$1 AND f.path=$2";

/// Accept one verified native snapshot, including its parsed projection, before replying.
pub async fn upload(
    State(state): State<Arc<AppState>>,
    ApiPath(source): ApiPath<Uuid>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> ApiResult<Json<FileUploadResult>> {
    if headers
        .get(header::CONTENT_TYPE)
        .is_some_and(|value| value.as_bytes().len() > 1024)
    {
        return Err(invalid_multipart());
    }
    let metadata = {
        let mut field = multipart
            .next_field()
            .await
            .map_err(multipart_error)?
            .ok_or_else(invalid_multipart)?;
        validate_part_headers(field.headers())?;
        if field.name() != Some("metadata") {
            return Err(invalid_multipart());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = field.chunk().await.map_err(multipart_error)? {
            if chunk.len() > MAX_METADATA_BYTES - bytes.len() {
                return Err(ApiError::new(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "metadata_too_large",
                    "File metadata must not exceed 64 KiB.",
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice::<FileUpload>(&bytes).map_err(|_| {
            ApiError::bad(
                "invalid_metadata",
                "The metadata part must contain valid FileUpload JSON.",
            )
        })?
    };
    validate(&metadata, state.config.limits.max_file_bytes)?;
    let harness: String = sqlx::query_scalar("SELECT harness FROM sources WHERE id=$1")
        .bind(source)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let harness: Harness = harness.parse().map_err(ApiError::internal)?;
    let bytes = {
        let mut field = multipart
            .next_field()
            .await
            .map_err(multipart_error)?
            .ok_or_else(invalid_multipart)?;
        validate_part_headers(field.headers())?;
        if field.name() != Some("file") {
            return Err(invalid_multipart());
        }
        let mut bytes = Vec::with_capacity(metadata.byte_length as usize);
        let mut digest = Sha256::new();
        while let Some(chunk) = field.chunk().await.map_err(multipart_error)? {
            if chunk.len() as u64 > metadata.byte_length - bytes.len() as u64 {
                return Err(length_mismatch());
            }
            digest.update(&chunk);
            bytes.extend_from_slice(&chunk);
        }
        if bytes.len() as u64 != metadata.byte_length {
            return Err(length_mismatch());
        }
        if hex::encode(digest.finalize()) != metadata.sha256 {
            return Err(ApiError::bad(
                "checksum_mismatch",
                "The uploaded bytes do not match the declared SHA-256.",
            ));
        }
        bytes
    };
    if multipart
        .next_field()
        .await
        .map_err(multipart_error)?
        .is_some()
    {
        return Err(invalid_multipart());
    }

    let prior = sqlx::query(FILE_SELECT)
        .bind(source)
        .bind(&metadata.path)
        .fetch_optional(&state.pool)
        .await?;
    if prior.as_ref().is_some_and(|row| same_bytes(row, &metadata)) {
        let mut tx = state.pool.begin().await?;
        lock_source(&mut tx, source).await?;
        let current = current(&mut tx, source, &metadata.path).await?;
        let row = current
            .as_ref()
            .filter(|row| same_bytes(row, &metadata))
            .ok_or_else(version_conflict)?;
        let result = receipt(&mut tx, row, true).await?;
        sqlx::query("UPDATE sources SET last_synced_at=now() WHERE id=$1")
            .bind(source)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        return Ok(Json(result));
    }
    check_version(prior.as_ref(), &metadata)?;

    // Parsing and immutable object I/O happen before taking catalog publication locks.
    let (metadata, bytes, parsed, append) = tokio::task::spawn_blocking(move || {
        let append = prior.as_ref().is_some_and(|row| {
            let length = row.get::<i64, _>("byte_length") as usize;
            length < bytes.len()
                && hex::encode(Sha256::digest(&bytes[..length])) == row.get::<&str, _>("sha256")
        });
        let parsed = parse(harness, &metadata.path, &bytes);
        (metadata, bytes, parsed, append)
    })
    .await
    .map_err(ApiError::internal)?;
    let sessions = parsed
        .map_err(|error| ApiError::bad("unsupported_native_record", error.to_string()))?
        .sessions;
    let original = store(&state, bytes, &metadata.sha256, "application/x-ndjson").await?;
    let mut prepared = Vec::with_capacity(sessions.len());
    for mut native in sessions {
        let mut artifacts = Vec::with_capacity(native.artifacts.len());
        for artifact in std::mem::take(&mut native.artifacts) {
            let digest = hex::encode(Sha256::digest(&artifact.bytes));
            let key = store(&state, artifact.bytes, &digest, &artifact.media_type).await?;
            artifacts.push(StoredArtifact {
                native_id: artifact.id,
                key,
                media_type: artifact.media_type,
                kind: artifact.kind,
            });
        }
        prepared.push(PreparedSession { native, artifacts });
    }
    prepared.sort_unstable_by(|a, b| a.native.native_key.cmp(&b.native.native_key));

    let mut tx = state.pool.begin().await?;
    lock_source(&mut tx, source).await?;
    let current = current(&mut tx, source, &metadata.path).await?;
    // A lost response or an overlapping retry must not turn acceptance into a conflict.
    if let Some(row) = current.as_ref().filter(|row| same_bytes(row, &metadata)) {
        let result = receipt(&mut tx, row, true).await?;
        sqlx::query("UPDATE sources SET last_synced_at=now() WHERE id=$1")
            .bind(source)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        return Ok(Json(result));
    }
    check_version(current.as_ref(), &metadata)?;
    // Order native-key locks consistently so duplicate copies across sources coalesce safely.
    for session in &prepared {
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(format!("{}:{}", harness, session.native.native_key))
            .execute(&mut *tx)
            .await?;
    }
    let file = current
        .as_ref()
        .map_or_else(Uuid::new_v4, |row| row.get("id"));
    let generation = if append {
        current
            .as_ref()
            .ok_or_else(version_conflict)?
            .get("generation_id")
    } else {
        Uuid::new_v4()
    };
    let version = Uuid::new_v4().to_string();
    let unavailable: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM unnest($1::uuid[]) p(id) WHERE NOT EXISTS(SELECT 1 FROM projects WHERE id=p.id))")
        .bind(&metadata.project_ids).fetch_one(&mut *tx).await?;
    if unavailable {
        return Err(ApiError::bad(
            "unknown_project",
            "The file references a project that does not exist.",
        ));
    }
    sqlx::query("INSERT INTO source_files(id,source_id,path,generation_id,version) VALUES($1,$2,$3,$4,$5) ON CONFLICT(source_id,path) DO UPDATE SET generation_id=EXCLUDED.generation_id,version=EXCLUDED.version,updated_at=now()")
        .bind(file).bind(source).bind(&metadata.path).bind(generation).bind(&version)
        .execute(&mut *tx).await?;
    sqlx::query("INSERT INTO file_generations(id,file_id,object_key) VALUES($1,$2,$3) ON CONFLICT(id) DO UPDATE SET object_key=EXCLUDED.object_key")
        .bind(generation).bind(file).bind(&original).execute(&mut *tx).await?;
    for session in prepared {
        publish(
            &mut tx, source, file, generation, harness, &metadata, session,
        )
        .await?;
    }
    sqlx::query("UPDATE sources SET last_synced_at=now() WHERE id=$1")
        .bind(source)
        .execute(&mut *tx)
        .await?;
    let row = sqlx::query(FILE_SELECT)
        .bind(source)
        .bind(&metadata.path)
        .fetch_one(&mut *tx)
        .await?;
    let result = receipt(&mut tx, &row, false).await?;
    tx.commit().await?;
    Ok(Json(result))
}

fn validate_part_headers(headers: &HeaderMap) -> ApiResult<()> {
    let length: usize = headers
        .iter()
        .map(|(name, value)| name.as_str().len() + value.as_bytes().len() + 4)
        .sum();
    if length > 16 * 1024 {
        return Err(ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "multipart_headers_too_large",
            "Each part's headers must not exceed 16 KiB.",
        ));
    }
    Ok(())
}
fn multipart_error(error: MultipartError) -> ApiError {
    ApiError::new(error.status(), "invalid_multipart", error.body_text())
}
fn invalid_multipart() -> ApiError {
    ApiError::bad(
        "invalid_multipart",
        "Supply exactly two parts in order: metadata, then file.",
    )
}
fn length_mismatch() -> ApiError {
    ApiError::bad(
        "length_mismatch",
        "The uploaded bytes do not match the declared byte length.",
    )
}
fn version_conflict() -> ApiError {
    ApiError::conflict(
        "version_conflict",
        "Refresh the current source file version before replacing its bytes.",
    )
}
fn validate(metadata: &FileUpload, max_file_bytes: u64) -> ApiResult<()> {
    if !sources::valid_path(&metadata.path) {
        return Err(ApiError::bad(
            "invalid_path",
            "File paths must be relative and contain no traversal components.",
        ));
    }
    if metadata.byte_length > max_file_bytes {
        return Err(ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "file_too_large",
            "The file exceeds the server's upload limit.",
        ));
    }
    if metadata.sha256.len() != 64
        || !metadata
            .sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(ApiError::bad(
            "invalid_digest",
            "SHA-256 must contain 64 lowercase hexadecimal digits.",
        ));
    }
    if metadata
        .expected_version
        .as_ref()
        .is_some_and(|v| v.is_empty() || v.len() > 128 || !v.is_ascii())
    {
        return Err(ApiError::bad(
            "invalid_version",
            "The expected version must be null or a bounded version token.",
        ));
    }
    if metadata.project_ids.len() > 128 {
        return Err(ApiError::bad(
            "invalid_labels",
            "Too many initial project labels.",
        ));
    }
    Ok(())
}
fn same_bytes(row: &PgRow, metadata: &FileUpload) -> bool {
    row.get::<i64, _>("byte_length") as u64 == metadata.byte_length
        && row.get::<&str, _>("sha256") == metadata.sha256
}
fn check_version(row: Option<&PgRow>, metadata: &FileUpload) -> ApiResult<()> {
    if row.map(|row| row.get::<&str, _>("version")) != metadata.expected_version.as_deref() {
        return Err(version_conflict());
    }
    Ok(())
}
async fn lock_source(tx: &mut Transaction<'_, Postgres>, source: Uuid) -> ApiResult<()> {
    sqlx::query("SELECT id FROM sources WHERE id=$1 FOR UPDATE")
        .bind(source)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok(())
}
async fn current(
    tx: &mut Transaction<'_, Postgres>,
    source: Uuid,
    path: &str,
) -> ApiResult<Option<PgRow>> {
    Ok(sqlx::query(FILE_SELECT)
        .bind(source)
        .bind(path)
        .fetch_optional(&mut **tx)
        .await?)
}
async fn receipt(
    tx: &mut Transaction<'_, Postgres>,
    row: &PgRow,
    unchanged: bool,
) -> ApiResult<FileUploadResult> {
    let file = sources::file(row)?;
    let session_ids = sqlx::query_scalar("SELECT session_id FROM session_files WHERE file_id=$1 AND generation_id=$2 ORDER BY session_id")
        .bind(file.id).bind(file.generation_id).fetch_all(&mut **tx).await?;
    Ok(FileUploadResult {
        file,
        session_ids,
        unchanged,
    })
}
async fn store(
    state: &AppState,
    bytes: Vec<u8>,
    sha256: &str,
    media_type: &str,
) -> ApiResult<String> {
    let key = format!("objects/{}", Uuid::new_v4());
    sqlx::query("INSERT INTO objects(object_key,sha256,byte_length) VALUES($1,$2,$3)")
        .bind(&key)
        .bind(sha256)
        .bind(bytes.len() as i64)
        .execute(&state.pool)
        .await?;
    state
        .objects
        .put(&key, bytes, media_type, sha256)
        .await
        .map_err(sources::storage_error)?;
    Ok(key)
}

struct StoredArtifact {
    native_id: Uuid,
    key: String,
    media_type: String,
    kind: ArtifactKind,
}
struct PreparedSession {
    native: NativeSession,
    artifacts: Vec<StoredArtifact>,
}

async fn publish(
    tx: &mut Transaction<'_, Postgres>,
    source: Uuid,
    file: Uuid,
    generation: Uuid,
    harness: Harness,
    metadata: &FileUpload,
    prepared: PreparedSession,
) -> ApiResult<()> {
    let PreparedSession {
        mut native,
        artifacts,
    } = prepared;
    let prior = sqlx::query("SELECT s.* FROM sessions s JOIN session_files sf ON sf.session_id=s.id WHERE sf.file_id=$1 AND s.native_key=$3 ORDER BY (sf.generation_id=$2) DESC,s.updated_at DESC,s.id LIMIT 1 FOR UPDATE OF s")
        .bind(file).bind(generation).bind(&native.native_key).fetch_optional(&mut **tx).await?;
    let shared = if let Some(row) = &prior {
        sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM session_files WHERE session_id=$1 AND file_id<>$2)",
        )
        .bind(row.get::<Uuid, _>("id"))
        .bind(file)
        .fetch_one(&mut **tx)
        .await?
    } else {
        false
    };
    if prior.is_none() || shared {
        let duplicate = sqlx::query("SELECT DISTINCT s.id FROM sessions s JOIN session_files sf ON sf.session_id=s.id JOIN source_files f ON f.id=sf.file_id AND f.generation_id=sf.generation_id JOIN file_generations g ON g.id=sf.generation_id JOIN objects o ON o.object_key=g.object_key WHERE s.harness=$1 AND s.native_key=$2 AND o.sha256=$3 AND o.byte_length=$4 AND sf.file_id<>$5 ORDER BY s.id LIMIT 2")
            .bind(harness.as_str()).bind(&native.native_key).bind(&metadata.sha256).bind(metadata.byte_length as i64)
            .bind(file).fetch_all(&mut **tx).await?;
        if duplicate.len() == 1 {
            let id: Uuid = duplicate[0].get("id");
            if let Some(row) = &prior {
                sqlx::query("DELETE FROM session_files WHERE session_id=$1 AND file_id=$2 AND generation_id=$3")
                    .bind(row.get::<Uuid, _>("id")).bind(file).bind(generation).execute(&mut **tx).await?;
            }
            sqlx::query("INSERT INTO session_files(session_id,file_id,generation_id) VALUES($1,$2,$3) ON CONFLICT DO NOTHING")
                .bind(id).bind(file).bind(generation).execute(&mut **tx).await?;
            return Ok(());
        }
    }
    let id = if !shared {
        prior
            .as_ref()
            .map_or_else(Uuid::new_v4, |row| row.get("id"))
    } else {
        let prior_id: Uuid = prior.as_ref().expect("shared session exists").get("id");
        sqlx::query(
            "DELETE FROM session_files WHERE session_id=$1 AND file_id=$2 AND generation_id=$3",
        )
        .bind(prior_id)
        .bind(file)
        .bind(generation)
        .execute(&mut **tx)
        .await?;
        Uuid::new_v4()
    };
    let artifact_ids: HashMap<_, _> = artifacts
        .iter()
        .map(|a| (a.native_id, Uuid::new_v5(&id, a.native_id.as_bytes())))
        .collect();
    for message in &mut native.messages {
        remap_artifacts(&mut message.content, &artifact_ids);
    }
    let title = prior
        .as_ref()
        .map_or_else(|| native.title(), |row| row.get("title"));
    let tags: Vec<String> = prior.as_ref().map_or_else(Vec::new, |row| row.get("tags"));
    let updated_at = native
        .messages
        .iter()
        .filter_map(|message| message.timestamp)
        .max()
        .or(native.started_at)
        .unwrap_or_else(Utc::now);
    let relations: Vec<_> = native
        .relations
        .into_iter()
        .map(|relation| SessionRelation {
            relation_type: relation.relation_type,
            session_id: None,
            native_reference: Some(relation.native_reference),
            resolved: false,
            history_boundary: relation.history_boundary,
        })
        .collect();
    sqlx::query("INSERT INTO sessions(id,source_id,native_key,harness,native_session_id,title,tags,recorded_cwd,started_at,updated_at,message_count,relations) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12) ON CONFLICT(id) DO UPDATE SET recorded_cwd=EXCLUDED.recorded_cwd,started_at=EXCLUDED.started_at,updated_at=EXCLUDED.updated_at,message_count=EXCLUDED.message_count,relations=EXCLUDED.relations")
        .bind(id).bind(source).bind(&native.native_key).bind(harness.as_str()).bind(&native.native_session_id)
        .bind(title).bind(tags).bind(native.recorded_cwd).bind(native.started_at).bind(updated_at)
        .bind(native.messages.len() as i64).bind(sqlx::types::Json(relations))
        .execute(&mut **tx).await?;
    if let Some(prior) = &prior {
        if shared {
            sqlx::query("INSERT INTO session_projects(session_id,project_id) SELECT $1,project_id FROM session_projects WHERE session_id=$2 ON CONFLICT DO NOTHING")
                .bind(id).bind(prior.get::<Uuid, _>("id"))
                .execute(&mut **tx).await?;
        }
    } else {
        sqlx::query("INSERT INTO session_projects(session_id,project_id) SELECT $1,id FROM projects WHERE id=ANY($2) ON CONFLICT DO NOTHING")
            .bind(id).bind(&metadata.project_ids).execute(&mut **tx).await?;
    }
    sqlx::query("INSERT INTO session_files(session_id,file_id,generation_id) VALUES($1,$2,$3) ON CONFLICT DO NOTHING")
        .bind(id).bind(file).bind(generation).execute(&mut **tx).await?;
    sqlx::query("DELETE FROM messages WHERE session_id=$1")
        .bind(id)
        .execute(&mut **tx)
        .await?;
    sqlx::query("DELETE FROM session_events WHERE session_id=$1")
        .bind(id)
        .execute(&mut **tx)
        .await?;
    sqlx::query("DELETE FROM artifacts WHERE session_id=$1")
        .bind(id)
        .execute(&mut **tx)
        .await?;
    let message_ids: HashMap<_, _> = native
        .messages
        .iter()
        .map(|message| {
            (
                message.key.as_str(),
                Uuid::new_v5(&id, message.key.as_bytes()),
            )
        })
        .collect();
    for (sequence, message) in native.messages.iter().enumerate() {
        let message_id = message_ids[message.key.as_str()];
        let parent = message
            .parent_key
            .as_ref()
            .and_then(|key| message_ids.get(key.as_str()))
            .copied();
        let anchors: Vec<_> = message
            .anchors
            .iter()
            .map(|anchor| NativeRecordRef {
                file_id: file,
                generation_id: generation,
                record_id: Some(anchor.record_id.clone()),
                line: anchor.line,
                byte_offset: anchor.byte_offset,
                byte_length: anchor.byte_length,
            })
            .collect();
        sqlx::query("INSERT INTO messages(id,session_id,sequence,role,native_role,origin,recorded_at,conversation_parent_id,content,provider_response_id,model,provider_usage,source_refs) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)")
            .bind(message_id).bind(id).bind(sequence as i64).bind(message.role.as_str())
            .bind(&message.native_role).bind(message.origin.as_str()).bind(message.timestamp).bind(parent)
            .bind(NativeJson(&message.content)).bind(&message.provider_response_id)
            .bind(&message.model).bind(message.provider_usage.as_ref().map(NativeJson))
            .bind(sqlx::types::Json(anchors))
            .execute(&mut **tx).await?;
    }
    sqlx::query("SELECT sessiondb_index_session($1)")
        .bind(id)
        .execute(&mut **tx)
        .await?;
    for (sequence, event) in native.events.into_iter().enumerate() {
        let event_id = Uuid::new_v5(
            &id,
            format!(
                "event:{}:{}:{}",
                event.anchor.record_id, event.event_type, event.anchor.line
            )
            .as_bytes(),
        );
        sqlx::query("INSERT INTO session_events(id,session_id,sequence,type,recorded_at,native_type,data) VALUES($1,$2,$3,$4,$5,$4,$6)")
            .bind(event_id).bind(id).bind(sequence as i64).bind(event.event_type)
            .bind(event.timestamp).bind(NativeJson(event.data)).execute(&mut **tx).await?;
    }
    for artifact in artifacts {
        let kind = match artifact.kind {
            ArtifactKind::Image => "image",
            ArtifactKind::File => "file",
            ArtifactKind::Audio => "audio",
            ArtifactKind::Video => "video",
            ArtifactKind::Other => "other",
        };
        sqlx::query("INSERT INTO artifacts(id,session_id,object_key,media_type,kind) VALUES($1,$2,$3,$4,$5)")
            .bind(artifact_ids[&artifact.native_id]).bind(id).bind(artifact.key)
            .bind(artifact.media_type).bind(kind).execute(&mut **tx).await?;
    }
    Ok(())
}

fn remap_artifacts(blocks: &mut [FullContentBlock], ids: &HashMap<Uuid, Uuid>) {
    for block in blocks {
        match block {
            FullContentBlock::Artifact { artifact_id, .. } => {
                if let Some(id) = ids.get(artifact_id) {
                    *artifact_id = *id;
                }
            }
            FullContentBlock::ToolResult { content, .. } => {
                for block in content {
                    if let ToolOutputBlock::Artifact { artifact_id, .. } = block
                        && let Some(id) = ids.get(artifact_id)
                    {
                        *artifact_id = *id;
                    }
                }
            }
            _ => {}
        }
    }
}
