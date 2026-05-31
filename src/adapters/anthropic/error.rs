//! Anthropic-shaped error envelopes: `{"type":"error","error":{"type","message"}}`.
//! Distinct from OpenAI's `{"error":{...}}`, so the genuine `anthropic` SDK
//! raises its matching typed exception.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

use crate::core::InjectError;

#[derive(Debug, Serialize)]
pub struct ErrorEnvelope {
    #[serde(rename = "type")]
    pub envelope_type: &'static str, // "error"
    pub error: ErrorBody,
}

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    #[serde(rename = "type")]
    pub error_type: String,
    pub message: String,
}

pub struct ApiError {
    pub status: StatusCode,
    pub body: ErrorEnvelope,
}

impl ApiError {
    pub fn new(
        status: StatusCode,
        error_type: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        ApiError {
            status,
            body: ErrorEnvelope {
                envelope_type: "error",
                error: ErrorBody {
                    error_type: error_type.into(),
                    message: message.into(),
                },
            },
        }
    }

    pub fn invalid_request(message: impl Into<String>) -> Self {
        ApiError::new(StatusCode::BAD_REQUEST, "invalid_request_error", message)
    }

    /// llmock could not find a fixture — flagged distinctly; not a real API error.
    pub fn no_fixture(message: impl Into<String>) -> Self {
        ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "llmock_no_fixture", message)
    }

    /// Render a developer-configured injected error into the Anthropic envelope.
    /// (`code`/`param` from the fixture are OpenAI-specific and ignored here.)
    pub fn from_inject(err: InjectError) -> Self {
        let status =
            StatusCode::from_u16(err.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        ApiError::new(status, err.error_type, err.message)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}
