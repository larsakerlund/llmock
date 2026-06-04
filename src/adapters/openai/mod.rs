//! OpenAI protocol adapter: routes, request parsing, and response
//! serialization for the Chat Completions and Models endpoints.

pub(crate) mod error;
pub(crate) mod models;
pub(crate) mod request;
pub(crate) mod response;
pub(crate) mod sse;

use axum::extract::{Request, State};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};

use crate::cassette::Endpoint;
use crate::engine::{resolve, Resolution};
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
