use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    response::Response,
    routing::post,
};

use crate::{AppState, error::ApiError, model::SpatialRequest};

use super::common::streaming_arrow_response;

pub(super) fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/datasets/{dataset_id}/spatial", post(spatial))
}

async fn spatial(
    State(state): State<Arc<AppState>>,
    Path(dataset_id): Path<String>,
    Json(mut payload): Json<SpatialRequest>,
) -> Result<Response, ApiError> {
    if payload.max_features == 0 {
        return Err(ApiError::bad_request("max_features must be at least 1"));
    }
    payload.max_features = payload
        .max_features
        .min(state.settings.max_spatial_features);

    let trace_id = payload.trace_id.clone();
    let stream = match state.engine.spatial_stream(&dataset_id, payload).await {
        Ok(stream) => stream,
        Err(error) => {
            state
                .engine
                .trace_fail(trace_id.as_deref(), "Spatial query failed");
            return Err(ApiError::from_engine(error));
        }
    };

    Ok(streaming_arrow_response(
        stream,
        Some((
            "x-parquet-viewer-spatial-filter",
            "row-group-bbox-candidates",
        )),
    ))
}
