//! HTTP transport layer for `/api/v1`.
//!
//! Handlers in this file validate request-shape concerns and translate engine
//! results into JSON, Arrow IPC streams, or downloadable files. Parquet logic
//! belongs in `engine`; keeping handlers thin makes the engine reusable.

use std::{io, sync::Arc};

use axum::{
    Json, Router,
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderValue, StatusCode, header},
    response::Response,
    routing::{delete, get, post},
};
use bytes::Bytes;
use futures::{StreamExt, stream};
use tokio_util::io::ReaderStream;

use crate::{
    AppState,
    error::ApiError,
    model::{
        AnalysisRecommendationsResponse, AnalysisSummaryResponse, ColumnsAnalysisResponse,
        CountRequest, CountResponse, DatasetInfo, DatasetOpenRequest, ExportRequest,
        HealthResponse, PageRequest, PagesAnalysisQuery, PagesAnalysisResponse, QueryCostRequest,
        QueryCostResponse, RowGroupsAnalysisResponse, SchemaResponse, SnapshotRequest,
        SpatialRequest,
    },
    security::validate_source_uri,
};

const ARROW_STREAM: &str = "application/vnd.apache.arrow.stream";

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/datasets/open", post(open_dataset))
        .route("/datasets/{dataset_id}/metadata", get(metadata))
        .route("/datasets/{dataset_id}/schema", get(schema))
        // Read-only physical-layout analysis. These routes describe the source
        // file; they never rewrite, move, upload, or otherwise manage it.
        .route(
            "/datasets/{dataset_id}/analysis/summary",
            get(analysis_summary),
        )
        .route(
            "/datasets/{dataset_id}/analysis/columns",
            get(analysis_columns),
        )
        .route(
            "/datasets/{dataset_id}/analysis/row-groups",
            get(analysis_row_groups),
        )
        .route("/datasets/{dataset_id}/analysis/pages", get(analysis_pages))
        .route(
            "/datasets/{dataset_id}/analysis/query-cost",
            post(analysis_query_cost),
        )
        .route(
            "/datasets/{dataset_id}/analysis/recommendations",
            get(analysis_recommendations),
        )
        .route("/datasets/{dataset_id}/page", post(page))
        .route("/datasets/{dataset_id}/count", post(count))
        .route("/datasets/{dataset_id}/snapshot", post(snapshot))
        .route("/datasets/{dataset_id}/spatial", post(spatial))
        .route("/datasets/{dataset_id}/export", post(export))
        .route("/diagnostics/{trace_id}", get(diagnostics))
        .route("/datasets/{dataset_id}", delete(close_dataset))
        .with_state(state)
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        native_engine: format!("parquet-viewer-backend/{}", env!("CARGO_PKG_VERSION")),
    })
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

/// Return a cheap, footer-derived summary of the file's physical layout.
async fn analysis_summary(
    State(state): State<Arc<AppState>>,
    Path(dataset_id): Path<String>,
) -> Result<Json<AnalysisSummaryResponse>, ApiError> {
    state
        .engine
        .analysis_summary(&dataset_id)
        .await
        .map(Json)
        .map_err(ApiError::from_engine)
}

/// Aggregate compressed storage and statistics coverage by Parquet leaf column.
async fn analysis_columns(
    State(state): State<Arc<AppState>>,
    Path(dataset_id): Path<String>,
) -> Result<Json<ColumnsAnalysisResponse>, ApiError> {
    state
        .engine
        .analysis_columns(&dataset_id)
        .await
        .map(Json)
        .map_err(ApiError::from_engine)
}

/// Return row-group and column-chunk layout without reading data pages.
async fn analysis_row_groups(
    State(state): State<Arc<AppState>>,
    Path(dataset_id): Path<String>,
) -> Result<Json<RowGroupsAnalysisResponse>, ApiError> {
    state
        .engine
        .analysis_row_groups(&dataset_id)
        .await
        .map(Json)
        .map_err(ApiError::from_engine)
}

