//! Byte-faithful OpenAI Responses API streaming.
//!
//! Unlike Chat Completions (one repeated `chat.completion.chunk` type), the
//! Responses API streams a rich, named event lifecycle. For a text turn:
//!   response.created → response.in_progress
//!     → response.output_item.added (message, in_progress)
//!       → response.content_part.added
//!         → response.output_text.delta ×N
//!       → response.output_text.done
//!       → response.content_part.done
//!     → response.output_item.done (message, completed)
//!   → response.completed
//! A function call swaps the inner part for:
//!     → response.output_item.added (function_call, in_progress)
//!       → response.function_call_arguments.delta ×N
//!       → response.function_call_arguments.done
//!     → response.output_item.done (function_call, completed)
//!
//! Every event carries an incrementing `sequence_number`. There is no
//! `[DONE]` sentinel — the stream ends after `response.completed`.

use std::convert::Infallible;

use axum::body::{Body, Bytes};
use axum::http::{header, StatusCode};
use axum::response::Response;
use serde::Serialize;
use tokio::time::sleep;

use crate::core::{Fault, NeutralResponse};
use crate::sse::{event, execute_fault, fault_after};
use crate::stream::{chunk_text, delay};
use crate::util;

use super::response::{
    completed_response, initial_response, FunctionCallItem, MessageItem, OutputItem, OutputText,
    ResponseIds, ResponseObject,
};

/// A deliberately broken frame for the `malformed` fault.
const MALFORMED: &[u8] = b"event: response.output_text.delta\ndata: {BROKEN\n\n";

