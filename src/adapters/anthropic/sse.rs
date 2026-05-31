//! Byte-faithful Anthropic Messages API streaming.
//!
//! Event lifecycle (each event has both an SSE `event:` name and a matching
//! `type` in its JSON data):
//!   message_start
//!   (per content block:)
//!     content_block_start → [ping] → content_block_delta ×N → content_block_stop
//!   message_delta (stop_reason + cumulative output usage)
//!   message_stop
//!
//! Text blocks delta as `text_delta`; tool_use blocks delta as `input_json_delta`
//! carrying `partial_json` fragments of the tool input.

use std::convert::Infallible;

use axum::body::{Body, Bytes};
use axum::http::{header, StatusCode};
use axum::response::Response;
use serde::Serialize;
use serde_json::Value;
use tokio::time::sleep;

use crate::core::{Fault, NeutralResponse};
use crate::stream::{chunk_text, delay};
use crate::util;

use super::response::{stop_reason_str, ContentBlock, TextBlock, ToolUseBlock, Usage};

fn fault_after(f: Fault) -> usize {
    match f {
        Fault::Truncate { after } | Fault::Malformed { after } | Fault::Hang { after, .. } => after,
    }
}

fn event<T: Serialize>(name: &str, payload: &T) -> Bytes {
    let json = serde_json::to_string(payload).expect("event serializes");
    let mut buf = String::with_capacity(name.len() + json.len() + 16);
    buf.push_str("event: ");
    buf.push_str(name);
    buf.push_str("\ndata: ");
    buf.push_str(&json);
    buf.push_str("\n\n");
    Bytes::from(buf)
}

pub(crate) fn stream_response(resp: &NeutralResponse) -> Response {
    let id = util::anthropic_message_id();
    let tool_ids: Vec<String> = resp
        .tool_calls
        .iter()
        .map(|_| util::tool_use_id())
        .collect();
    let resp = resp.clone();
    let spec = resp.stream;
    let fault = resp.fault;

    let body = Body::from_stream(async_stream::stream! {
        // message_start — content empty, output_tokens starts at 1 like the API.
        let start_msg = StartMessage {
            id: &id,
            message_type: "message",
            role: "assistant",
            model: &resp.model,
            content: &[],
            stop_reason: None,
            stop_sequence: None,
            usage: Usage { input_tokens: resp.usage.prompt_tokens, output_tokens: 1 },
        };
        yield Ok::<_, Infallible>(event("message_start", &MessageStart {
            event_type: "message_start", message: &start_msg,
        }));

        let has_text = !resp.content.is_empty();
        let mut index: u32 = 0;
        let mut emitted_ping = false;

        // --- Text block ---
        if has_text {
            yield Ok(event("content_block_start", &ContentBlockStart {
                event_type: "content_block_start", index,
                content_block: ContentBlock::Text(TextBlock { block_type: "text", text: String::new() }),
            }));
            // The real API interleaves periodic pings; emit one early.
            yield Ok(event("ping", &Ping { event_type: "ping" }));
            emitted_ping = true;

            let pieces = chunk_text(&resp.content, spec.chunk_by);
            let mut triggered: Option<Fault> = None;
            for (i, piece) in pieces.iter().enumerate() {
                if let Some(f) = fault {
                    if fault_after(f) == i { triggered = Some(f); break; }
                }
                if let Some(d) = delay(if i == 0 { spec.ttft_ms } else { spec.inter_token_ms }) {
                    sleep(d).await;
                }
                yield Ok(event("content_block_delta", &ContentBlockDelta {
                    event_type: "content_block_delta", index,
                    delta: DeltaKind::Text { delta_type: "text_delta", text: piece.clone() },
                }));
            }
            if triggered.is_none() {
                if let Some(f) = fault {
                    if fault_after(f) >= pieces.len() { triggered = Some(f); }
                }
            }
            if let Some(f) = triggered {
                match f {
                    Fault::Truncate { .. } => {}
                    Fault::Malformed { .. } => {
                        yield Ok(Bytes::from_static(b"event: content_block_delta\ndata: {BROKEN\n\n"));
                    }
                    Fault::Hang { hold_ms, .. } => {
                        if let Some(d) = delay(hold_ms) { sleep(d).await; }
                    }
                }
                return; // end stream without message_delta / message_stop
            }

            yield Ok(event("content_block_stop", &ContentBlockStop {
                event_type: "content_block_stop", index,
            }));
            index += 1;
        }

        // --- Tool-use blocks ---
        for (t, tc) in resp.tool_calls.iter().enumerate() {
            yield Ok(event("content_block_start", &ContentBlockStart {
                event_type: "content_block_start", index,
                content_block: ContentBlock::ToolUse(ToolUseBlock {
                    block_type: "tool_use",
                    id: tool_ids[t].clone(),
                    name: tc.name.clone(),
                    input: Value::Object(serde_json::Map::new()), // empty; filled via deltas
                }),
            }));
            if !emitted_ping {
                yield Ok(event("ping", &Ping { event_type: "ping" }));
                emitted_ping = true;
            }

            // Arguments stream as partial_json fragments.
            for frag in chunk_text(&tc.arguments, spec.chunk_by) {
                if let Some(d) = delay(spec.inter_token_ms) { sleep(d).await; }
                yield Ok(event("content_block_delta", &ContentBlockDelta {
                    event_type: "content_block_delta", index,
                    delta: DeltaKind::InputJson { delta_type: "input_json_delta", partial_json: frag },
                }));
            }

            yield Ok(event("content_block_stop", &ContentBlockStop {
                event_type: "content_block_stop", index,
            }));
            index += 1;
        }

        // message_delta — stop reason + cumulative output tokens.
        yield Ok(event("message_delta", &MessageDelta {
            event_type: "message_delta",
            delta: StopDelta { stop_reason: stop_reason_str(resp.stop_reason), stop_sequence: None },
            usage: DeltaUsage { output_tokens: resp.usage.completion_tokens },
        }));
        yield Ok(event("message_stop", &MessageStop { event_type: "message_stop" }));
    });

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .header("connection", "keep-alive")
        .body(body)
        .expect("valid streaming response")
}

