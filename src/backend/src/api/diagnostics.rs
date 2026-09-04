use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::get,
};

use crate::AppState;

pub(super) fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/diagnostics/{trace_id}", get(diagnostics))
}

async fn diagnostics(
    State(state): State<Arc<AppState>>,
    Path(trace_id): Path<String>,
) -> Result<Json<crate::model::TraceSnapshot>, StatusCode> {
    state
        .engine
        .trace_snapshot(&trace_id)
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}
