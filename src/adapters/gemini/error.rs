//! Gemini (Google API) error envelopes: `{"error":{"code","message","status"}}`.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

use crate::core::InjectError;

#[derive(Debug, Serialize)]
pub(crate) struct ErrorEnvelope {
    pub error: ErrorBody,
}

#[derive(Debug, Serialize)]
pub(crate) struct ErrorBody {
    pub code: u16,
    pub message: String,
    pub status: String,
}

pub(crate) struct ApiError {
    pub status: StatusCode,
    pub body: ErrorEnvelope,
}

impl ApiError {
    pub(crate) fn new(
        status: StatusCode,
        status_text: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        ApiError {
            status,
            body: ErrorEnvelope {
                error: ErrorBody {
                    code: status.as_u16(),
                    message: message.into(),
                    status: status_text.into(),
                },
            },
        }
    }

    pub(crate) fn invalid_request(message: impl Into<String>) -> Self {
        ApiError::new(StatusCode::BAD_REQUEST, "INVALID_ARGUMENT", message)
    }

    /// Request body exceeds the configured `--max-body-bytes` limit. This cap is
    /// a mock-only construct (the real Generative Language API rejects oversized
    /// input as HTTP 400 `INVALID_ARGUMENT` by token/byte limits, not a 413), but
    /// llmock returns 413 uniformly across adapters. `new` sets `code` from the
    /// status, so the body carries `code: 413`; the status string is
    /// `FAILED_PRECONDITION`, a canonical google.rpc code that does not contradict
    /// 413 the way `INVALID_ARGUMENT` (canonically 400) would.
    pub(crate) fn payload_too_large(message: impl Into<String>) -> Self {
        ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "FAILED_PRECONDITION",
            message,
        )
    }

    pub(crate) fn no_fixture(message: impl Into<String>) -> Self {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "LLMOCK_NO_FIXTURE",
            message,
        )
    }

    /// Render a developer-configured injected error into the Google envelope.
    /// The fixture's `type` becomes the `status` string (e.g. RESOURCE_EXHAUSTED);
    /// `code`/`param` are OpenAI-specific and ignored.
    pub(crate) fn from_inject(err: InjectError) -> Self {
        let status = StatusCode::from_u16(err.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        ApiError::new(status, err.error_type, err.message)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}
