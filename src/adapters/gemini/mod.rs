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

use axum::extract::{Path, Request, State};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};

use crate::cassette::Endpoint;
use crate::engine::{Resolution, resolve};
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
    req: Request,
) -> Result<Response, ApiError> {
    let (parts, body) = req.into_parts();
    let bytes = axum::body::to_bytes(body, state.max_body_bytes)
        .await
        .map_err(|_| ApiError::payload_too_large("request body exceeds the configured limit"))?;
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
            )));
        }
    };

    let parsed: GenerateRequest = serde_json::from_slice(&bytes)
        .map_err(|e| ApiError::invalid_request(format!("invalid request body: {e}")))?;
    let neutral = parsed.into_neutral(model.to_string(), stream);

    let resolution = resolve(
        &state,
        Endpoint::Gemini,
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
                Ok(Json(generate_response(&resp)).into_response())
            }
        }
        Resolution::Error(err) => Err(ApiError::from_inject(err)),
        Resolution::NoMatch => Err(ApiError::no_fixture(format!(
            "no fixture or cassette matched (model={:?}); add a fallback rule with an empty `match`",
            neutral.model
        ))),
    }
}
