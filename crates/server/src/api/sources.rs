use super::*;
use crate::{
    auth::{Principal, is_admin, organization_role},
    objects::ObjectError,
};
use axum::{Extension, body::Body, http::HeaderMap};
use serde::Deserialize;
use sessiondb_types::{Harness, ListOptions, Source, SourceCreate, SourceFile};
use sqlx::postgres::PgRow;
use tokio_util::io::ReaderStream;

pub(crate) fn source(row: &PgRow) -> ApiResult<Source> {
    Ok(Source {
        id: row.get("id"),
        organization_id: row.get("organization_id"),
        harness: row
            .get::<String, _>("harness")
            .parse()
            .map_err(ApiError::internal)?,
        device_label: row.get("device_label"),
        root: row.get("root"),
        default_visibility: row
            .get::<String, _>("default_visibility")
            .parse()
            .map_err(ApiError::internal)?,
        owner_id: row.get("owner_id"),
        paused: row.get("paused"),
        last_synced_at: row.get("last_synced_at"),
    })
}
pub(crate) async fn owned(
    state: &AppState,
    org: Uuid,
    id: Uuid,
    p: &Principal,
) -> ApiResult<Source> {
    let admin = is_admin(organization_role(&state.pool, org, p).await?);
    let row = sqlx::query(
        "SELECT * FROM sources WHERE id=$1 AND organization_id=$2 AND (owner_id=$3 OR $4)",
    )
    .bind(id)
    .bind(org)
    .bind(p.user_id()?)
    .bind(admin)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(ApiError::not_found)?;
    source(&row)
}
pub(super) async fn list(
    State(state): State<Arc<AppState>>,
    Extension(p): Extension<Principal>,
    ApiPath(org): ApiPath<Uuid>,
    ApiQuery(o): ApiQuery<ListOptions>,
) -> ApiResult<Json<Page<Source>>> {
    let admin = is_admin(organization_role(&state.pool, org, &p).await?);
    let scope = scope(("sources", org, p.user_id()?, o.harness))?;
    let cursor = decode_cursor(o.cursor.as_deref(), &scope)?;
    let size = limit(o.limit)?;
    let rows = sqlx::query("SELECT * FROM sources WHERE organization_id=$1 AND (owner_id=$2 OR $3) AND ($4::text IS NULL OR harness=$4) AND ($5::uuid IS NULL OR id>$5) ORDER BY id LIMIT $6").bind(org).bind(p.user_id()?).bind(admin).bind(o.harness.map(Harness::as_str)).bind(cursor.map(|c| c.id)).bind(size as i64+1).fetch_all(&state.pool).await?;
    let data = rows.iter().map(source).collect::<ApiResult<Vec<_>>>()?;
    Ok(Json(page(data, size, scope, |s| s.id)?))
}
pub(super) async fn create(
    State(state): State<Arc<AppState>>,
    Extension(p): Extension<Principal>,
    ApiPath(org): ApiPath<Uuid>,
    ApiJson(input): ApiJson<SourceCreate>,
) -> ApiResult<Response> {
    organization_role(&state.pool, org, &p).await?;
    if input.root.is_empty()
        || input.root.len() > 4096
        || input.root.chars().any(char::is_control)
        || input.device_label.is_empty()
        || input.device_label.len() > 200
    {
        return Err(ApiError::bad(
            "invalid_source",
            "A source needs a device label and native store root.",
        ));
    }
    let row = sqlx::query("INSERT INTO sources(id,organization_id,owner_id,harness,device_label,root,default_visibility) VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT(organization_id,owner_id,device_label,harness,root) DO NOTHING RETURNING *").bind(Uuid::new_v4()).bind(org).bind(p.user_id()?).bind(input.harness.as_str()).bind(&input.device_label).bind(&input.root).bind(input.default_visibility.as_str()).fetch_optional(&state.pool).await?;
    let row = row.ok_or_else(|| {
        ApiError::conflict(
            "source_exists",
            "This native store is already registered for the device.",
        )
    })?;
    let result = source(&row)?;
    let location = format!(
        "{}/v1/organizations/{org}/sources/{}",
        state.config.public_url.as_str().trim_end_matches('/'),
        result.id
    );
    Ok((
        StatusCode::CREATED,
        [(header::LOCATION, location)],
        Json(result),
    )
        .into_response())
}
pub(super) async fn get(
    State(state): State<Arc<AppState>>,
    Extension(p): Extension<Principal>,
    ApiPath((org, id)): ApiPath<(Uuid, Uuid)>,
) -> ApiResult<Json<Source>> {
    Ok(Json(owned(&state, org, id, &p).await?))
}
pub(super) async fn files(
    State(state): State<Arc<AppState>>,
    Extension(p): Extension<Principal>,
    ApiPath((org, id)): ApiPath<(Uuid, Uuid)>,
    ApiQuery(o): ApiQuery<ListOptions>,
) -> ApiResult<Json<Page<SourceFile>>> {
    owned(&state, org, id, &p).await?;
    if o.harness.is_some() {
        return Err(ApiError::bad(
            "unsupported_option",
            "harness is not supported here.",
        ));
    }
    let scope = scope(("files", org, id, p.user_id()?))?;
    let cursor = decode_cursor(o.cursor.as_deref(), &scope)?;
    let size = limit(o.limit)?;
    let rows = sqlx::query("SELECT * FROM source_files WHERE source_id=$1 AND ($2::uuid IS NULL OR id>$2) ORDER BY id LIMIT $3").bind(id).bind(cursor.map(|c| c.id)).bind(size as i64+1).fetch_all(&state.pool).await?;
    let data = rows.iter().map(file).collect();
    Ok(Json(page(data, size, scope, |f| f.id)?))
}
pub(crate) fn file(r: &PgRow) -> SourceFile {
    SourceFile {
        id: r.get("id"),
        source_id: r.get("source_id"),
        path: r.get("path"),
        generation_id: r.get("generation_id"),
        version: r.get("version"),
        byte_length: r.get::<i64, _>("byte_length") as u64,
        sha256: r.get("sha256"),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ContentOptions {
    generation_id: Option<Uuid>,
}
pub(super) async fn content(
    State(state): State<Arc<AppState>>,
    Extension(p): Extension<Principal>,
    ApiPath((org, source_id, file_id)): ApiPath<(Uuid, Uuid, Uuid)>,
    ApiQuery(o): ApiQuery<ContentOptions>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    organization_role(&state.pool, org, &p).await?;
    let row = sqlx::query("SELECT g.id,g.object_key,g.byte_length FROM source_files f JOIN sources s ON s.id=f.source_id JOIN file_generations g ON g.file_id=f.id AND g.id=COALESCE($3,f.generation_id) WHERE f.id=$1 AND f.source_id=$2 AND s.organization_id=$4").bind(file_id).bind(source_id).bind(o.generation_id).bind(org).fetch_optional(&state.pool).await?.ok_or_else(ApiError::not_found)?;
    if !super::sessions::file_readable(&state, org, file_id, row.get("id"), &p).await? {
        return Err(ApiError::not_found());
    }
    stream(
        &state,
        row.get("object_key"),
        "application/octet-stream",
        &headers,
    )
    .await
}
pub(crate) fn storage_error(e: ObjectError) -> ApiError {
    match e {
        ObjectError::Missing => ApiError::unavailable(),
        ObjectError::InvalidRange => ApiError::new(
            StatusCode::RANGE_NOT_SATISFIABLE,
            "invalid_range",
            "The requested byte range is unavailable.",
        ),
        _ => ApiError::unavailable(),
    }
}
pub(crate) async fn stream(
    state: &AppState,
    key: &str,
    media_type: &str,
    headers: &HeaderMap,
) -> ApiResult<Response> {
    let range = headers
        .get(header::RANGE)
        .map(|h| {
            h.to_str()
                .map_err(|_| ApiError::bad("invalid_range", "Use one bytes range."))
        })
        .transpose()?;
    if range.is_some_and(|r| !valid_range(r)) {
        return Err(ApiError::bad("invalid_range", "Use one bytes range."));
    }
    let body = state.objects.get(key, range).await.map_err(storage_error)?;
    let mut response = Response::new(Body::from_stream(ReaderStream::new(body.reader)));
    *response.status_mut() = if body.content_range.is_some() {
        StatusCode::PARTIAL_CONTENT
    } else {
        StatusCode::OK
    };
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        media_type
            .parse()
            .map_err(|_| ApiError::internal("invalid stored media type"))?,
    );
    response
        .headers_mut()
        .insert(header::CONTENT_LENGTH, body.byte_length.into());
    response.headers_mut().insert(
        header::ACCEPT_RANGES,
        "bytes".parse().expect("constant header"),
    );
    if let Some(range) = body.content_range {
        response.headers_mut().insert(
            header::CONTENT_RANGE,
            range.parse().map_err(ApiError::internal)?,
        );
    }
    Ok(response)
}
fn valid_range(value: &str) -> bool {
    let Some(range) = value.strip_prefix("bytes=") else {
        return false;
    };
    let Some((start, end)) = range.split_once('-') else {
        return false;
    };
    !(start.is_empty() && end.is_empty())
        && (start.is_empty() || start.parse::<u64>().is_ok())
        && (end.is_empty() || end.parse::<u64>().is_ok())
}
