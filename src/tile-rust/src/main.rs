use axum::{
    extract::{Path, Query},
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use std::{env, net::SocketAddr};

#[derive(Debug, Deserialize)]
struct TileParams {
    dataset: String,
    geom_column: String,
    where_clause: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LegacyTileParams {
    dataset: String,
    geom_column: String,
    #[serde(rename = "where")]
    where_clause: Option<String>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,axum=info".into()),
        )
        .init();

    let app = Router::new()
        .route("/health", get(health))
        .route("/tiles/{z}/{x}/{y}.mvt", get(get_tile));

    let bind = env::var("TILE_BIND").unwrap_or_else(|_| "0.0.0.0:8090".to_string());
    let addr: SocketAddr = bind.parse().expect("TILE_BIND must be a valid socket address");

    tracing::info!(%addr, "starting rust tile service");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind TCP listener");

    axum::serve(listener, app)
        .await
        .expect("server exited with error");
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok", "service": "tile-rust"}))
}

async fn get_tile(
    Path((_z, _x, _y)): Path<(u32, u32, u32)>,
    query: Query<LegacyTileParams>,
) -> impl IntoResponse {
    let _normalized = TileParams {
        dataset: query.dataset.clone(),
        geom_column: query.geom_column.clone(),
        where_clause: query.where_clause.clone(),
    };

    // v1 placeholder implementation:
    // Return an empty MVT payload. This keeps the endpoint contract stable
    // while a full high-performance tile engine is implemented.
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        "application/vnd.mapbox-vector-tile".parse().expect("valid content-type"),
    );

    (StatusCode::OK, headers, Vec::<u8>::new())
}
