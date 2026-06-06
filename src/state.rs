//! Shared application state handed to every request handler.

use std::sync::Arc;

use crate::cassette::{Cassettes, RecordConfig};
use crate::core::StreamDefaults;
use crate::fixtures::Fixtures;

/// Cheap to clone (just bumps `Arc`s), as axum requires of `State`.
#[derive(Clone)]
pub(crate) struct AppState {
    pub fixtures: Arc<Fixtures>,
    /// Streaming defaults (per-model, overridable) used when a fixture doesn't
    /// specify its own timing.
    pub stream_defaults: StreamDefaults,
    /// Cassettes loaded at startup, matched by the same engine as fixtures.
    pub cassettes: Arc<Cassettes>,
    /// Present when recording: proxy misses to the real upstream and save them.
    pub record: Option<RecordConfig>,
    /// HTTP client used for record-mode proxying.
    pub client: reqwest::Client,
    /// Factor applied to recorded stream timing on replay (1.0 = real timing,
    /// 0 = instant). See `--replay-speed`.
    pub replay_speed: f64,
    /// Maximum accepted request body size in bytes; a larger body gets a 413 in
    /// the requesting adapter's own error envelope. See `--max-body-bytes`.
    pub max_body_bytes: usize,
}

impl AppState {
    pub(crate) fn new(fixtures: Fixtures, stream_defaults: StreamDefaults) -> Self {
        AppState {
            fixtures: Arc::new(fixtures),
            stream_defaults,
            cassettes: Arc::new(Cassettes::default()),
            record: None,
            client: reqwest::Client::new(),
            replay_speed: 1.0,
            max_body_bytes: crate::config::DEFAULT_MAX_BODY_BYTES,
        }
    }

    pub(crate) fn with_cassettes(
        mut self,
        cassettes: Cassettes,
        record: Option<RecordConfig>,
        replay_speed: f64,
    ) -> Self {
        self.cassettes = Arc::new(cassettes);
        self.record = record;
        self.replay_speed = replay_speed;
        self
    }

    pub(crate) fn with_max_body_bytes(mut self, n: usize) -> Self {
        self.max_body_bytes = n;
        self
    }
}
