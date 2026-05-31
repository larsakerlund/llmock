//! llmock — a fast, byte-faithful emulator of LLM provider HTTP APIs.
//!
//! Milestone 1: OpenAI Chat Completions (non-streaming) + the Models endpoints,
//! served against a YAML fixture set.

mod adapters;
mod config;
mod core;
mod fixtures;
mod state;
mod stream;
mod util;

use std::net::SocketAddr;

use axum::routing::get;
use axum::{Json, Router};
use clap::Parser;

use config::Config;
use fixtures::Fixtures;
use state::AppState;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "llmock=info,tower_http=info".into()),
        )
        .init();

    let config = Config::parse();

    let fixtures = if let Some(path) = &config.fixtures {
        match Fixtures::load(path) {
            Ok(f) => {
                tracing::info!("loaded fixtures from {}", path.display());
                f
            }
            Err(e) => {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
    } else {
        tracing::info!("no fixtures file given; using built-in default fixture");
        Fixtures::builtin_default()
    };

    let stream_defaults = config.stream_defaults().unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(1);
    });

    let state = AppState::new(fixtures, stream_defaults);

    let app = Router::new()
        .route("/healthz", get(healthz))
        .merge(adapters::openai::router())
        .merge(adapters::openai_responses::router())
        .merge(adapters::anthropic::router())
        .with_state(state);

    let addr = SocketAddr::new(config.host, config.port);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|e| {
            eprintln!("error: cannot bind {addr}: {e}");
            std::process::exit(1);
        });

    tracing::info!("llmock listening on http://{addr}");
    axum::serve(listener, app).await.expect("server error");
}

/// Liveness probe (not part of the emulated API surface).
async fn healthz() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}
