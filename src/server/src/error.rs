use axum::{Json, http::StatusCode, response::{IntoResponse, Response}};
use serde::Serialize;

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    message: String,
}

#[derive(Serialize)]
struct ErrorBody<'a> { error: &'a str }

impl ApiError {
    pub fn bad_request(message: impl Into<String>) -> Self { Self { status: StatusCode::BAD_REQUEST, message: message.into() } }
    pub fn not_implemented(message: impl Into<String>) -> Self { Self { status: StatusCode::NOT_IMPLEMENTED, message: message.into() } }
    pub fn internal(message: impl Into<String>) -> Self { Self { status: StatusCode::INTERNAL_SERVER_ERROR, message: message.into() } }
    pub fn from_engine(error: anyhow::Error) -> Self {
        let message = format!("{error:#}");
        if message.contains("dataset not found") {
            Self { status: StatusCode::NOT_FOUND, message }
        } else if message.contains("unknown column") || message.contains("bbox") || message.contains("geometry") || message.contains("maximum number") {
            Self::bad_request(message)
        } else {
            tracing::error!(error = %message, "engine error");
            Self::internal(message)
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(ErrorBody { error: &self.message })).into_response()
    }
}
