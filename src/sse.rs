//! Shared server-sent-events plumbing used by every streaming adapter: byte
//! framing, the fault-trigger index, and executing a triggered fault. The
//! per-adapter serializers keep only what genuinely differs (their event names,
//! delta shapes, and the order they emit them).

use axum::body::Bytes;
use serde::Serialize;
use tokio::time::sleep;

use crate::core::Fault;
use crate::stream::delay;

/// Build an SSE frame, optionally prefixed with an `event: <name>` line.
fn frame(name: Option<&str>, json: &str) -> Bytes {
    let mut buf = String::with_capacity(name.map_or(0, |n| n.len() + 8) + json.len() + 8);
    if let Some(name) = name {
        buf.push_str("event: ");
        buf.push_str(name);
        buf.push('\n');
    }
    buf.push_str("data: ");
    buf.push_str(json);
    buf.push_str("\n\n");
    Bytes::from(buf)
}

/// A data-only SSE frame: `data: <json>\n\n` (OpenAI Chat Completions, Gemini).
pub(crate) fn data<T: Serialize>(value: &T) -> Bytes {
    let json = serde_json::to_string(value).expect("event serializes");
    frame(None, &json)
}

/// A named SSE frame: `event: <name>\ndata: <json>\n\n` (Anthropic, Responses).
pub(crate) fn event<T: Serialize>(name: &str, value: &T) -> Bytes {
    let json = serde_json::to_string(value).expect("event serializes");
    frame(Some(name), &json)
}

/// How many content deltas to emit before a fault triggers.
pub(crate) fn fault_after(f: Fault) -> usize {
    match f {
        Fault::Truncate { after } | Fault::Malformed { after } | Fault::Hang { after, .. } => after,
    }
}

/// Carry out a triggered fault's tail and return any bytes the caller should
/// emit before ending the stream (with no terminal event). For `Hang` this
/// stalls first; for `Malformed` it returns the provider's broken frame.
pub(crate) async fn execute_fault(f: Fault, malformed: Bytes) -> Option<Bytes> {
    match f {
        Fault::Truncate { .. } => None,
        Fault::Malformed { .. } => Some(malformed),
        Fault::Hang { hold_ms, .. } => {
            if let Some(d) = delay(hold_ms) {
                sleep(d).await;
            }
            None
        }
    }
}
