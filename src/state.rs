//! Shared application state handed to every request handler.

use std::sync::Arc;

use crate::fixtures::Fixtures;

/// Cheap to clone (just bumps an `Arc`), as axum requires of `State`.
#[derive(Clone)]
pub struct AppState {
    pub fixtures: Arc<Fixtures>,
}

impl AppState {
    pub fn new(fixtures: Fixtures) -> Self {
        AppState {
            fixtures: Arc::new(fixtures),
        }
    }
}