/// Load optional Parquet page indexes for deeper page-level inspection.
///
/// Query parameters are optional: `row_group=0&column=geometry` can be used to
/// keep the response small for large files.
async fn analysis_pages(
    State(state): State<Arc<AppState>>,
    Path(dataset_id): Path<String>,
    Query(query): Query<PagesAnalysisQuery>,
) -> Result<Json<PagesAnalysisResponse>, ApiError> {
    state
        .engine
        .analysis_pages(&dataset_id, query)
        .await
        .map(Json)
        .map_err(ApiError::from_engine)
}

/// Estimate compressed bytes a row query is likely to touch without executing it.
async fn analysis_query_cost(
    State(state): State<Arc<AppState>>,
    Path(dataset_id): Path<String>,
    Json(payload): Json<QueryCostRequest>,
) -> Result<Json<QueryCostResponse>, ApiError> {
    if payload.limit == 0 {
        return Err(ApiError::bad_request("limit must be at least 1"));
    }
    state
        .engine
        .analysis_query_cost(&dataset_id, payload)
        .await
        .map(Json)
        .map_err(ApiError::from_engine)
}

/// Return read-only findings for characteristics that affect interactive access.
async fn analysis_recommendations(
    State(state): State<Arc<AppState>>,
    Path(dataset_id): Path<String>,
) -> Result<Json<AnalysisRecommendationsResponse>, ApiError> {
    state
        .engine
        .analysis_recommendations(&dataset_id)
        .await
        .map(Json)
        .map_err(ApiError::from_engine)
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

async fn snapshot(
    State(state): State<Arc<AppState>>,
    Path(dataset_id): Path<String>,
    Json(payload): Json<SnapshotRequest>,
) -> Result<Response, ApiError> {
    let trace_id = payload.trace_id.clone();
    let stream = match state.engine.snapshot_stream(&dataset_id, payload).await {
        Ok(stream) => stream,
        Err(error) => {
            state
                .engine
                .trace_fail(trace_id.as_deref(), "Map data query failed");
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

async fn export(
    State(state): State<Arc<AppState>>,
    Path(dataset_id): Path<String>,
    Json(payload): Json<ExportRequest>,
) -> Result<Response, ApiError> {
    if payload.limit == Some(0) {
        return Err(ApiError::bad_request("limit must be at least 1"));
    }
    if payload.spatial_bbox.is_some() {
        return Err(ApiError::not_implemented(
            "spatial export is disabled until exact feature-level GeoArrow/GeoRust filtering is implemented; /spatial currently returns conservative row-group candidates",
        ));
    }

    let artifact = state
        .engine
        .export(&dataset_id, payload)
        .await
        .map_err(ApiError::from_engine)?;
    let file = tokio::fs::File::open(&artifact.path)
        .await
        .map_err(|error| ApiError::internal(format!("could not open export: {error}")))?;

    // Hold TempPath inside the stream state. It deletes the temporary export as
    // soon as the HTTP body reaches EOF or is dropped after a disconnect.
    let reader = ReaderStream::new(file);
    let guarded_stream =
        stream::unfold((reader, artifact.path), |(mut reader, guard)| async move {
            match reader.next().await {
                Some(Ok(bytes)) => Some((Ok::<Bytes, io::Error>(bytes), (reader, guard))),
                Some(Err(error)) => Some((Err(error), (reader, guard))),
                None => None,
            }
        });

    let mut response = Response::new(Body::from_stream(guarded_stream));
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(artifact.content_type),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        format!("attachment; filename=\"{}\"", artifact.filename)
            .parse::<HeaderValue>()
            .map_err(|error| ApiError::internal(format!("invalid export filename: {error}")))?,
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
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

fn streaming_arrow_response<S>(
    stream: S,
    extra_header: Option<(&'static str, &'static str)>,
) -> Response
where
    S: futures::Stream<Item = Result<Bytes, io::Error>> + Send + 'static,
{
    let mut response = Response::new(Body::from_stream(stream));
    *response.status_mut() = StatusCode::OK;
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(ARROW_STREAM));
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        http::HeaderName::from_static("x-parquet-viewer-format"),
        HeaderValue::from_static("arrow-ipc-stream"),
    );
    if let Some((name, value)) = extra_header {
        if let (Ok(name), Ok(value)) = (
            name.parse::<http::HeaderName>(),
            value.parse::<HeaderValue>(),
        ) {
            response.headers_mut().insert(name, value);
        }
    }
    response
}
