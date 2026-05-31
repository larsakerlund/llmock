//! OpenAI protocol adapter: routes, request parsing, and response
//! serialization for the Chat Completions and Models endpoints.

pub(crate) mod error;
pub(crate) mod models;
pub(crate) mod request;
pub(crate) mod response;
pub(crate) mod sse;

use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};

use crate::core::Outcome;
use crate::state::AppState;
use error::ApiError;
use request::ChatCompletionRequest;
use response::ChatCompletion;

/// All OpenAI endpoints, mounted under `/v1`.
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/chat/completions", post(chat_completions))
        .route("/v1/models", get(models::list_models))
        .route("/v1/models/{model}", get(models::get_model))
}

async fn chat_completions(
    State(state): State<AppState>,
    body: Result<Json<ChatCompletionRequest>, axum::extract::rejection::JsonRejection>,
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
        // Upfront errors come back as a normal HTTP error, even for stream
        // requests (the real API errors before the stream starts).
        Outcome::Error(err) => return Err(ApiError::from_inject(err)),
        Outcome::Respond(resp) => resp,
    };

    if neutral.stream {
        Ok(sse::stream_response(&resp, neutral.include_usage))
    } else {
        Ok(Json(ChatCompletion::from_neutral(&resp)).into_response())
    }
}
