//! OpenAI-shaped error envelopes. The real SDKs branch on `error.type` and the
//! HTTP status, so both must be faithful.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ErrorEnvelope {
    pub error: ErrorBody,
}

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub message: String,
    #[serde(rename = "type")]
    pub error_type: String,
    pub param: Option<String>,
    pub code: Option<String>,
}

/// An error ready to be returned from a handler with the right status + body.
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
                error: ErrorBody {
                    message: message.into(),
                    error_type: error_type.into(),
                    param: None,
                    code: None,
                },
            },
        }
    }

    /// Malformed request body — what the real API returns for bad JSON / fields.
    pub fn invalid_request(message: impl Into<String>) -> Self {
        ApiError::new(StatusCode::BAD_REQUEST, "invalid_request_error", message)
    }

    /// llmock could not find a fixture for this request — a mock-config problem,
    /// not something the real API produces. Flagged with a distinct type so it
    /// is obvious in test output.
    pub fn no_fixture(message: impl Into<String>) -> Self {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "llmock_no_fixture",
            message,
        )
    }

    /// A capability llmock has not implemented yet.
    pub fn not_implemented(message: impl Into<String>) -> Self {
        ApiError::new(
            StatusCode::NOT_IMPLEMENTED,
            "llmock_not_implemented",
            message,
        )
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}
