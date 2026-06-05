//! OpenAI protocol adapter. One vendor exposing several wire formats: Chat
//! Completions (`chat/`), the Responses API (`responses/`), and Models. They
//! share the vendor's error envelope (`error.rs`) and serve the same neutral
//! fixtures, so one rule can answer any of them.

pub(crate) mod chat;
pub(crate) mod error;
pub(crate) mod models;
pub(crate) mod responses;

use axum::Router;

use crate::state::AppState;

/// All OpenAI endpoints, mounted under `/v1` — one merged sub-router per wire
/// format the vendor exposes.
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .merge(chat::router())
        .merge(responses::router())
        .merge(models::router())
}
