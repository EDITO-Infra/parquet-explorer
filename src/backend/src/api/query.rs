use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    response::Response,
    routing::post,
};

use crate::{
    AppState,
    error::ApiError,
    model::{CountRequest, CountResponse, PageRequest, ResultRequest},
};

use super::common::streaming_arrow_response;

pub(super) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/datasets/{dataset_id}/page", post(page))
        .route("/datasets/{dataset_id}/count", post(count))
        .route("/datasets/{dataset_id}/result", post(result))
}

async fn page(
    State(state): State<Arc<AppState>>,
    Path(dataset_id): Path<String>,
    Json(payload): Json<PageRequest>,
) -> Result<Response, ApiError> {
    if !payload.sort.is_empty() {
        return Err(ApiError::not_implemented(
            "global sorting is reserved in the API contract but not implemented yet",
        ));
    }
    if payload.limit == 0 {
        return Err(ApiError::bad_request("limit must be at least 1"));
    }
    let max_page_size = state.settings.max_page_size.min(10_000);
    if payload.limit > max_page_size {
        return Err(ApiError::bad_request(format!(
            "limit must not exceed {max_page_size} rows per page"
        )));
    }

    let trace_id = payload.trace_id.clone();
    let stream = match state.engine.page_stream(&dataset_id, payload).await {
        Ok(stream) => stream,
        Err(error) => {
            state
                .engine
                .trace_fail(trace_id.as_deref(), "Data query failed");
            return Err(ApiError::from_engine(error));
        }
    };

    Ok(streaming_arrow_response(stream, None))
}

async fn result(
    State(state): State<Arc<AppState>>,
    Path(dataset_id): Path<String>,
    Json(payload): Json<ResultRequest>,
) -> Result<Response, ApiError> {
    let trace_id = payload.trace_id.clone();
    let stream = match state.engine.result_stream(&dataset_id, payload).await {
        Ok(stream) => stream,
        Err(error) => {
            state
                .engine
                .trace_fail(trace_id.as_deref(), "Complete result query failed");
            return Err(ApiError::from_engine(error));
        }
    };

    Ok(streaming_arrow_response(stream, None))
}

async fn count(
    State(state): State<Arc<AppState>>,
    Path(dataset_id): Path<String>,
    Json(payload): Json<CountRequest>,
) -> Result<Json<CountResponse>, ApiError> {
    let trace_id = payload.trace_id.clone();
    let count = match state.engine.count_rows(&dataset_id, payload).await {
        Ok(count) => count,
        Err(error) => {
            state
                .engine
                .trace_fail(trace_id.as_deref(), "Count query failed");
            return Err(ApiError::from_engine(error));
        }
    };
    Ok(Json(CountResponse { count }))
}
