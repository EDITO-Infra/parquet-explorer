use std::{io, sync::Arc};

use axum::{
    Json,
    body::Body,
    extract::{Path, State},
    http::{HeaderValue, StatusCode, header},
    response::Response,
    routing::{delete, get, post},
    Router,
};
use bytes::Bytes;
use futures::{StreamExt, stream};
use tokio_util::io::ReaderStream;

use crate::{
    AppState,
    error::ApiError,
    model::{
        DatasetInfo, DatasetOpenRequest, ExportRequest, HealthResponse, PageRequest,
        SchemaResponse, SpatialRequest,
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
        .route("/datasets/{dataset_id}/page", post(page))
        .route("/datasets/{dataset_id}/spatial", post(spatial))
        .route("/datasets/{dataset_id}/export", post(export))
        .route("/datasets/{dataset_id}", delete(close_dataset))
        .with_state(state)
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        native_engine: format!("parquet-viewer-server/{}", env!("CARGO_PKG_VERSION")),
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

    let uri = validate_source_uri(&payload.uri, &state.settings)
        .await
        .map_err(|error| ApiError::bad_request(format!("{error:#}")))?;
    let info = state
        .engine
        .open_dataset(&uri, payload.name)
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

async fn page(
    State(state): State<Arc<AppState>>,
    Path(dataset_id): Path<String>,
    Json(mut payload): Json<PageRequest>,
) -> Result<Response, ApiError> {
    if !payload.sort.is_empty() {
        return Err(ApiError::not_implemented(
            "global sorting is reserved in the API contract but not implemented yet",
        ));
    }
    if payload.limit == 0 {
        return Err(ApiError::bad_request("limit must be at least 1"));
    }
    payload.limit = payload.limit.min(state.settings.max_page_size);

    let stream = state
        .engine
        .page_stream(&dataset_id, payload)
        .await
        .map_err(ApiError::from_engine)?;

    Ok(streaming_arrow_response(stream, None))
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

    let stream = state
        .engine
        .spatial_stream(&dataset_id, payload)
        .await
        .map_err(ApiError::from_engine)?;

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
    let guarded_stream = stream::unfold(
        (reader, artifact.path),
        |(mut reader, guard)| async move {
            match reader.next().await {
                Some(Ok(bytes)) => Some((Ok::<Bytes, io::Error>(bytes), (reader, guard))),
                Some(Err(error)) => Some((Err(error), (reader, guard))),
                None => None,
            }
        },
    );

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

