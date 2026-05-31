//! Small helpers for generating realistic-looking ids and timestamps.

use std::time::{SystemTime, UNIX_EPOCH};

use rand::distributions::Alphanumeric;
use rand::Rng;

/// Seconds since the Unix epoch, as the real APIs report in `created`.
pub(crate) fn unix_now() -> u64 {
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
pub(crate) fn completion_id() -> String {
    format!("chatcmpl-{}", random_suffix(29))
}

/// `fp_…` system fingerprint, matching OpenAI's shape.
pub(crate) fn system_fingerprint() -> String {
    format!("fp_{}", random_suffix(10))
}

/// `call_…` tool-call id, matching OpenAI's shape.
pub(crate) fn tool_call_id() -> String {
    format!("call_{}", random_suffix(24))
}

/// `resp_…` Responses API response id.
pub(crate) fn response_id() -> String {
    format!("resp_{}", random_suffix(24))
}

/// `msg_…` Responses API output message item id.
pub(crate) fn message_item_id() -> String {
    format!("msg_{}", random_suffix(24))
}

/// `fc_…` Responses API function-call item id.
pub(crate) fn function_item_id() -> String {
    format!("fc_{}", random_suffix(24))
}

/// `msg_…` Anthropic Messages id.
pub(crate) fn anthropic_message_id() -> String {
    format!("msg_{}", random_suffix(24))
}

/// `toolu_…` Anthropic tool-use block id.
pub(crate) fn tool_use_id() -> String {
    format!("toolu_{}", random_suffix(24))
}
