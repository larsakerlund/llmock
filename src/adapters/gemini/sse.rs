//! Gemini `streamGenerateContent?alt=sse` streaming.
//!
//! Data-only SSE (`data: <GenerateContentResponse>\n\n`, no event names, no
//! `[DONE]`). Each chunk is a full `GenerateContentResponse` carrying the
//! incremental parts; the final chunk carries `finishReason` + `usageMetadata`.

use std::convert::Infallible;

use axum::body::{Body, Bytes};
use axum::http::{header, StatusCode};
use axum::response::Response;
use tokio::time::sleep;

use crate::core::{Fault, NeutralResponse};
use crate::sse::{data as frame, execute_fault, fault_after};
use crate::stream::{chunk_text, delay};

use super::response::{
    finish_reason_str, parse_args, usage_metadata, Candidate, ContentOut, FunctionCall,
    GenerateContentResponse, Part,
};

/// A deliberately broken frame for the `malformed` fault.
const MALFORMED: &[u8] = b"data: {BROKEN\n\n";

fn text_chunk(piece: String, model: &str) -> GenerateContentResponse {
    GenerateContentResponse {
        candidates: vec![Candidate {
            content: ContentOut {
                parts: vec![Part::Text { text: piece }],
                role: "model",
            },
            finish_reason: None,
            index: 0,
        }],
        usage_metadata: None,
        model_version: model.to_string(),
    }
}

pub(crate) fn stream_response(resp: &NeutralResponse) -> Response {
    let resp = resp.clone();
    let spec = resp.stream;
    let fault = resp.fault;

    let body = Body::from_stream(async_stream::stream! {
        // Text parts, paced, with optional fault.
        let pieces = chunk_text(&resp.content, spec.chunk_by);
        let mut triggered: Option<Fault> = None;
        for (i, piece) in pieces.iter().enumerate() {
            if let Some(f) = fault {
                if fault_after(f) == i { triggered = Some(f); break; }
            }
            if let Some(d) = delay(if i == 0 { spec.ttft_ms } else { spec.inter_token_ms }) {
                sleep(d).await;
            }
            yield Ok::<_, Infallible>(frame(&text_chunk(piece.clone(), &resp.model)));
        }
        if triggered.is_none() {
            if let Some(f) = fault {
                if fault_after(f) >= pieces.len() { triggered = Some(f); }
            }
        }
        if let Some(f) = triggered {
            // End the stream without a final (finishReason) chunk.
            if let Some(bytes) = execute_fault(f, Bytes::from_static(MALFORMED)).await {
                yield Ok(bytes);
            }
            return;
        }

        // One chunk per function call.
        for tc in &resp.tool_calls {
            if let Some(d) = delay(spec.inter_token_ms) { sleep(d).await; }
            let chunk = GenerateContentResponse {
                candidates: vec![Candidate {
                    content: ContentOut {
                        parts: vec![Part::FunctionCall {
                            function_call: FunctionCall {
                                name: tc.name.clone(),
                                args: parse_args(&tc.arguments),
                            },
                        }],
                        role: "model",
                    },
                    finish_reason: None,
                    index: 0,
                }],
                usage_metadata: None,
                model_version: resp.model.clone(),
            };
            yield Ok(frame(&chunk));
        }

        // Final chunk: empty parts, finishReason + usageMetadata.
        let final_chunk = GenerateContentResponse {
            candidates: vec![Candidate {
                content: ContentOut { parts: vec![], role: "model" },
                finish_reason: Some(finish_reason_str(resp.stop_reason)),
                index: 0,
            }],
            usage_metadata: Some(usage_metadata(&resp)),
            model_version: resp.model.clone(),
        };
        yield Ok(frame(&final_chunk));
    });

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .header("connection", "keep-alive")
        .body(body)
        .expect("valid streaming response")
}