// --- Event payload structs (field order = wire order) ---

#[derive(Serialize)]
struct MessageStart<'a> {
    #[serde(rename = "type")]
    event_type: &'static str,
    message: &'a StartMessage<'a>,
}

#[derive(Serialize)]
struct StartMessage<'a> {
    id: &'a str,
    #[serde(rename = "type")]
    message_type: &'static str,
    role: &'static str,
    model: &'a str,
    content: &'a [()],
    stop_reason: Option<&'static str>,
    stop_sequence: Option<String>,
    usage: Usage,
}

#[derive(Serialize)]
struct Ping {
    #[serde(rename = "type")]
    event_type: &'static str,
}

#[derive(Serialize)]
struct ContentBlockStart {
    #[serde(rename = "type")]
    event_type: &'static str,
    index: u32,
    content_block: ContentBlock,
}

#[derive(Serialize)]
struct ContentBlockDelta {
    #[serde(rename = "type")]
    event_type: &'static str,
    index: u32,
    delta: DeltaKind,
}

#[derive(Serialize)]
#[serde(untagged)]
enum DeltaKind {
    Text {
        #[serde(rename = "type")]
        delta_type: &'static str,
        text: String,
    },
    InputJson {
        #[serde(rename = "type")]
        delta_type: &'static str,
        partial_json: String,
    },
}

#[derive(Serialize)]
struct ContentBlockStop {
    #[serde(rename = "type")]
    event_type: &'static str,
    index: u32,
}

#[derive(Serialize)]
struct MessageDelta {
    #[serde(rename = "type")]
    event_type: &'static str,
    delta: StopDelta,
    usage: DeltaUsage,
}

#[derive(Serialize)]
struct StopDelta {
    stop_reason: &'static str,
    stop_sequence: Option<String>,
}

#[derive(Serialize)]
struct DeltaUsage {
    output_tokens: u32,
}

#[derive(Serialize)]
struct MessageStop {
    #[serde(rename = "type")]
    event_type: &'static str,
}
