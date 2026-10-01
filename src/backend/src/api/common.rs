use std::io;

use axum::{
    body::Body,
    http::{HeaderValue, StatusCode, header},
    response::Response,
};
use bytes::Bytes;

const ARROW_STREAM: &str = "application/vnd.apache.arrow.stream";

pub(super) fn streaming_arrow_response<S>(
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
        http::HeaderName::from_static("x-parquet-explorer-format"),
        HeaderValue::from_static("arrow-ipc-stream"),
    );
    if let Some((name, value)) = extra_header
        && let (Ok(name), Ok(value)) = (
            name.parse::<http::HeaderName>(),
            value.parse::<HeaderValue>(),
        )
    {
        response.headers_mut().insert(name, value);
    }
    response
}
