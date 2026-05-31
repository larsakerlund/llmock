//! Small helpers for generating realistic-looking ids and timestamps.

use std::time::{SystemTime, UNIX_EPOCH};

use rand::distributions::Alphanumeric;
use rand::Rng;

/// Seconds since the Unix epoch, as the real APIs report in `created`.
pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn random_suffix(len: usize) -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}

/// `chatcmpl-…` id, matching OpenAI's shape.
pub fn completion_id() -> String {
    format!("chatcmpl-{}", random_suffix(29))
}

/// `fp_…` system fingerprint, matching OpenAI's shape.
pub fn system_fingerprint() -> String {
    format!("fp_{}", random_suffix(10))
}
