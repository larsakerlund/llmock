//! OpenAI Responses API adapter (`POST /v1/responses`).
//!
//! Reuses the neutral core, the fixture engine, latency, and fault injection
//! unchanged — only the request parsing and the streaming/serialization differ
//! from the Chat Completions adapter. One fixture rule therefore serves both
//! APIs.

pub(crate) mod request;
pub(crate) mod response;
pub(crate) mod sse;

use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};

use crate::adapters::openai::error::ApiError;
use crate::core::Outcome;
use crate::state::AppState;
use request::ResponsesRequest;
use response::{completed_response, ResponseIds};

pub(crate) fn router() -> Router<AppState> {
    Router::new().route("/v1/responses", post(responses))
}

async fn responses(
    State(state): State<AppState>,
    body: Result<Json<ResponsesRequest>, axum::extract::rejection::JsonRejection>,
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
        let ids = ResponseIds::for_response(&resp);
        let object = completed_response(&resp, &ids, crate::util::unix_now());
        Ok(Json(object).into_response())
    }
}
