//! OpenAI Responses API adapter (`POST /v1/responses`).
//!
//! Reuses the neutral core, the fixture engine, latency, and fault injection
//! unchanged — only the request parsing and the streaming/serialization differ
//! from the Chat Completions adapter. One fixture rule therefore serves both
//! APIs.

pub(crate) mod request;
pub(crate) mod response;
pub(crate) mod sse;

use axum::extract::{Request, State};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};

use super::error::ApiError;
use crate::cassette::Endpoint;
use crate::engine::{Resolution, resolve};
use crate::state::AppState;
use crate::util;
use request::ResponsesRequest;
use response::{ResponseIds, completed_response};

pub(crate) fn router() -> Router<AppState> {
    Router::new().route("/v1/responses", post(responses))
}

async fn responses(State(state): State<AppState>, req: Request) -> Result<Response, ApiError> {
    let (parts, body) = req.into_parts();
    let bytes = axum::body::to_bytes(body, usize::MAX)
        .await
        .map_err(|_| ApiError::invalid_request("could not read request body"))?;
    let parsed: ResponsesRequest = serde_json::from_slice(&bytes)
        .map_err(|e| ApiError::invalid_request(format!("invalid request body: {e}")))?;
    let neutral = parsed.into_neutral();

    let resolution = resolve(
        &state,
        Endpoint::OpenAiResponses,
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
                let ids = ResponseIds::for_response(&resp);
                let object = completed_response(&resp, &ids, util::unix_now());
                Ok(Json(object).into_response())
            }
        }
        Resolution::Error(err) => Err(ApiError::from_inject(err)),
        Resolution::NoMatch => Err(ApiError::no_fixture(format!(
            "no fixture or cassette matched (model={:?}); add a fallback rule with an empty `match`",
            neutral.model
        ))),
    }
}
