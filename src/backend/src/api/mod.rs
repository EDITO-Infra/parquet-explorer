//! HTTP transport layer for `/api/v1`.
//!
//! Each feature owns its routes and request-shape validation. Shared Parquet
//! execution stays in `engine`, preventing table, spatial, analysis, and export
//! transport concerns from accumulating in one handler module.

mod analysis;
mod common;
mod datasets;
mod diagnostics;
mod export;
mod health;
mod query;
mod spatial;

use std::sync::Arc;

use axum::Router;

use crate::AppState;

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .merge(health::routes())
        .merge(datasets::routes())
        .merge(analysis::routes())
        .merge(query::routes())
        .merge(spatial::routes())
        .merge(export::routes())
        .merge(diagnostics::routes())
        .with_state(state)
}
