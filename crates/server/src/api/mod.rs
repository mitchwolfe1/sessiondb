mod profiles;
mod search;
pub(crate) mod sessions;
pub(crate) mod sources;

use crate::{
    AppState,
    auth::authenticate,
    error::{ApiError, ApiResult, REQUEST_ID},
};
use axum::{
    Json, Router,
    extract::{FromRequest, FromRequestParts, Request, State},
    http::{StatusCode, header, request::Parts},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{any, get, post},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sessiondb_types::{FIRST_BUILD_OPERATIONS, Page, ServerInfo};
use sqlx::Row;
use std::sync::{Arc, atomic::Ordering};
use uuid::Uuid;

pub fn router(state: Arc<AppState>) -> Router {
    let protected = Router::new()
        .route("/v1/info", get(info))
        .route("/v1/sources", get(sources::list).post(sources::create))
        .route(
            "/v1/sources/{source_id}",
            get(sources::get).patch(sources::set_member),
        )
        .route(
            "/v1/sources/{source_id}/files",
            get(sources::files).post(crate::ingest::upload),
        )
        .route(
            "/v1/sources/{source_id}/files/{file_id}/content",
            get(sources::content),
        )
        .route("/v1/sessions", get(sessions::list))
        .route(
            "/v1/sessions/{session_id}",
            get(sessions::get).patch(unsupported).delete(unsupported),
        )
        .route(
            "/v1/sessions/{session_id}/messages",
            get(sessions::messages),
        )
        .route("/v1/sessions/{session_id}/export", get(sessions::export))
        .route(
            "/v1/embedding-profiles",
            get(profiles::list).post(profiles::create),
        )
        .route(
            "/v1/embedding-profiles/{profile_id}",
            get(profiles::get).patch(profiles::update),
        )
        .route("/v1/search", post(search::search))
        .route("/v1/messages/{message_id}/around", get(sessions::around))
        .route("/v1/messages/{message_id}", get(sessions::message))
        .route(
            "/v1/messages/{message_id}/content",
            get(sessions::full_content),
        )
        .route("/v1/artifacts/{artifact_id}", get(sessions::artifact))
        .route(
            "/v1/artifacts/{artifact_id}/content",
            get(sessions::artifact_content),
        );
    let mut protected = protected;
    for path in [
        "/v1/projects",
        "/v1/projects/{project_id}",
        "/v1/changes",
        "/v1/sources/{source_id}/files/{file_id}/generations",
        "/v1/sessions/{session_id}/events",
    ] {
        protected = protected.route(path, any(unsupported));
    }
    let public = Router::new()
        .route(
            "/healthz",
            get(|| async { Json(serde_json::json!({"status":"ok"})) }),
        )
        .route("/readyz", get(readiness))
        .route(
            "/openapi.yaml",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "application/yaml")],
                    include_str!("../../../../docs/openapi.yaml"),
                )
            }),
        );
    public
        .merge(protected.route_layer(middleware::from_fn_with_state(state.clone(), authenticate)))
        .fallback(|| async { ApiError::not_found() })
        .layer(axum::extract::DefaultBodyLimit::max(
            usize::try_from(state.config.limits.max_file_bytes)
                .unwrap_or(usize::MAX)
                .saturating_add(128 * 1024),
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            request_context,
        ))
        .with_state(state)
}

async fn unsupported() -> ApiError {
    ApiError::new(
        StatusCode::NOT_IMPLEMENTED,
        "not_implemented",
        "This operation is not implemented by this server. See /v1/info for supported capabilities.",
    )
}
async fn info(State(state): State<Arc<AppState>>) -> Json<ServerInfo> {
    let mut capabilities: Vec<String> =
        FIRST_BUILD_OPERATIONS.iter().map(|v| (*v).into()).collect();
    capabilities.extend(
        [
            "codex",
            "claude",
            "pi",
            "getArtifact",
            "getArtifactContent",
            "searchMessages",
            "readAroundMessage",
            "setSourceMember",
            "listEmbeddingProfiles",
            "createEmbeddingProfile",
            "getEmbeddingProfile",
            "updateEmbeddingProfile",
        ]
        .map(str::to_owned),
    );
    Json(ServerInfo {
        api_version: "v1".into(),
        server_version: env!("CARGO_PKG_VERSION").into(),
        capabilities,
        limits: state.config.limits.clone(),
    })
}
async fn readiness(State(state): State<Arc<AppState>>) -> ApiResult<Json<serde_json::Value>> {
    sqlx::query("SELECT 1")
        .execute(&state.pool)
        .await
        .map_err(|_| ApiError::unavailable())?;
    state
        .objects
        .check()
        .await
        .map_err(|_| ApiError::unavailable())?;
    Ok(Json(serde_json::json!({"status":"ready"})))
}
async fn request_context(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Response {
    let id = Uuid::new_v4().to_string();
    let method = request.method().clone();
    let path = request
        .extensions()
        .get::<axum::extract::MatchedPath>()
        .map(|p| p.as_str().to_owned())
        .unwrap_or_else(|| "<unmatched>".into());
    let started = std::time::Instant::now();
    let request_id = id.clone();
    REQUEST_ID.scope(id, async move {
        let mut response = next.run(request).await;
        if (response.status().is_client_error() || response.status().is_server_error()) && response.headers().get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()) != Some("application/problem+json") {
            response = ApiError::new(response.status(), "invalid_request", "The request could not be processed.").into_response();
        }
        response.headers_mut().insert("x-request-id", request_id.parse().expect("UUID header"));
        response.headers_mut().insert(header::CACHE_CONTROL, "no-store".parse().expect("constant header"));
        if response.status() == StatusCode::UNAUTHORIZED { response.headers_mut().insert(header::WWW_AUTHENTICATE, "Bearer".parse().expect("constant header")); }
        state.requests.fetch_add(1, Ordering::Relaxed);
        tracing::info!(request_id = %request_id, %method, %path, status = response.status().as_u16(), duration_ms = started.elapsed().as_millis(), "request");
        response
    }).await
}

