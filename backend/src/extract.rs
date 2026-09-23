//! Request extractors with the plan's error answers.

use axum::extract::rejection::JsonRejection;
use axum::extract::{FromRequest, Request};
use serde::de::DeserializeOwned;
use serde_json::json;

use crate::error::ApiError;

/// JSON bodies. A `Content-Type` other than `application/json` is `415`,
/// which keeps a cross-site form from posting to login or registration
/// (PLAN.md § API).
pub struct ApiJson<T>(pub T);

impl<S, T> FromRequest<S> for ApiJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, ApiError> {
        match axum::Json::<T>::from_request(req, state).await {
            Ok(axum::Json(v)) => Ok(ApiJson(v)),
            Err(JsonRejection::MissingJsonContentType(_)) => Err(ApiError::UnsupportedMediaType),
            Err(JsonRejection::BytesRejection(e)) => {
                if e.status() == axum::http::StatusCode::PAYLOAD_TOO_LARGE {
                    Err(ApiError::BadRequest(json!({ "error": "body_too_large" })))
                } else {
                    Err(ApiError::BadRequest(json!({ "error": "invalid_body" })))
                }
            }
            Err(e) => Err(ApiError::BadRequest(
                json!({ "error": "invalid_json", "message": e.body_text() }),
            )),
        }
    }
}
