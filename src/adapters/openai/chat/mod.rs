//! OpenAI Chat Completions adapter (`POST /v1/chat/completions`): request
//! parsing and response/stream serialization for the `chat.completion` wire
//! format.

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
use request::ChatCompletionRequest;
use response::ChatCompletion;

pub(crate) fn router() -> Router<AppState> {
    Router::new().route("/v1/chat/completions", post(chat_completions))
}

async fn chat_completions(
    State(state): State<AppState>,
    req: Request,
) -> Result<Response, ApiError> {
    let (parts, body) = req.into_parts();
    let bytes = axum::body::to_bytes(body, usize::MAX)
        .await
        .map_err(|_| ApiError::invalid_request("could not read request body"))?;
    let parsed: ChatCompletionRequest = serde_json::from_slice(&bytes)
        .map_err(|e| ApiError::invalid_request(format!("invalid request body: {e}")))?;
    let neutral = parsed.into_neutral();

    let resolution = resolve(
        &state,
        Endpoint::OpenAiChat,
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
                Ok(sse::stream_response(&resp, neutral.include_usage))
            } else {
                Ok(Json(ChatCompletion::from_neutral(&resp)).into_response())
            }
        }
        Resolution::Error(err) => Err(ApiError::from_inject(err)),
        Resolution::NoMatch => Err(ApiError::no_fixture(format!(
            "no fixture or cassette matched (model={:?}); add a fallback rule with an empty `match`",
            neutral.model
        ))),
    }
}
