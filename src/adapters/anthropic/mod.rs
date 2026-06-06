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

use axum::extract::{Request, State};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};

use crate::cassette::Endpoint;
use crate::engine::{Resolution, resolve};
use crate::state::AppState;
use crate::util;
use error::ApiError;
use request::MessagesRequest;
use response::{message_object, tool_use_ids};

pub(crate) fn router() -> Router<AppState> {
    Router::new().route("/v1/messages", post(messages))
}

async fn messages(State(state): State<AppState>, req: Request) -> Result<Response, ApiError> {
    let (parts, body) = req.into_parts();
    let bytes = axum::body::to_bytes(body, usize::MAX)
        .await
        .map_err(|_| ApiError::invalid_request("could not read request body"))?;
    let parsed: MessagesRequest = serde_json::from_slice(&bytes)
        .map_err(|e| ApiError::invalid_request(format!("invalid request body: {e}")))?;
    let neutral = parsed.into_neutral();

    let resolution = resolve(
        &state,
        Endpoint::Anthropic,
        &neutral,
        &parts.method,
        parts.uri.path(),
        parts.uri.query().unwrap_or(""),
        &bytes,
        &parts.headers,
    )
    .await;

    match resolution {
        Resolution::Raw(resp) => Ok(resp),
        Resolution::Synthesize(resp) => {
            if neutral.stream {
                Ok(sse::stream_response(&resp))
            } else {
                crate::stream::sleep_response_delay(&resp).await;
                let id = util::anthropic_message_id();
                let tool_ids = tool_use_ids(&resp);
                Ok(Json(message_object(&resp, &id, &tool_ids)).into_response())
            }
        }
        Resolution::Error(err) => Err(ApiError::from_inject(err)),
        Resolution::NoMatch => Err(ApiError::no_fixture(format!(
            "no fixture or cassette matched (model={:?}); add a fallback rule with an empty `match`",
            neutral.model
        ))),
    }
}
