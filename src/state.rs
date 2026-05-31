//! Shared application state handed to every request handler.

use std::sync::Arc;

use crate::core::StreamSpec;
use crate::fixtures::Fixtures;

/// Cheap to clone (just bumps an `Arc`), as axum requires of `State`.
#[derive(Clone)]
pub(crate) struct AppState {
    pub fixtures: Arc<Fixtures>,
    /// Streaming timing/granularity used when a fixture doesn't override it.
    pub stream_defaults: StreamSpec,
}

impl AppState {
    pub(crate) fn new(fixtures: Fixtures, stream_defaults: StreamSpec) -> Self {
        AppState {
            fixtures: Arc::new(fixtures),
            stream_defaults,
        }
    }
}
