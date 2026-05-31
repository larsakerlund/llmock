//! Small helpers for generating realistic-looking ids and timestamps.
//!
//! In **deterministic mode** (enabled via [`enable_deterministic`]) ids use a
//! monotonic counter and timestamps are fixed, so output is byte-reproducible —
//! handy for snapshot tests (ours and the user's).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use rand::distributions::Alphanumeric;
use rand::Rng;

static DETERMINISTIC: AtomicBool = AtomicBool::new(false);
static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Fixed timestamp used in deterministic mode (2023-11-14T22:13:20Z).
const FIXED_TIME: u64 = 1_700_000_000;

/// Switch on deterministic ids/timestamps for the rest of the process.
pub(crate) fn enable_deterministic() {
    DETERMINISTIC.store(true, Ordering::Relaxed);
}

fn deterministic() -> bool {
    DETERMINISTIC.load(Ordering::Relaxed)
}

/// Seconds since the Unix epoch (fixed in deterministic mode).
pub(crate) fn unix_now() -> u64 {
    if deterministic() {
        return FIXED_TIME;
    }
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// A suffix of `len` characters: random normally, or a zero-padded monotonic
/// counter (so every id is unique and stable) in deterministic mode.
fn suffix(len: usize) -> String {
    if deterministic() {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let digits = format!("{n:0>len$}");
        // Keep exactly `len` chars even if the counter ever overflows the width.
        return digits[digits.len() - len..].to_string();
    }
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}

/// `chatcmpl-…` id, matching OpenAI's shape.
pub(crate) fn completion_id() -> String {
    format!("chatcmpl-{}", suffix(29))
}

/// `fp_…` system fingerprint, matching OpenAI's shape.
pub(crate) fn system_fingerprint() -> String {
    format!("fp_{}", suffix(10))
}

/// `call_…` tool-call id, matching OpenAI's shape.
pub(crate) fn tool_call_id() -> String {
    format!("call_{}", suffix(24))
}

/// `resp_…` Responses API response id.
pub(crate) fn response_id() -> String {
    format!("resp_{}", suffix(24))
}

/// `msg_…` Responses API output message item id.
pub(crate) fn message_item_id() -> String {
    format!("msg_{}", suffix(24))
}

/// `fc_…` Responses API function-call item id.
pub(crate) fn function_item_id() -> String {
    format!("fc_{}", suffix(24))
}

/// `msg_…` Anthropic Messages id.
pub(crate) fn anthropic_message_id() -> String {
    format!("msg_{}", suffix(24))
}

/// `toolu_…` Anthropic tool-use block id.
pub(crate) fn tool_use_id() -> String {
    format!("toolu_{}", suffix(24))
}
