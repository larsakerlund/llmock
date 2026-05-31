//! Google Gemini API adapter.
//!
//! The Gemini REST surface encodes the action in the URL as
//! `/v1beta/models/{model}:generateContent` (and `:streamGenerateContent` for
//! streaming), so the model and the streaming flag come from the path, not the
//! body. A third vendor, reusing the neutral core and fixture engine unchanged.

pub(crate) mod error;
pub(crate) mod request;
pub(crate) mod response;
pub(crate) mod sse;

use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};

use crate::core::Outcome;
use crate::state::AppState;
use error::ApiError;
use request::GenerateRequest;
use response::generate_response;

pub(crate) fn router() -> Router<AppState> {
    // `{model_action}` captures e.g. `gemini-2.0-flash:generateContent`.
    Router::new().route("/v1beta/models/{model_action}", post(generate))
}

async fn generate(
    State(state): State<AppState>,
    Path(model_action): Path<String>,
    body: Result<Json<GenerateRequest>, axum::extract::rejection::JsonRejection>,
) -> Result<Response, ApiError> {
    // Split "<model>:<action>" — action selects streaming vs not.
    let (model, action) = model_action.rsplit_once(':').ok_or_else(|| {
        ApiError::invalid_request(format!(
            "expected /v1beta/models/<model>:generateContent, got {model_action:?}"
        ))
    })?;
    let stream = match action {
        "generateContent" => false,
        "streamGenerateContent" => true,
        other => {
            return Err(ApiError::invalid_request(format!(
                "unknown action {other:?} (expected generateContent or streamGenerateContent)"
            )))
        }
    };

    let Json(req) = body.map_err(|e| ApiError::invalid_request(e.body_text()))?;
    let neutral = req.into_neutral(model.to_string(), stream);

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
        Ok(Json(generate_response(&resp)).into_response())
    }
}
