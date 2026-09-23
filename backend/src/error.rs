//! API errors and their JSON bodies.

use axum::Json;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use serde_json::{Value, json};

/// A field error for the auth and account forms (PQ-002: shape).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FieldError {
    pub field: String,
    pub message: String,
}

impl FieldError {
    pub fn new(field: &str, message: impl Into<String>) -> Self {
        FieldError {
            field: field.into(),
            message: message.into(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("bad request")]
    BadRequest(Value),
    #[error("field errors")]
    Fields(Vec<FieldError>),
    #[error("unauthorized")]
    Unauthorized,
    #[error("forbidden")]
    Forbidden(&'static str),
    #[error("not found")]
    NotFound,
    #[error("conflict")]
    Conflict(Value),
    #[error("unsupported media type")]
    UnsupportedMediaType,
    #[error("unprocessable")]
    Unprocessable(Value),
    #[error("too many requests")]
    TooManyRequests { retry_after_secs: u64 },
    #[error("service unavailable")]
    Unavailable { retry_after_secs: u64 },
    #[error("upgrade required")]
    UpgradeRequired(Value),
    #[error("internal: {0}")]
    Internal(#[from] anyhow::Error),
}

impl ApiError {
    pub fn bad_request(error: &str) -> Self {
        ApiError::BadRequest(json!({ "error": error }))
    }

    pub fn conflict(error: &str) -> Self {
        ApiError::Conflict(json!({ "error": error }))
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        ApiError::Internal(e.into())
    }
}

fn with_retry_after(mut resp: Response, secs: u64) -> Response {
    resp.headers_mut().insert(
        header::RETRY_AFTER,
        HeaderValue::from_str(&secs.max(1).to_string()).expect("digits"),
    );
    resp
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self {
            ApiError::BadRequest(v) => (StatusCode::BAD_REQUEST, Json(v)).into_response(),
            ApiError::Fields(errors) => {
                (StatusCode::BAD_REQUEST, Json(json!({ "errors": errors }))).into_response()
            }
            ApiError::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "unauthorized" })),
            )
                .into_response(),
            ApiError::Forbidden(reason) => {
                (StatusCode::FORBIDDEN, Json(json!({ "error": reason }))).into_response()
            }
            ApiError::NotFound => {
                (StatusCode::NOT_FOUND, Json(json!({ "error": "not_found" }))).into_response()
            }
            ApiError::Conflict(v) => (StatusCode::CONFLICT, Json(v)).into_response(),
            ApiError::UnsupportedMediaType => (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                Json(json!({ "error": "unsupported_media_type" })),
            )
                .into_response(),
            ApiError::Unprocessable(v) => {
                (StatusCode::UNPROCESSABLE_ENTITY, Json(v)).into_response()
            }
            ApiError::TooManyRequests { retry_after_secs } => with_retry_after(
                (
                    StatusCode::TOO_MANY_REQUESTS,
                    Json(json!({ "error": "rate_limited" })),
                )
                    .into_response(),
                retry_after_secs,
            ),
            ApiError::Unavailable { retry_after_secs } => with_retry_after(
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({ "error": "unavailable" })),
                )
                    .into_response(),
                retry_after_secs,
            ),
            ApiError::UpgradeRequired(v) => (StatusCode::UPGRADE_REQUIRED, Json(v)).into_response(),
            ApiError::Internal(e) => {
                tracing::error!(error = %e, "internal error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": "internal" })),
                )
                    .into_response()
            }
        }
    }
}

pub type ApiResult<T> = Result<T, ApiError>;
