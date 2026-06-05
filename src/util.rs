//! Small helpers for generating realistic-looking ids and timestamps.
//!
//! In **deterministic mode** (enabled via [`enable_deterministic`]) ids use a
//! monotonic counter and timestamps are fixed, so output is byte-reproducible —
//! handy for snapshot tests (ours and the user's).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use rand::Rng;
use rand::distributions::Alphanumeric;

static DETERMINISTIC: AtomicBool = AtomicBool::new(false);
static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Fixed timestamp used in deterministic mode (2023-11-14T22:13:20Z).
const FIXED_TIME: u64 = 1_700_000_000;

/// Switch on deterministic ids/timestamps for the rest of the process.
pub(crate) fn enable_deterministic() {
    DETERMINISTIC.store(true, Ordering::Relaxed);
}

pub(crate) fn is_deterministic() -> bool {
    DETERMINISTIC.load(Ordering::Relaxed)
}

/// Seconds since the Unix epoch (fixed in deterministic mode).
pub(crate) fn unix_now() -> u64 {
    if is_deterministic() {
        return FIXED_TIME;
    }
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// A suffix of `len` characters: random normally, or a zero-padded monotonic
/// counter (so every id is unique and stable) in deterministic mode.
fn suffix(len: usize) -> String {
    if is_deterministic() {
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

/// An id of the form `<prefix><random suffix of `len` chars>`. The prefix
/// carries its own separator (e.g. `"chatcmpl-"`, `"fp_"`).
fn id(prefix: &str, len: usize) -> String {
    format!("{prefix}{}", suffix(len))
}

/// `chatcmpl-…` id, matching OpenAI's shape.
pub(crate) fn completion_id() -> String {
    id("chatcmpl-", 29)
}

/// `fp_…` system fingerprint, matching OpenAI's shape.
pub(crate) fn system_fingerprint() -> String {
    id("fp_", 10)
}

/// `call_…` tool-call id, matching OpenAI's shape.
pub(crate) fn tool_call_id() -> String {
    id("call_", 24)
}

/// `resp_…` Responses API response id.
pub(crate) fn response_id() -> String {
    id("resp_", 24)
}

/// `msg_…` Responses API output message item id.
pub(crate) fn message_item_id() -> String {
    id("msg_", 24)
}

/// `fc_…` Responses API function-call item id.
pub(crate) fn function_item_id() -> String {
    id("fc_", 24)
}

/// `msg_…` Anthropic Messages id.
pub(crate) fn anthropic_message_id() -> String {
    id("msg_", 24)
}

/// `toolu_…` Anthropic tool-use block id.
pub(crate) fn tool_use_id() -> String {
    id("toolu_", 24)
}
