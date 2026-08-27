mod api;
mod config;
mod engine;
mod error;
mod model;
mod security;

use std::sync::Arc;

use anyhow::Result;
use axum::Router;
use config::Settings;
use engine::CoreEngine;
use tower_http::{cors::{Any, CorsLayer}, trace::TraceLayer};
use tracing_subscriber::EnvFilter;

pub struct AppState {
    pub settings: Settings,
    pub engine: CoreEngine,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,tower_http=info")))
        .json()
        .init();

    let settings = Settings::from_env()?;
    let state = Arc::new(AppState {
        engine: CoreEngine::new(settings.batch_size, settings.max_open_datasets),
        settings: settings.clone(),
    });

    let cors = if settings.cors_origins.iter().any(|origin| origin == "*") {
        CorsLayer::new().allow_origin(Any).allow_headers(Any).allow_methods(Any)
    } else {
        let origins = settings.cors_origins.iter()
            .map(|origin| origin.parse())
            .collect::<Result<Vec<axum::http::HeaderValue>, _>>()?;
        CorsLayer::new().allow_origin(origins).allow_headers(Any).allow_methods(Any)
    };

    let app = Router::new()
        .nest("/api/v1", api::router(state))
        .layer(cors)
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(settings.bind).await?;
    tracing::info!(address = %settings.bind, "parquet-viewer-server listening");
    axum::serve(listener, app).with_graceful_shutdown(shutdown_signal()).await?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async { let _ = tokio::signal::ctrl_c().await; };
    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{signal, SignalKind};
        if let Ok(mut stream) = signal(SignalKind::terminate()) { stream.recv().await; }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = terminate => {} }
}
