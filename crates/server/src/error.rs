use axum::{
    Json,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use sessiondb_types::{FileError, Problem};
use uuid::Uuid;

tokio::task_local! { pub static REQUEST_ID: String; }

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub detail: String,
    pub errors: Vec<FileError>,
}
pub type ApiResult<T> = Result<T, ApiError>;
impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, detail: impl Into<String>) -> Self {
        Self {
            status,
            code,
            detail: detail.into(),
            errors: vec![],
        }
    }
    pub fn bad(code: &'static str, detail: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, detail)
    }
    pub fn not_found() -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "The resource was not found.",
        )
    }
    pub fn unauthorized() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "authentication_required",
            "Supply the instance API key as a Bearer credential.",
        )
    }
    pub fn conflict(code: &'static str, detail: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, code, detail)
    }
    pub fn unavailable() -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "dependency_unavailable",
            "A required service is temporarily unavailable.",
        )
    }
    pub fn internal(error: impl std::fmt::Display) -> Self {
        tracing::error!(error = %error, "request failed");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "The request could not be completed.",
        )
    }
}
impl From<sqlx::Error> for ApiError {
    fn from(error: sqlx::Error) -> Self {
        Self::internal(error)
    }
}
impl From<serde_json::Error> for ApiError {
    fn from(error: serde_json::Error) -> Self {
        Self::internal(error)
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let request_id = REQUEST_ID
            .try_with(Clone::clone)
            .unwrap_or_else(|_| Uuid::new_v4().to_string());
        let problem = Problem {
            problem_type: "about:blank".into(),
            title: self
                .status
                .canonical_reason()
                .unwrap_or("Request failed")
                .into(),
            status: self.status.as_u16(),
            detail: self.detail,
            code: self.code.into(),
            request_id,
            instance: None,
            errors: self.errors,
        };
        (
            self.status,
            [
                (header::CONTENT_TYPE, "application/problem+json"),
                (header::CACHE_CONTROL, "no-store"),
            ],
            Json(problem),
        )
            .into_response()
    }
}
