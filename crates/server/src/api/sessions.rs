use super::*;
use crate::auth::{Principal, is_admin, organization_role};
use axum::{Extension, body::Body, http::HeaderMap};
use serde_json::json;
use sessiondb_types::{Artifact, FullContent, Message, MessageFilter, Session, SessionFilter};
use sqlx::{Postgres, QueryBuilder, postgres::PgRow};
use tokio_util::io::ReaderStream;

pub(crate) async fn readable(
    state: &AppState,
    org: Uuid,
    id: Uuid,
    p: &Principal,
) -> ApiResult<PgRow> {
    let admin = is_admin(organization_role(&state.pool, org, p).await?);
    sqlx::query("SELECT * FROM sessions WHERE id=$1 AND organization_id=$2 AND (owner_id=$3 OR visibility='organization' OR $4)").bind(id).bind(org).bind(p.user_id()?).bind(admin).fetch_optional(&state.pool).await?.ok_or_else(ApiError::not_found)
}
async fn document(state: &AppState, row: &PgRow, p: &Principal) -> ApiResult<Session> {
    let mut session: Session = row.get::<sqlx::types::Json<Session>, _>("document").0;
    for relation in &mut session.relations {
        if let Some(native) = &relation.native_reference {
            let candidates = sqlx::query(
                "SELECT id FROM sessions WHERE organization_id=$1 AND owner_id=$2 AND harness=$3 AND native_key=$4 ORDER BY id LIMIT 2",
            )
            .bind(session.organization_id)
            .bind(session.owner_id)
            .bind(session.harness.as_str())
            .bind(native)
            .fetch_all(&state.pool)
            .await?;
            if candidates.len() == 1 {
                let id = candidates[0].get("id");
                if readable(state, session.organization_id, id, p)
                    .await
                    .is_ok()
                {
                    relation.session_id = Some(id);
                    relation.resolved = true;
                }
            }
        }
    }
    Ok(session)
}
pub(super) async fn get(
    State(state): State<Arc<AppState>>,
    Extension(p): Extension<Principal>,
    ApiPath((org, id)): ApiPath<(Uuid, Uuid)>,
) -> ApiResult<Json<Session>> {
    Ok(Json(
        document(&state, &readable(&state, org, id, &p).await?, &p).await?,
    ))
}
pub(super) async fn list(
    State(state): State<Arc<AppState>>,
    Extension(p): Extension<Principal>,
    ApiPath(org): ApiPath<Uuid>,
    ApiQuery(mut filter): ApiQuery<SessionFilter>,
) -> ApiResult<Json<Page<Session>>> {
    let admin = is_admin(organization_role(&state.pool, org, &p).await?);
    if filter.team_id.is_some() {
        return Err(ApiError::bad(
            "unsupported_option",
            "Team filters are not implemented by this server.",
        ));
    }
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
    let scope = scope(("sessions", org, p.user_id()?, &filter))?;
    let cursor = decode_cursor(value.as_deref(), &scope)?;
    let mut q =
        QueryBuilder::<Postgres>::new("SELECT s.* FROM sessions s WHERE s.organization_id=");
    q.push_bind(org)
        .push(" AND (s.owner_id=")
        .push_bind(p.user_id()?)
        .push(" OR s.visibility='organization' OR ")
        .push_bind(admin)
        .push(")");
    if let Some(h) = filter.harness {
        q.push(" AND s.harness=").push_bind(h.as_str());
    }
    if let Some(author) = filter.author_id {
        q.push(" AND s.owner_id=").push_bind(author);
    }
    if let Some(project) = filter.project_id {
        q.push(" AND EXISTS(SELECT 1 FROM session_projects sp WHERE sp.session_id=s.id AND sp.project_id=").push_bind(project).push(")");
    }
    if filter.unassigned == Some(true) {
        q.push(" AND NOT EXISTS(SELECT 1 FROM session_projects sp WHERE sp.session_id=s.id)");
    }
    if let Some(tag) = &filter.tag {
        q.push(" AND s.document->'tags' @> ")
            .push_bind(sqlx::types::Json(json!([tag])));
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
        data.push(document(&state, row, &p).await?);
    }
    let next_cursor = if more {
        data.last()
            .map(|s| {
                encode_cursor(Cursor {
                    scope,
                    id: s.id,
                    time: Some(s.updated_at),
                    revision: None,
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
    Extension(p): Extension<Principal>,
    ApiPath((org, id)): ApiPath<(Uuid, Uuid)>,
    ApiQuery(filter): ApiQuery<MessageFilter>,
) -> ApiResult<Json<Page<Message>>> {
    let session = readable(&state, org, id, &p).await?;
    let revision: String = session.get("revision");
    let scope = scope(("messages", org, id, p.user_id()?, filter.leaf_id))?;
    let cursor = decode_cursor(filter.cursor.as_deref(), &scope)?;
    let size = limit(filter.limit)?;
    if cursor
        .as_ref()
        .is_some_and(|c| c.revision.as_deref() != Some(&revision))
    {
        return Err(ApiError::conflict(
            "cursor_stale",
            "The session changed. Restart this message listing.",
        ));
    }
    let mut sequence = -1;
    if let Some(c) = &cursor {
        sequence = sqlx::query_scalar::<_, i64>(
            "SELECT sequence FROM messages WHERE id=$1 AND session_id=$2",
        )
        .bind(c.id)
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| ApiError::conflict("cursor_stale", "Restart this message listing."))?;
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
        sqlx::query("WITH RECURSIVE ancestry AS (SELECT id,document,ARRAY[id] AS visited FROM messages WHERE id=$1 AND session_id=$2 UNION ALL SELECT m.id,m.document,a.visited||m.id FROM messages m JOIN ancestry a ON m.id=(a.document->>'conversation_parent_id')::uuid WHERE m.session_id=$2 AND NOT m.id=ANY(a.visited)) SELECT m.document FROM messages m JOIN ancestry a ON a.id=m.id WHERE m.sequence>$3 ORDER BY m.sequence,m.id LIMIT $4").bind(leaf).bind(id).bind(sequence).bind(size as i64+1).fetch_all(&state.pool).await?
    } else {
        sqlx::query("SELECT document FROM messages WHERE session_id=$1 AND sequence>$2 ORDER BY sequence,id LIMIT $3").bind(id).bind(sequence).bind(size as i64+1).fetch_all(&state.pool).await?
    };
    let mut data: Vec<Message> = rows
        .into_iter()
        .map(|r| r.get::<sqlx::types::Json<Message>, _>("document").0)
        .collect();
    let more = data.len() > size;
    data.truncate(size);
    for message in &mut data {
        hydrate_message(message, state.config.public_url.as_str(), org);
    }
    let next_cursor = if more {
        data.last()
            .map(|m| {
                encode_cursor(Cursor {
                    scope,
                    id: m.id,
                    time: None,
                    revision: Some(revision),
                })
            })
            .transpose()?
    } else {
        None
    };
    Ok(Json(Page { data, next_cursor }))
}
async fn message_row(state: &AppState, org: Uuid, id: Uuid, p: &Principal) -> ApiResult<PgRow> {
    let row = sqlx::query("SELECT m.* FROM messages m JOIN sessions s ON s.id=m.session_id WHERE m.id=$1 AND s.organization_id=$2").bind(id).bind(org).fetch_optional(&state.pool).await?.ok_or_else(ApiError::not_found)?;
    readable(state, org, row.get("session_id"), p).await?;
    Ok(row)
}
pub(super) async fn message(
    State(state): State<Arc<AppState>>,
    Extension(p): Extension<Principal>,
    ApiPath((org, id)): ApiPath<(Uuid, Uuid)>,
) -> ApiResult<Response> {
    let row = message_row(&state, org, id, &p).await?;
    let mut m: Message = row.get::<sqlx::types::Json<Message>, _>("document").0;
    hydrate_message(&mut m, state.config.public_url.as_str(), org);
    let etag = scope(&m)?;
    Ok(([(header::ETAG, format!("\"{etag}\""))], Json(m)).into_response())
}
fn hydrate_message(message: &mut Message, base: &str, org: Uuid) {
    let url = format!(
        "{}/v1/organizations/{org}/messages/{}/content",
        base.trim_end_matches('/'),
        message.id
    );
    if message.truncated {
        message.content_url = Some(url.clone());
    }
    for block in &mut message.content {
        if let sessiondb_types::ContentBlock::Reference(reference) = block {
            reference.content_url = url.clone();
        }
    }
}
pub(super) async fn full_content(
    State(state): State<Arc<AppState>>,
    Extension(p): Extension<Principal>,
    ApiPath((org, id)): ApiPath<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let row = message_row(&state, org, id, &p).await?;
    let m: Message = row.get::<sqlx::types::Json<Message>, _>("document").0;
    let etag = format!("\"{}\"", m.revision);
    if let Some(value) = headers.get(header::IF_MATCH)
        && !value.to_str().is_ok_and(|s| {
            s.split(',')
                .any(|candidate| candidate.trim() == etag || candidate.trim() == "*")
        })
    {
        return Err(ApiError::new(
            StatusCode::PRECONDITION_FAILED,
            "revision_changed",
            "Reload the message before expanding content.",
        ));
    }
    let full = FullContent {
        message_id: id,
        revision: m.revision,
        content: row.get::<sqlx::types::Json<_>, _>("full_content").0,
    };
    Ok(([(header::ETAG, etag)], Json(full)).into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ExportOptions {
    format: Option<String>,
    leaf_id: Option<Uuid>,
}
pub(super) async fn export(
    State(state): State<Arc<AppState>>,
    Extension(p): Extension<Principal>,
    ApiPath((org, id)): ApiPath<(Uuid, Uuid)>,
    ApiQuery(options): ApiQuery<ExportOptions>,
) -> ApiResult<Response> {
    if options.format.as_deref().is_some_and(|v| v != "native") || options.leaf_id.is_some() {
        return Err(ApiError::bad(
            "unsupported_option",
            "This server supports native ZIP export without leaf_id.",
        ));
    }
    let first = readable(&state, org, id, &p).await?;
    let mut queue = vec![document(&state, &first, &p).await?];
    let mut visited = std::collections::HashSet::new();
    let mut files = std::collections::BTreeMap::new();
    let mut warnings = Vec::new();
    while let Some(session) = queue.pop() {
        if !visited.insert(session.id) {
            continue;
        }
        if visited.len() > 128 {
            warnings.push("dependency_limit");
            break;
        }
        let rows = sqlx::query("SELECT DISTINCT ON(f.id) f.id,f.source_id,f.path,g.id AS generation_id,g.object_key,g.byte_length,g.sha256 FROM session_files sf JOIN source_files f ON f.id=sf.file_id JOIN file_generations g ON g.id=sf.generation_id WHERE sf.session_id=$1 ORDER BY f.id,(g.id=f.generation_id) DESC,g.created_at DESC").bind(session.id).fetch_all(&state.pool).await?;
        for row in rows {
            if file_readable(&state, org, row.get("id"), row.get("generation_id"), &p).await? {
                files.insert(
                    (
                        row.get::<Uuid, _>("id"),
                        row.get::<Uuid, _>("generation_id"),
                    ),
                    row,
                );
            } else {
                warnings.push("unavailable_original");
            }
        }
        for relation in session.relations {
            if let Some(parent) = relation.session_id {
                if let Ok(row) = readable(&state, org, parent, &p).await {
                    queue.push(document(&state, &row, &p).await?);
                }
            } else {
                warnings.push("unavailable_dependency");
            }
        }
    }
    let directory = tempfile::tempdir().map_err(ApiError::internal)?;
    let mut entries = Vec::new();
    let mut manifest_files = Vec::new();
    for (index, row) in files.values().enumerate() {
        let path: String = row.get("path");
        if !super::imports::valid_path(&path) {
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
    warnings.sort();
    warnings.dedup();
    let manifest = serde_json::to_vec_pretty(
        &json!({"version":1,"session_id":id,"files":manifest_files,"warnings":warnings}),
    )?;
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
pub(crate) async fn file_readable(
    state: &AppState,
    org: Uuid,
    file: Uuid,
    generation: Uuid,
    p: &Principal,
) -> ApiResult<bool> {
    let admin = is_admin(organization_role(&state.pool, org, p).await?);
    let row = sqlx::query("SELECT s.owner_id FROM source_files f JOIN sources s ON s.id=f.source_id WHERE f.id=$1 AND s.organization_id=$2").bind(file).bind(org).fetch_optional(&state.pool).await?;
    let Some(row) = row else {
        return Ok(false);
    };
    if admin || row.get::<Uuid, _>("owner_id") == p.user_id()? {
        return Ok(true);
    }
    let result: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM session_files WHERE file_id=$1 AND generation_id=$2) AND NOT EXISTS(SELECT 1 FROM session_files sf JOIN sessions s ON s.id=sf.session_id WHERE sf.file_id=$1 AND sf.generation_id=$2 AND s.owner_id<>$3 AND s.visibility<>'organization')").bind(file).bind(generation).bind(p.user_id()?).fetch_one(&state.pool).await?;
    Ok(result)
}
async fn artifact_row(state: &AppState, org: Uuid, id: Uuid, p: &Principal) -> ApiResult<PgRow> {
    let row = sqlx::query("SELECT a.* FROM artifacts a JOIN sessions s ON s.id=a.session_id WHERE a.id=$1 AND s.organization_id=$2").bind(id).bind(org).fetch_optional(&state.pool).await?.ok_or_else(ApiError::not_found)?;
    readable(state, org, row.get("session_id"), p).await?;
    Ok(row)
}
pub(super) async fn artifact(
    State(state): State<Arc<AppState>>,
    Extension(p): Extension<Principal>,
    ApiPath((org, id)): ApiPath<(Uuid, Uuid)>,
) -> ApiResult<Json<Artifact>> {
    let row = artifact_row(&state, org, id, &p).await?;
    Ok(Json(Artifact {
        id,
        session_id: row.get("session_id"),
        media_type: row.get("media_type"),
        kind: row.get("kind"),
        byte_length: row.get::<i64, _>("byte_length") as u64,
        content_url: format!(
            "{}/v1/organizations/{org}/artifacts/{id}/content",
            state.config.public_url.as_str().trim_end_matches('/')
        ),
    }))
}
pub(super) async fn artifact_content(
    State(state): State<Arc<AppState>>,
    Extension(p): Extension<Principal>,
    ApiPath((org, id)): ApiPath<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let row = artifact_row(&state, org, id, &p).await?;
    super::sources::stream(
        &state,
        row.get("object_key"),
        row.get("media_type"),
        &headers,
    )
    .await
}
