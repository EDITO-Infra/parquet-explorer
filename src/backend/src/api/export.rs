use std::{io, sync::Arc};

use axum::{
    Json, Router,
    body::Body,
    extract::{Path, State},
    http::{HeaderValue, StatusCode, header},
    response::Response,
    routing::post,
};
use bytes::Bytes;
use futures::{StreamExt, stream};
use tokio_util::io::ReaderStream;

use crate::{AppState, error::ApiError, model::ExportRequest};

pub(super) fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/datasets/{dataset_id}/export", post(export))
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
