//! API 统一错误类型（映射到 HTTP 状态码）。

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

pub type ApiResult<T> = Result<T, ApiError>;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("unauthorized: {0}")]
    Unauthorized(String),
    #[error("forbidden: insufficient scope")]
    Forbidden,
    #[error("not found")]
    NotFound,
    #[error("payment required: insufficient credits")]
    InsufficientCredits,
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("no available model provider")]
    NoProvider,
    #[error("upstream error: {0}")]
    Upstream(String),
    #[error("internal error: {0}")]
    Internal(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            ApiError::Unauthorized(_) => (StatusCode::UNAUTHORIZED, "unauthorized"),
            ApiError::Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
            ApiError::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            ApiError::InsufficientCredits => (StatusCode::PAYMENT_REQUIRED, "insufficient_credits"),
            ApiError::BadRequest(_) => (StatusCode::BAD_REQUEST, "bad_request"),
            ApiError::NoProvider => (StatusCode::SERVICE_UNAVAILABLE, "no_provider"),
            ApiError::Upstream(_) => (StatusCode::BAD_GATEWAY, "upstream_error"),
            ApiError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
        };
        let body = Json(json!({
            "error": {
                "code": code,
                "message": self.to_string(),
            }
        }));
        (status, body).into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError::Internal(e.to_string())
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        ApiError::Internal(format!("db error: {e}"))
    }
}
