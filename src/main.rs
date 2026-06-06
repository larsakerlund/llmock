//! llmock — a fast, byte-faithful emulator of LLM provider HTTP APIs.
//!
//! Serves canned fixture responses, or replayed cassettes, that match the
//! OpenAI, Anthropic, and Google Gemini wire formats against a YAML fixture set.
//! This module wires the server: routing, shared state, and startup.

mod adapters;
mod cassette;
mod config;
mod core;
mod engine;
mod fixtures;
mod sse;
mod state;
mod stream;
mod tokenize;
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
        let f = exit_on_error(Fixtures::load(path));
        tracing::info!("loaded fixtures from {}", path.display());
        f
    } else {
        tracing::info!("no fixtures file given; using built-in default fixture");
        Fixtures::builtin_default()
    };

    let stream_defaults = exit_on_error(config.stream_defaults());

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
        let store = exit_on_error(Cassettes::load(dir));
        tracing::info!(
            "loaded {} cassette(s) from {}{}",
            store.len(),
            dir.display(),
            if config.record { " (recording)" } else { "" }
        );
        let record = config.record.then(|| RecordConfig {
            dir: dir.clone(),
            upstream: config.upstream.clone(),
            upstream_openai: config.upstream_openai.clone(),
            upstream_anthropic: config.upstream_anthropic.clone(),
            upstream_gemini: config.upstream_gemini.clone(),
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

/// Unwrap a startup `Result`, or print the error to stderr and exit non-zero.
/// Centralises the `error: {e}` / exit(1) pattern used at the init sites.
fn exit_on_error<T>(r: Result<T, String>) -> T {
    r.unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(1)
    })
}

/// Assemble the full router over the given state. Each provider is served only
/// under its `/{provider}` prefix (e.g. base URL `http://host/openai`); root no
/// longer carries any provider APIs. We emulate many providers that share the
/// `/v1/` namespace (OpenAI and Anthropic both define `/v1/models`, etc.), so
/// mounting them all at root would collide; the prefix makes routing
/// unambiguous. Shared by `main` and the in-process tests.
fn build_app(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        // Provider-prefixed mounts.
        .nest("/openai", adapters::openai::router())
        .nest("/anthropic", adapters::anthropic::router())
        .nest("/gemini", adapters::gemini::router())
        .with_state(state)
}

/// Liveness probe (not part of the emulated API surface).
async fn healthz() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}

#[cfg(test)]
mod tests;
