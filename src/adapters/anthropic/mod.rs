//! Anthropic Messages API adapter (`POST /v1/messages`).
//!
//! A second *vendor* (not just a second OpenAI surface): different request
//! shape, different streaming event model, and a different error envelope and
//! auth scheme (`x-api-key` + `anthropic-version`, which llmock accepts and
//! ignores). It reuses the neutral core, fixture engine, latency, and fault
//! injection unchanged — so one fixture serves OpenAI and Anthropic alike.

pub(crate) mod error;
pub(crate) mod request;
pub(crate) mod response;
pub(crate) mod sse;

use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};

use crate::core::Outcome;
use crate::state::AppState;
use crate::util;
use error::ApiError;
use request::MessagesRequest;
use response::{message_object, tool_use_ids};

pub(crate) fn router() -> Router<AppState> {
    Router::new().route("/v1/messages", post(messages))
}

async fn messages(
    State(state): State<AppState>,
    body: Result<Json<MessagesRequest>, axum::extract::rejection::JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(req) = body.map_err(|e| ApiError::invalid_request(e.body_text()))?;
    let neutral = req.into_neutral();

    let outcome = state
        .fixtures
        .outcome_for(&neutral, state.stream_defaults)
        .ok_or_else(|| {
            ApiError::no_fixture(format!(
                "no fixture rule matched (model={:?}); add a fallback rule with an empty `match`",
                neutral.model
            ))
        })?;

    let resp = match outcome {
        Outcome::Error(err) => return Err(ApiError::from_inject(err)),
        Outcome::Respond(resp) => resp,
    };

    if neutral.stream {
        Ok(sse::stream_response(&resp))
    } else {
        let id = util::anthropic_message_id();
        let tool_ids = tool_use_ids(&resp);
        Ok(Json(message_object(&resp, &id, &tool_ids)).into_response())
    }
}
