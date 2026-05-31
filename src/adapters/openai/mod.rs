//! OpenAI protocol adapter: routes, request parsing, and response
//! serialization for the Chat Completions and Models endpoints.

pub mod error;
pub mod models;
pub mod request;
pub mod response;

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};

use crate::state::AppState;
use error::ApiError;
use request::ChatCompletionRequest;
use response::ChatCompletion;

/// All OpenAI endpoints, mounted under `/v1`.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/chat/completions", post(chat_completions))
        .route("/v1/models", get(models::list_models))
        .route("/v1/models/{model}", get(models::get_model))
}

async fn chat_completions(
    State(state): State<AppState>,
    body: Result<Json<ChatCompletionRequest>, axum::extract::rejection::JsonRejection>,
) -> Result<Json<ChatCompletion>, ApiError> {
    let Json(req) = body.map_err(|e| ApiError::invalid_request(e.body_text()))?;
    let neutral = req.into_neutral();

    if neutral.stream {
        // Streaming lands in the next milestone; be honest until then.
        return Err(ApiError::not_implemented(
            "streaming responses are not implemented yet",
        ));
    }

    let resp = state.fixtures.respond_to(&neutral).ok_or_else(|| {
        ApiError::no_fixture(format!(
            "no fixture rule matched (model={:?}); add a fallback rule with an empty `match`",
            neutral.model
        ))
    })?;

    Ok(Json(ChatCompletion::from_neutral(&resp)))
}