pub struct ApiJson<T>(pub T);
impl<S: Send + Sync, T: DeserializeOwned> FromRequest<S> for ApiJson<T> {
    type Rejection = ApiError;
    async fn from_request(request: Request, state: &S) -> ApiResult<Self> {
        Json::<T>::from_request(request, state)
            .await
            .map(|Json(v)| Self(v))
            .map_err(|e| {
                ApiError::new(
                    e.status(),
                    "invalid_json",
                    "The request must contain a valid JSON document with supported fields.",
                )
            })
    }
}
pub struct ApiQuery<T>(pub T);
impl<S: Send + Sync, T: DeserializeOwned> FromRequestParts<S> for ApiQuery<T> {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &S) -> ApiResult<Self> {
        axum::extract::Query::<T>::from_request_parts(parts, state)
            .await
            .map(|axum::extract::Query(v)| Self(v))
            .map_err(|_| {
                ApiError::bad(
                    "invalid_query",
                    "The query contains invalid or unsupported parameters.",
                )
            })
    }
}
pub struct ApiPath<T>(pub T);
impl<S: Send + Sync, T: DeserializeOwned + Send> FromRequestParts<S> for ApiPath<T> {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &S) -> ApiResult<Self> {
        axum::extract::Path::<T>::from_request_parts(parts, state)
            .await
            .map(|axum::extract::Path(v)| Self(v))
            .map_err(|_| ApiError::bad("invalid_path", "The resource identifier is invalid."))
    }
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Cursor {
    pub scope: String,
    pub id: Uuid,
    pub time: Option<DateTime<Utc>>,
}
pub(crate) fn scope(value: impl Serialize) -> ApiResult<String> {
    Ok(hex::encode(sha2::Sha256::digest(serde_json::to_vec(
        &value,
    )?)))
}
use sha2::Digest;
pub(crate) fn decode_cursor(
    value: Option<&str>,
    expected_scope: &str,
) -> ApiResult<Option<Cursor>> {
    let Some(value) = value else {
        return Ok(None);
    };
    let cursor: Cursor = decode_position(value)?;
    if cursor.scope != expected_scope {
        return Err(ApiError::bad(
            "invalid_cursor",
            "The cursor does not match this request.",
        ));
    }
    Ok(Some(cursor))
}
fn decode_position<T: DeserializeOwned>(value: &str) -> ApiResult<T> {
    if value.len() > 4096 {
        return Err(ApiError::bad("invalid_cursor", "The cursor is invalid."));
    }
    URL_SAFE_NO_PAD
        .decode(value)
        .ok()
        .and_then(|v| serde_json::from_slice(&v).ok())
        .ok_or_else(|| ApiError::bad("invalid_cursor", "The cursor is invalid."))
}
pub(crate) fn encode_cursor(cursor: impl Serialize) -> ApiResult<String> {
    Ok(URL_SAFE_NO_PAD.encode(serde_json::to_vec(&cursor)?))
}
pub(crate) fn limit(value: Option<u32>) -> ApiResult<usize> {
    match value.unwrap_or(50) {
        n @ 1..=200 => Ok(n as usize),
        _ => Err(ApiError::bad(
            "invalid_limit",
            "limit must be between 1 and 200.",
        )),
    }
}
pub(crate) fn page<T>(
    mut data: Vec<T>,
    size: usize,
    scope: String,
    id: impl Fn(&T) -> Uuid,
) -> ApiResult<Page<T>> {
    let more = data.len() > size;
    data.truncate(size);
    let next_cursor = if more {
        data.last()
            .map(|v| {
                encode_cursor(Cursor {
                    scope,
                    id: id(v),
                    time: None,
                })
            })
            .transpose()?
    } else {
        None
    };
    Ok(Page { data, next_cursor })
}
