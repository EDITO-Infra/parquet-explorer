//! Mapping from internal engine errors to stable HTTP error responses.
//!
//! The engine uses `anyhow` for context-rich internal errors. This module keeps
//! transport-specific status-code decisions out of the engine itself.

use axum::{Json, http::StatusCode, response::{IntoResponse, Response}};
use serde::Serialize;

/// HTTP-safe error wrapper used only at the API boundary.
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
    /// Classify context-rich engine errors into client/server HTTP failures.
    /// Unknown failures are logged server-side and returned as HTTP 500.
    pub fn from_engine(error: anyhow::Error) -> Self {
        let message = format!("{error:#}");
        if message.contains("dataset not found") {
            Self { status: StatusCode::NOT_FOUND, message }
        } else if message.contains("unknown column") || message.contains("filter") || message.contains("bbox") || message.contains("geometry") || message.contains("maximum number") || message.contains("row_group") || message.contains("out of range") || message.contains("limit") {
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
