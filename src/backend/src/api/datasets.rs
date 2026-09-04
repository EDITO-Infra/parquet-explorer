use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post},
};

use crate::{
    AppState,
    error::ApiError,
    model::{DatasetInfo, DatasetOpenRequest, SchemaResponse},
    security::validate_source_uri,
};

pub(super) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/datasets/open", post(open_dataset))
        .route("/datasets/{dataset_id}/metadata", get(metadata))
        .route("/datasets/{dataset_id}/schema", get(schema))
        .route("/datasets/{dataset_id}", delete(close_dataset))
}

async fn open_dataset(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<DatasetOpenRequest>,
) -> Result<(StatusCode, Json<DatasetInfo>), ApiError> {
    if payload.uri.trim().is_empty() {
        return Err(ApiError::bad_request("uri must not be empty"));
    }
    if payload.name.as_ref().is_some_and(|name| name.len() > 256) {
        return Err(ApiError::bad_request("name exceeds 256 characters"));
    }

    let trace_id = payload.trace_id.clone();
    state
        .engine
        .start_trace(trace_id.as_deref(), "open", "Checking source URL");
    let uri = match validate_source_uri(&payload.uri, &state.settings).await {
        Ok(uri) => uri,
        Err(error) => {
            state
                .engine
                .trace_fail(trace_id.as_deref(), "Source URL was rejected");
            return Err(ApiError::bad_request(format!("{error:#}")));
        }
    };
    state.engine.trace_event(
        trace_id.as_deref(),
        "source_validated",
        "Source URL accepted",
    );
    let info = state
        .engine
        .open_dataset(&uri, payload.name, trace_id.as_deref())
        .await
        .map_err(ApiError::from_engine)?;
    Ok((StatusCode::CREATED, Json(info)))
}

async fn metadata(
    State(state): State<Arc<AppState>>,
    Path(dataset_id): Path<String>,
) -> Result<Json<DatasetInfo>, ApiError> {
    state
        .engine
        .metadata(&dataset_id)
        .map(Json)
        .map_err(ApiError::from_engine)
}

async fn schema(
    State(state): State<Arc<AppState>>,
    Path(dataset_id): Path<String>,
) -> Result<Json<SchemaResponse>, ApiError> {
    let info = state
        .engine
        .metadata(&dataset_id)
        .map_err(ApiError::from_engine)?;
    Ok(Json(SchemaResponse {
        dataset_id: info.dataset_id,
        columns: info.columns,
        geo_columns: info.geo_columns,
    }))
}

async fn close_dataset(
    State(state): State<Arc<AppState>>,
    Path(dataset_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state
        .engine
        .close_dataset(&dataset_id)
        .map_err(ApiError::from_engine)?;
    Ok(StatusCode::NO_CONTENT)
}