pub(crate) fn stream_response(resp: &NeutralResponse) -> Response {
    let created_at = util::unix_now();
    let ids = ResponseIds::for_response(resp);
    let initial = initial_response(resp, &ids, created_at);
    let completed = completed_response(resp, &ids, created_at);

    let resp = resp.clone();
    let spec = resp.stream;
    let fault = resp.fault;

    let body = Body::from_stream(async_stream::stream! {
        let mut seq: u64 = 0;
        macro_rules! next { () => {{ let s = seq; seq += 1; s }} }

        // response.created / response.in_progress
        yield Ok::<_, Infallible>(event("response.created", &Created {
            event_type: "response.created", sequence_number: next!(), response: &initial,
        }));
        yield Ok(event("response.in_progress", &Created {
            event_type: "response.in_progress", sequence_number: next!(), response: &initial,
        }));

        let mut output_index: u32 = 0;
        let mut id_idx = 0usize;
        let has_message = !resp.content.is_empty() || resp.tool_calls.is_empty();

        // --- Text message item ---
        if has_message {
            let item_id = ids.item_ids[id_idx].clone();
            id_idx += 1;

            // output_item.added — message in progress, empty content
            yield Ok(event("response.output_item.added", &ItemEvent {
                event_type: "response.output_item.added",
                sequence_number: next!(),
                output_index,
                item: OutputItem::Message(MessageItem {
                    id: item_id.clone(), item_type: "message", status: "in_progress",
                    role: "assistant", content: vec![],
                }),
            }));
            // content_part.added — empty output_text part
            yield Ok(event("response.content_part.added", &PartEvent {
                event_type: "response.content_part.added",
                sequence_number: next!(),
                item_id: &item_id, output_index, content_index: 0,
                part: OutputText { part_type: "output_text", text: String::new(), annotations: vec![] },
            }));

            // output_text.delta ×N, paced, with optional fault
            let pieces = chunk_text(&resp.content, spec.chunk_by);
            let mut triggered: Option<Fault> = None;
            for (i, piece) in pieces.iter().enumerate() {
                if let Some(f) = fault {
                    if fault_after(f) == i { triggered = Some(f); break; }
                }
                if let Some(d) = delay(if i == 0 { spec.ttft_ms } else { spec.inter_token_ms }) {
                    sleep(d).await;
                }
                yield Ok(event("response.output_text.delta", &TextDelta {
                    event_type: "response.output_text.delta",
                    sequence_number: next!(),
                    item_id: &item_id, output_index, content_index: 0,
                    delta: piece, logprobs: &[],
                }));
            }
            if triggered.is_none() {
                if let Some(f) = fault {
                    if fault_after(f) >= pieces.len() { triggered = Some(f); }
                }
            }
            if let Some(f) = triggered {
                // End the stream without response.completed.
                if let Some(bytes) = execute_fault(f, Bytes::from_static(MALFORMED)).await {
                    yield Ok(bytes);
                }
                return;
            }

            // output_text.done / content_part.done / output_item.done (completed)
            yield Ok(event("response.output_text.done", &TextDone {
                event_type: "response.output_text.done",
                sequence_number: next!(),
                item_id: &item_id, output_index, content_index: 0,
                text: &resp.content, logprobs: &[],
            }));
            yield Ok(event("response.content_part.done", &PartEvent {
                event_type: "response.content_part.done",
                sequence_number: next!(),
                item_id: &item_id, output_index, content_index: 0,
                part: OutputText { part_type: "output_text", text: resp.content.clone(), annotations: vec![] },
            }));
            yield Ok(event("response.output_item.done", &ItemEvent {
                event_type: "response.output_item.done",
                sequence_number: next!(),
                output_index,
                item: OutputItem::Message(MessageItem {
                    id: item_id.clone(), item_type: "message", status: "completed",
                    role: "assistant",
                    content: vec![OutputText { part_type: "output_text", text: resp.content.clone(), annotations: vec![] }],
                }),
            }));
            output_index += 1;
        }

        // --- Function-call items ---
        for tc in &resp.tool_calls {
            let item_id = ids.item_ids[id_idx].clone();
            id_idx += 1;

            // output_item.added — function_call in progress, empty arguments
            yield Ok(event("response.output_item.added", &ItemEvent {
                event_type: "response.output_item.added",
                sequence_number: next!(),
                output_index,
                item: OutputItem::FunctionCall(FunctionCallItem {
                    id: item_id.clone(), item_type: "function_call", status: "in_progress",
                    call_id: tc.id.clone(), name: tc.name.clone(), arguments: String::new(),
                }),
            }));

            // function_call_arguments.delta ×N
            for frag in chunk_text(&tc.arguments, spec.chunk_by) {
                if let Some(d) = delay(spec.inter_token_ms) { sleep(d).await; }
                yield Ok(event("response.function_call_arguments.delta", &FnArgsDelta {
                    event_type: "response.function_call_arguments.delta",
                    sequence_number: next!(),
                    item_id: &item_id, output_index, delta: &frag,
                }));
            }

            // function_call_arguments.done / output_item.done (completed)
            yield Ok(event("response.function_call_arguments.done", &FnArgsDone {
                event_type: "response.function_call_arguments.done",
                sequence_number: next!(),
                item_id: &item_id, output_index,
                name: &tc.name, arguments: &tc.arguments,
            }));
            yield Ok(event("response.output_item.done", &ItemEvent {
                event_type: "response.output_item.done",
                sequence_number: next!(),
                output_index,
                item: OutputItem::FunctionCall(FunctionCallItem {
                    id: item_id.clone(), item_type: "function_call", status: "completed",
                    call_id: tc.id.clone(), name: tc.name.clone(), arguments: tc.arguments.clone(),
                }),
            }));
            output_index += 1;
        }

        // response.completed — full response object with output + usage
        yield Ok(event("response.completed", &Created {
            event_type: "response.completed", sequence_number: next!(), response: &completed,
        }));
        let _ = seq; // final increment intentionally unused
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
struct Created<'a> {
    #[serde(rename = "type")]
    event_type: &'static str,
    sequence_number: u64,
    response: &'a ResponseObject,
}

#[derive(Serialize)]
struct ItemEvent {
    #[serde(rename = "type")]
    event_type: &'static str,
    sequence_number: u64,
    output_index: u32,
    item: OutputItem,
}

#[derive(Serialize)]
struct PartEvent<'a> {
    #[serde(rename = "type")]
    event_type: &'static str,
    sequence_number: u64,
    item_id: &'a str,
    output_index: u32,
    content_index: u32,
    part: OutputText,
}

#[derive(Serialize)]
struct TextDelta<'a> {
    #[serde(rename = "type")]
    event_type: &'static str,
    sequence_number: u64,
    item_id: &'a str,
    output_index: u32,
    content_index: u32,
    delta: &'a str,
    logprobs: &'a [()],
}

#[derive(Serialize)]
struct TextDone<'a> {
    #[serde(rename = "type")]
    event_type: &'static str,
    sequence_number: u64,
    item_id: &'a str,
    output_index: u32,
    content_index: u32,
    text: &'a str,
    logprobs: &'a [()],
}

#[derive(Serialize)]
struct FnArgsDelta<'a> {
    #[serde(rename = "type")]
    event_type: &'static str,
    sequence_number: u64,
    item_id: &'a str,
    output_index: u32,
    delta: &'a str,
}

#[derive(Serialize)]
struct FnArgsDone<'a> {
    #[serde(rename = "type")]
    event_type: &'static str,
    sequence_number: u64,
    item_id: &'a str,
    output_index: u32,
    name: &'a str,
    arguments: &'a str,
}
