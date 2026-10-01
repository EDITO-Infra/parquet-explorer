use std::sync::Arc;

use axum::{Json, Router, routing::get};

use crate::{AppState, model::HealthResponse};

pub(super) fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/health", get(health))
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        native_engine: format!("parquet-explorer-backend/{}", env!("CARGO_PKG_VERSION")),
    })
}
