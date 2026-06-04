//! Shared application state handed to every request handler.

use std::sync::Arc;

use crate::cassette::{Cassettes, RecordConfig};
use crate::core::StreamSpec;
use crate::fixtures::Fixtures;

/// Cheap to clone (just bumps `Arc`s), as axum requires of `State`.
#[derive(Clone)]
pub(crate) struct AppState {
    pub fixtures: Arc<Fixtures>,
    /// Streaming timing/granularity used when a fixture doesn't override it.
    pub stream_defaults: StreamSpec,
    /// Cassettes loaded at startup, matched by the same engine as fixtures.
    pub cassettes: Arc<Cassettes>,
    /// Present when recording: proxy misses to the real upstream and save them.
    pub record: Option<RecordConfig>,
    /// HTTP client used for record-mode proxying.
    pub client: reqwest::Client,
}

impl AppState {
    pub(crate) fn new(fixtures: Fixtures, stream_defaults: StreamSpec) -> Self {
        AppState {
            fixtures: Arc::new(fixtures),
            stream_defaults,
            cassettes: Arc::new(Cassettes::default()),
            record: None,
            client: reqwest::Client::new(),
        }
    }

    pub(crate) fn with_cassettes(
        mut self,
        cassettes: Cassettes,
        record: Option<RecordConfig>,
    ) -> Self {
        self.cassettes = Arc::new(cassettes);
        self.record = record;
        self
    }
}
