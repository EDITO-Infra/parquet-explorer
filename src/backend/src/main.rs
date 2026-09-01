//! Backend entry point and application wiring.
//!
//! `main` loads environment settings, constructs the shared native Parquet
//! engine, mounts the versioned HTTP API, applies CORS/request tracing, and
//! starts Axum with graceful shutdown handling.

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
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};
use tracing_subscriber::EnvFilter;

pub struct AppState {
    pub settings: Settings,
    pub engine: CoreEngine,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,tower_http=info")),
        )
        .json()
        .init();

    let settings = Settings::from_env()?;
    let state = Arc::new(AppState {
        engine: CoreEngine::new(
            settings.batch_size,
            settings.max_open_datasets,
            settings.dataset_idle_timeout_seconds,
        ),
        settings: settings.clone(),
    });
    spawn_dataset_handle_cleanup(Arc::clone(&state));

    let cors = if settings.cors_origins.iter().any(|origin| origin == "*") {
        CorsLayer::new()
            .allow_origin(Any)
            .allow_headers(Any)
            .allow_methods(Any)
    } else {
        let origins = settings
            .cors_origins
            .iter()
            .map(|origin| origin.parse())
            .collect::<Result<Vec<axum::http::HeaderValue>, _>>()?;
        CorsLayer::new()
            .allow_origin(origins)
            .allow_headers(Any)
            .allow_methods(Any)
    };

    let app = Router::new()
        .nest("/api/v1", api::router(state))
        .layer(cors)
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(settings.bind).await?;
    tracing::info!(address = %settings.bind, "parquet-viewer-backend listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

/// Periodically remove abandoned in-memory dataset handles.
///
/// Exact expiration is also checked synchronously whenever a handle is used.
/// This task is therefore housekeeping rather than part of request correctness:
/// it frees handles left behind by closed/crashed browser sessions even when no
/// subsequent request references those IDs.
fn spawn_dataset_handle_cleanup(state: Arc<AppState>) {
    let Some(interval) = state.engine.dataset_cleanup_interval() else {
        tracing::info!("dataset handle idle expiration is disabled");
        return;
    };

    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        // `interval` ticks immediately once. Consume that tick so cleanup begins
        // after one interval instead of immediately after startup.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let removed = state.engine.prune_expired_datasets();
            if removed > 0 {
                tracing::info!(removed, "pruned expired dataset handles");
            }
        }
    });
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{SignalKind, signal};
        if let Ok(mut stream) = signal(SignalKind::terminate()) {
            stream.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = terminate => {} }
}
