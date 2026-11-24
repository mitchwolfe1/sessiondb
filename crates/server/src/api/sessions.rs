use super::*;
use axum::{body::Body, http::HeaderMap};
use serde_json::json;
use sessiondb_types::{
    AroundOptions, Artifact, ContentBlock, ContentReference, FullContent, FullContentBlock,
    Message, MessageContext, MessageFilter, PREVIEW_BYTES, ReferenceType, Session, SessionFilter,
};
use sqlx::{Postgres, QueryBuilder, postgres::PgRow};
use tokio_util::io::ReaderStream;

async fn read(state: &AppState, id: Uuid) -> ApiResult<PgRow> {
    sqlx::query("SELECT * FROM sessions WHERE id=$1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(ApiError::not_found)
}
async fn session(state: &AppState, row: &PgRow) -> ApiResult<Session> {
    let id: Uuid = row.try_get("id")?;
    let project_ids = sqlx::query_scalar(
        "SELECT project_id FROM session_projects WHERE session_id=$1 ORDER BY project_id",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;
    let mut session = Session {
        id,
        harness: row
            .try_get::<String, _>("harness")?
            .parse()
            .map_err(ApiError::internal)?,
        native_session_id: row.try_get("native_session_id")?,
        title: row.try_get("title")?,
        tags: row.try_get("tags")?,
        project_ids,
        recorded_cwd: row.try_get("recorded_cwd")?,
        started_at: row.try_get("started_at")?,
        updated_at: row.try_get("updated_at")?,
        message_count: row.try_get::<i64, _>("message_count")? as u64,
        relations: row.try_get::<sqlx::types::Json<_>, _>("relations")?.0,
    };
    for relation in &mut session.relations {
        if let Some(native) = &relation.native_reference {
            relation.session_id = None;
            relation.resolved = false;
            let candidates = sqlx::query(
                "SELECT id FROM sessions WHERE harness=$1 AND native_key=$2 ORDER BY id LIMIT 2",
            )
            .bind(session.harness.as_str())
            .bind(native)
            .fetch_all(&state.pool)
            .await?;
            if candidates.len() == 1 {
                let id = candidates[0].get("id");
                relation.session_id = Some(id);
                relation.resolved = true;
            }
        }
    }
    Ok(session)
}
pub(super) async fn get(
    State(state): State<Arc<AppState>>,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<Json<Session>> {
    Ok(Json(session(&state, &read(&state, id).await?).await?))
}
pub(super) async fn list(
    State(state): State<Arc<AppState>>,
    ApiQuery(mut filter): ApiQuery<SessionFilter>,
) -> ApiResult<Json<Page<Session>>> {
    if filter.project_id.is_some() && filter.unassigned == Some(true) {
        return Err(ApiError::bad(
            "incompatible_filters",
            "project_id and unassigned cannot be combined.",
        ));
    }
    let ascending = match filter.sort.as_deref().unwrap_or("-updated_at") {
        "updated_at" => true,
        "-updated_at" => false,
        _ => {
            return Err(ApiError::bad(
                "invalid_sort",
                "Use updated_at or -updated_at.",
            ));
        }
    };
    let size = limit(filter.limit)?;
    let value = filter.cursor.take();
    filter.limit = None;
    let scope = scope(("sessions", &filter))?;
    let cursor = decode_cursor(value.as_deref(), &scope)?;
    let mut q = QueryBuilder::<Postgres>::new("SELECT s.* FROM sessions s WHERE true");
    if let Some(h) = filter.harness {
        q.push(" AND s.harness=").push_bind(h.as_str());
    }
    if let Some(project) = filter.project_id {
        q.push(" AND EXISTS(SELECT 1 FROM session_projects sp WHERE sp.session_id=s.id AND sp.project_id=").push_bind(project).push(")");
    }
    if filter.unassigned == Some(true) {
        q.push(" AND NOT EXISTS(SELECT 1 FROM session_projects sp WHERE sp.session_id=s.id)");
    }
    if let Some(tag) = &filter.tag {
        q.push(" AND ").push_bind(tag).push("=ANY(s.tags)");
    }
    if let Some(after) = filter.after {
        q.push(" AND s.updated_at>=").push_bind(after);
    }
    if let Some(before) = filter.before {
        q.push(" AND s.updated_at<").push_bind(before);
    }
    if let Some(c) = cursor {
        let time = c.time.ok_or_else(|| {
            ApiError::bad("invalid_cursor", "The cursor has no ordering position.")
        })?;
        q.push(if ascending {
            " AND (s.updated_at,s.id)>("
        } else {
            " AND (s.updated_at,s.id)<("
        })
        .push_bind(time)
        .push(",")
        .push_bind(c.id)
        .push(")");
    }
    q.push(if ascending {
        " ORDER BY s.updated_at,s.id LIMIT "
    } else {
        " ORDER BY s.updated_at DESC,s.id DESC LIMIT "
    })
    .push_bind(size as i64 + 1);
    let rows = q.build().fetch_all(&state.pool).await?;
    let more = rows.len() > size;
    let mut data = Vec::new();
    for row in rows.iter().take(size) {
        data.push(session(&state, row).await?);
    }
    let next_cursor = if more {
        data.last()
            .map(|s| {
                encode_cursor(Cursor {
                    scope,
                    id: s.id,
                    time: Some(s.updated_at),
                })
            })
            .transpose()?
    } else {
        None
    };
    Ok(Json(Page { data, next_cursor }))
}
pub(super) async fn messages(
    State(state): State<Arc<AppState>>,
    ApiPath(id): ApiPath<Uuid>,
    ApiQuery(filter): ApiQuery<MessageFilter>,
) -> ApiResult<Json<Page<Message>>> {
    read(&state, id).await?;
    let scope = scope(("messages", id, filter.leaf_id))?;
    let cursor = decode_cursor(filter.cursor.as_deref(), &scope)?;
    let size = limit(filter.limit)?;
    let mut sequence = -1;
    if let Some(c) = &cursor {
        sequence = sqlx::query_scalar::<_, i64>(
            "SELECT sequence FROM messages WHERE id=$1 AND session_id=$2",
        )
        .bind(c.id)
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| ApiError::bad("invalid_cursor", "The cursor has no ordering position."))?;
    }
    if let Some(leaf) = filter.leaf_id {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM messages WHERE id=$1 AND session_id=$2)",
        )
        .bind(leaf)
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
        if !exists {
            return Err(ApiError::not_found());
        }
    }
    let rows = if let Some(leaf) = filter.leaf_id {
        sqlx::query("WITH RECURSIVE ancestry AS (SELECT id,conversation_parent_id,ARRAY[id] AS visited FROM messages WHERE id=$1 AND session_id=$2 UNION ALL SELECT m.id,m.conversation_parent_id,a.visited||m.id FROM messages m JOIN ancestry a ON m.id=a.conversation_parent_id WHERE m.session_id=$2 AND NOT m.id=ANY(a.visited)) SELECT m.* FROM messages m JOIN ancestry a ON a.id=m.id WHERE m.sequence>$3 ORDER BY m.sequence,m.id LIMIT $4").bind(leaf).bind(id).bind(sequence).bind(size as i64+1).fetch_all(&state.pool).await?
    } else {
        sqlx::query("SELECT * FROM messages WHERE session_id=$1 AND sequence>$2 ORDER BY sequence,id LIMIT $3").bind(id).bind(sequence).bind(size as i64+1).fetch_all(&state.pool).await?
    };
    let more = rows.len() > size;
    let data = rows
        .iter()
        .take(size)
        .map(|row| message_preview(row, state.config.public_url.as_str()))
        .collect::<ApiResult<Vec<_>>>()?;
    let next_cursor = if more {
        data.last()
            .map(|m| {
                encode_cursor(Cursor {
                    scope,
                    id: m.id,
                    time: None,
                })
            })
            .transpose()?
    } else {
        None
    };
    Ok(Json(Page { data, next_cursor }))
}
async fn message_row(state: &AppState, id: Uuid) -> ApiResult<PgRow> {
    sqlx::query("SELECT * FROM messages WHERE id=$1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(ApiError::not_found)
}
pub(super) async fn message(
    State(state): State<Arc<AppState>>,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<Response> {
    let row = message_row(&state, id).await?;
    let m = message_preview(&row, state.config.public_url.as_str())?;
    Ok(Json(m).into_response())
}
pub(super) async fn around(
    State(state): State<Arc<AppState>>,
    ApiPath(id): ApiPath<Uuid>,
    ApiQuery(options): ApiQuery<AroundOptions>,
) -> ApiResult<Json<MessageContext>> {
    let before = options.before.unwrap_or(2);
    let after = options.after.unwrap_or(2);
    if before > 50 || after > 50 {
        return Err(ApiError::bad(
            "invalid_window",
            "before and after must be between 0 and 50.",
        ));
    }
    let rows = sqlx::query(include_str!("around.sql"))
        .bind(id)
        .bind(options.leaf_id)
        .bind(before as i32)
        .bind(after as i32)
        .fetch_all(&state.pool)
        .await?;
    let first = rows.first().ok_or_else(ApiError::not_found)?;
    if !first.try_get::<bool, _>("valid_branch")? {
        return Err(ApiError::bad(
            "invalid_branch",
            "leaf_id must be the anchor or a descendant in its session.",
        ));
    }
    Ok(Json(MessageContext {
        anchor_id: id,
        branch_choices: first.try_get("choices")?,
        data: rows
            .iter()
            .map(|row| bounded_preview(row, state.config.public_url.as_str(), 2048))
            .collect::<ApiResult<_>>()?,
    }))
}
fn message_preview(row: &PgRow, base: &str) -> ApiResult<Message> {
    bounded_preview(row, base, PREVIEW_BYTES)
}
fn bounded_preview(row: &PgRow, base: &str, budget: usize) -> ApiResult<Message> {
    let mut message = Message {
        id: row.try_get("id")?,
        session_id: row.try_get("session_id")?,
        role: row
            .try_get::<String, _>("role")?
            .parse()
            .map_err(ApiError::internal)?,
        native_role: row.try_get("native_role")?,
        origin: row
            .try_get::<String, _>("origin")?
            .parse()
            .map_err(ApiError::internal)?,
        sequence: row.try_get::<i64, _>("sequence")? as u64,
        timestamp: row.try_get("recorded_at")?,
        conversation_parent_id: row.try_get("conversation_parent_id")?,
        content: Vec::new(),
        truncated: false,
        content_url: None,
        provider_response_id: row.try_get("provider_response_id")?,
        model: row.try_get("model")?,
        provider_usage: row
            .try_get::<Option<sqlx::types::Json<_>>, _>("provider_usage")?
            .map(|value| value.0),
        source_refs: row.try_get::<sqlx::types::Json<_>, _>("source_refs")?.0,
    };
    let blocks: Vec<FullContentBlock> = row.try_get::<sqlx::types::Json<_>, _>("content")?.0;
    let mut used = 2;
    for (index, block) in blocks.into_iter().enumerate() {
        let separator = usize::from(!message.content.is_empty());
        let len = encoded_len(&block)?;
        if used + separator + len <= budget {
            message.content.push(ContentBlock::Full(block));
            used += separator + len;
        } else {
            message.truncated = true;
            let url = message.content_url.get_or_insert_with(|| {
                format!(
                    "{}/v1/messages/{}/content",
                    base.trim_end_matches('/'),
                    message.id
                )
            });
            let reference = ContentReference {
                block_type: ReferenceType::ContentReference,
                content_url: url.clone(),
                block_index: index,
                original_type: block.kind().into(),
            };
            let len = encoded_len(&reference)?;
            if used + separator + len > budget {
                break;
            }
            message.content.push(ContentBlock::Reference(reference));
            used += separator + len;
        }
    }
    Ok(message)
}

fn encoded_len(value: &impl Serialize) -> ApiResult<usize> {
    struct Counter(usize);
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 += bytes.len();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter(0);
    serde_json::to_writer(&mut counter, value)?;
    Ok(counter.0)
}
pub(super) async fn full_content(
    State(state): State<Arc<AppState>>,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<Response> {
    let row = message_row(&state, id).await?;
    let full = FullContent {
        message_id: id,
        content: row.try_get::<sqlx::types::Json<_>, _>("content")?.0,
    };
    Ok(Json(full).into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ExportOptions {
    format: Option<String>,
    leaf_id: Option<Uuid>,
}
pub(super) async fn export(
    State(state): State<Arc<AppState>>,
    ApiPath(id): ApiPath<Uuid>,
    ApiQuery(options): ApiQuery<ExportOptions>,
) -> ApiResult<Response> {
    if options.format.as_deref().is_some_and(|v| v != "native") || options.leaf_id.is_some() {
        return Err(ApiError::bad(
            "unsupported_option",
            "This server supports native ZIP export without leaf_id.",
        ));
    }
    let first = read(&state, id).await?;
    let mut queue = vec![session(&state, &first).await?];
    let mut visited = std::collections::HashSet::new();
    let mut files = std::collections::BTreeMap::new();
    while let Some(session) = queue.pop() {
        if !visited.insert(session.id) {
            continue;
        }
        let rows = sqlx::query("SELECT DISTINCT ON(f.id) f.id,f.source_id,f.path,g.id AS generation_id,g.object_key,o.byte_length,o.sha256 FROM session_files sf JOIN source_files f ON f.id=sf.file_id JOIN file_generations g ON g.id=sf.generation_id JOIN objects o ON o.object_key=g.object_key WHERE sf.session_id=$1 ORDER BY f.id,(g.id=f.generation_id) DESC,g.created_at DESC").bind(session.id).fetch_all(&state.pool).await?;
        for row in rows {
            files.insert(
                (
                    row.get::<Uuid, _>("id"),
                    row.get::<Uuid, _>("generation_id"),
                ),
                row,
            );
        }
        for relation in session.relations {
            if let Some(parent) = relation.session_id {
                match read(&state, parent).await {
                    Ok(row) => queue.push(self::session(&state, &row).await?),
                    Err(error) if error.status == StatusCode::NOT_FOUND => {}
                    Err(error) => return Err(error),
                }
            }
        }
    }
    let directory = tempfile::tempdir().map_err(ApiError::internal)?;
    let mut entries = Vec::new();
    let mut manifest_files = Vec::new();
    for (index, row) in files.values().enumerate() {
        let path: String = row.get("path");
        if !super::sources::valid_path(&path) {
            return Err(ApiError::internal("invalid catalog path"));
        }
        let local = directory.path().join(index.to_string());
        let mut target = tokio::fs::File::create(&local)
            .await
            .map_err(ApiError::internal)?;
        let mut body = state
            .objects
            .get(row.get("object_key"), None)
            .await
            .map_err(super::sources::storage_error)?;
        tokio::io::copy(&mut body.reader, &mut target)
            .await
            .map_err(|_| ApiError::unavailable())?;
        let source_id: Uuid = row.get("source_id");
        let generation: Uuid = row.get("generation_id");
        let archive_path = format!("sources/{source_id}/{generation}/{path}");
        manifest_files.push(json!({"file_id":row.get::<Uuid,_>("id"),"source_id":source_id,"generation_id":generation,"path":path,"archive_path":archive_path,"byte_length":row.get::<i64,_>("byte_length"),"sha256":row.get::<String,_>("sha256")}));
        entries.push((local, archive_path));
    }
    let manifest =
        serde_json::to_vec_pretty(&json!({"version":1,"session_id":id,"files":manifest_files}))?;
    let file = tokio::task::spawn_blocking(move || -> std::io::Result<std::fs::File> {
        use std::io::{Seek, Write};
        let mut archive = zip::ZipWriter::new(tempfile::tempfile()?);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        archive.start_file("manifest.json", options)?;
        archive.write_all(&manifest)?;
        for (local, name) in entries {
            archive.start_file(name, options)?;
            std::io::copy(&mut std::fs::File::open(local)?, &mut archive)?;
        }
        let mut file = archive.finish()?;
        file.rewind()?;
        drop(directory);
        Ok(file)
    })
    .await
    .map_err(ApiError::internal)?
    .map_err(ApiError::internal)?;
    let length = file.metadata().map_err(ApiError::internal)?.len();
    let mut response = Response::new(Body::from_stream(ReaderStream::new(
        tokio::fs::File::from_std(file),
    )));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        "application/zip".parse().expect("constant header"),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        format!("attachment; filename=\"session-{id}.zip\"")
            .parse()
            .expect("UUID filename"),
    );
    response
        .headers_mut()
        .insert(header::CONTENT_LENGTH, length.into());
    Ok(response)
}
async fn artifact_row(state: &AppState, id: Uuid) -> ApiResult<PgRow> {
    sqlx::query("SELECT a.*,o.byte_length FROM artifacts a JOIN objects o ON o.object_key=a.object_key WHERE a.id=$1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(ApiError::not_found)
}
pub(super) async fn artifact(
    State(state): State<Arc<AppState>>,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<Json<Artifact>> {
    let row = artifact_row(&state, id).await?;
    Ok(Json(Artifact {
        id,
        session_id: row.get("session_id"),
        media_type: row.get("media_type"),
        kind: row.get("kind"),
        byte_length: row.get::<i64, _>("byte_length") as u64,
        content_url: format!(
            "{}/v1/artifacts/{id}/content",
            state.config.public_url.as_str().trim_end_matches('/')
        ),
    }))
}
pub(super) async fn artifact_content(
    State(state): State<Arc<AppState>>,
    ApiPath(id): ApiPath<Uuid>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let row = artifact_row(&state, id).await?;
    super::sources::stream(
        &state,
        row.get("object_key"),
        row.get("media_type"),
        &headers,
    )
    .await
}
