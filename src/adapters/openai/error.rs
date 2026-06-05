//! OpenAI-shaped error envelopes. The real SDKs branch on `error.type` and the
//! HTTP status, so both must be faithful.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

use crate::core::InjectError;

#[derive(Debug, Serialize)]
pub(crate) struct ErrorEnvelope {
    pub error: ErrorBody,
}

#[derive(Debug, Serialize)]
pub(crate) struct ErrorBody {
    pub message: String,
    #[serde(rename = "type")]
    pub error_type: String,
    pub param: Option<String>,
    pub code: Option<String>,
}

/// An error ready to be returned from a handler with the right status + body.
pub(crate) struct ApiError {
    pub status: StatusCode,
    pub body: ErrorEnvelope,
}

impl ApiError {
    pub(crate) fn new(
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
    pub(crate) fn invalid_request(message: impl Into<String>) -> Self {
        ApiError::new(StatusCode::BAD_REQUEST, "invalid_request_error", message)
    }

    /// llmock could not find a fixture for this request — a mock-config problem,
    /// not something the real API produces. Flagged with a distinct type so it
    /// is obvious in test output.
    pub(crate) fn no_fixture(message: impl Into<String>) -> Self {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "llmock_no_fixture",
            message,
        )
    }

    /// Render a developer-configured injected error into the OpenAI envelope.
    /// An unknown/invalid status falls back to 500.
    pub(crate) fn from_inject(err: InjectError) -> Self {
        let status = StatusCode::from_u16(err.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        ApiError {
            status,
            body: ErrorEnvelope {
                error: ErrorBody {
                    message: err.message,
                    error_type: err.error_type,
                    param: err.param,
                    code: err.code,
                },
            },
        }
    }

    /// A 501 response for a capability llmock does not implement. Currently
    /// unused; kept so an endpoint can signal not-implemented.
    #[allow(dead_code)]
    pub(crate) fn not_implemented(message: impl Into<String>) -> Self {
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
