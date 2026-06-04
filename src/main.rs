//! llmock — a fast, byte-faithful emulator of LLM provider HTTP APIs.
//!
//! Milestone 1: OpenAI Chat Completions (non-streaming) + the Models endpoints,
//! served against a YAML fixture set.

mod adapters;
mod cassette;
mod config;
mod core;
mod engine;
mod fixtures;
mod sse;
mod state;
mod stream;
mod util;

use std::net::SocketAddr;

use axum::routing::get;
use axum::{Json, Router};
use clap::Parser;

use cassette::{Cassettes, RecordConfig};
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

    if config.deterministic {
        util::enable_deterministic();
        tracing::info!("deterministic mode: ids and timestamps are reproducible");
    }

    // Optional record/replay cassettes, matched by the same engine as fixtures.
    if config.record && config.cassette_dir.is_none() {
        eprintln!("error: --record requires --cassette-dir");
        std::process::exit(1);
    }
    let mut state = AppState::new(fixtures, stream_defaults);
    if let Some(dir) = &config.cassette_dir {
        let store = Cassettes::load(dir).unwrap_or_else(|e| {
            eprintln!("error: {e}");
            std::process::exit(1);
        });
        tracing::info!(
            "loaded {} cassette(s) from {}{}",
            store.len(),
            dir.display(),
            if config.record { " (recording)" } else { "" }
        );
        let record = config.record.then(|| RecordConfig {
            dir: dir.clone(),
            upstream: config.upstream.clone(),
        });
        state = state.with_cassettes(store, record, config.replay_speed);
    }
    let app = build_app(state);

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

/// Assemble the full router over the given state. Each provider is mounted both
/// at the real root paths (drop-in: point an SDK's base URL straight at the
/// host) and under a `/{provider}` prefix (unambiguous for multi-provider
/// setups, e.g. base URL `http://host/openai`). Shared by `main` and the
/// in-process tests.
fn build_app(state: AppState) -> Router {
    // OpenAI spans two adapters (Chat Completions + Models, and Responses).
    let openai = || {
        Router::new()
            .merge(adapters::openai::router())
            .merge(adapters::openai_responses::router())
    };
    Router::new()
        .route("/healthz", get(healthz))
        // Root mounts.
        .merge(openai())
        .merge(adapters::anthropic::router())
        .merge(adapters::gemini::router())
        // Provider-prefixed aliases.
        .nest("/openai", openai())
        .nest("/anthropic", adapters::anthropic::router())
        .nest("/gemini", adapters::gemini::router())
        .with_state(state)
}

/// Liveness probe (not part of the emulated API surface).
async fn healthz() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}

#[cfg(test)]
mod cassette_tests;
#[cfg(test)]
mod wire_tests;
