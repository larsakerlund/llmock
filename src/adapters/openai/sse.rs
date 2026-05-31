//! Byte-exact OpenAI Chat Completions streaming (`chat.completion.chunk` SSE).
//!
//! The sequence we emit, matching the real API:
//!   1. role chunk      — `delta:{"role":"assistant","content":""}`
//!   2. content chunks  — `delta:{"content":"<piece>"}` (one per stream piece)
//!   3. final chunk     — `delta:{}` + `finish_reason`
//!   4. usage chunk     — `choices:[]` + `usage:{…}`   (only if include_usage)
//!   5. `data: [DONE]`
//!
//! Every event is `data: <compact-json>\n\n`. When `include_usage` is set the
//! real API also adds `"usage":null` to chunks 1–3, so we mirror that.

use std::convert::Infallible;

use axum::body::{Body, Bytes};
use axum::http::{header, StatusCode};
use axum::response::Response;
use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};
use tokio::time::sleep;

use crate::core::{Fault, NeutralResponse};
use crate::sse::{data as frame, execute_fault, fault_after};
use crate::stream::{chunk_text, delay};
use crate::util;

use super::response::{finish_reason_str, Usage};

/// A deliberately broken frame for the `malformed` fault.
const MALFORMED: &[u8] =
    b"data: {\"id\":\"llmock\",\"object\":\"chat.completion.chunk\",\"choices\":[{BROKEN\n\n";

/// Build the streaming HTTP response for a neutral response.
pub(crate) fn stream_response(resp: &NeutralResponse, include_usage: bool) -> Response {
    // Stable across all chunks of one response, as the real API does.
    let id = util::completion_id();
    let created = util::unix_now();
    let model = resp.model.clone();
    let fingerprint = util::system_fingerprint();
    let finish = finish_reason_str(resp.stop_reason);

    let pieces = chunk_text(&resp.content, resp.stream.chunk_by);
    let spec = resp.stream;
    let fault = resp.fault;
    let tool_calls = resp.tool_calls.clone();
    let usage = Usage {
        prompt_tokens: resp.usage.prompt_tokens,
        completion_tokens: resp.usage.completion_tokens,
        total_tokens: resp.usage.total(),
    };

    let body = Body::from_stream(async_stream::stream! {
        let usage_field = if include_usage { UsageField::Null } else { UsageField::Absent };

        // 1. role chunk. Content is `""` for a text turn, omitted for a pure
        //    tool-call turn (mirroring the real API's first delta).
        let role_content = if tool_calls.is_empty() { Some(String::new()) } else { None };
        let role = Chunk {
            id: &id, created, model: &model, system_fingerprint: &fingerprint,
            choices: vec![ChunkChoice {
                index: 0,
                delta: Delta { role: Some("assistant"), content: role_content, tool_calls: None },
                logprobs: None,
                finish_reason: None,
            }],
            usage: usage_field,
        };
        yield Ok::<_, Infallible>(frame(&role));

        // 2. content chunks, paced by ttft then inter-token delay. If a fault is
        //    configured, stop once `after` deltas have been emitted.
        let mut triggered: Option<Fault> = None;
        for (i, piece) in pieces.iter().enumerate() {
            if let Some(f) = fault {
                if fault_after(f) == i {
                    triggered = Some(f);
                    break;
                }
            }
            if let Some(d) = delay(if i == 0 { spec.ttft_ms } else { spec.inter_token_ms }) {
                sleep(d).await;
            }
            let chunk = Chunk {
                id: &id, created, model: &model, system_fingerprint: &fingerprint,
                choices: vec![ChunkChoice {
                    index: 0,
                    delta: Delta { role: None, content: Some(piece.clone()), tool_calls: None },
                    logprobs: None,
                    finish_reason: None,
                }],
                usage: usage_field,
            };
            yield Ok(frame(&chunk));
        }
        // A fault whose `after` is at/beyond the content length triggers now.
        if triggered.is_none() {
            if let Some(f) = fault {
                if fault_after(f) >= pieces.len() {
                    triggered = Some(f);
                }
            }
        }

        if let Some(f) = triggered {
            // Fault path: misbehave, then end the stream WITHOUT a final chunk or
            // `[DONE]` — exactly the broken behaviour the developer asked to test.
            if let Some(bytes) = execute_fault(f, Bytes::from_static(MALFORMED)).await {
                yield Ok(bytes);
            }
            return;
        }

        // 2b. tool-call deltas: one opening delta per call carrying id/type/name
        //     (and `arguments:""`), then the arguments streamed as fragments —
        //     exactly how OpenAI streams function calls.
        let ttft_for_tools = pieces.is_empty();
        for (idx, tc) in tool_calls.iter().enumerate() {
            let first_tool_emission = ttft_for_tools && idx == 0;
            if let Some(d) = delay(if first_tool_emission { spec.ttft_ms } else { spec.inter_token_ms }) {
                sleep(d).await;
            }
            let opening = Chunk {
                id: &id, created, model: &model, system_fingerprint: &fingerprint,
                choices: vec![ChunkChoice {
                    index: 0,
                    delta: Delta {
                        role: None,
                        content: None,
                        tool_calls: Some(vec![ToolCallDelta {
                            index: idx as u32,
                            id: Some(tc.id.clone()),
                            call_type: Some("function"),
                            function: FunctionDelta { name: Some(tc.name.clone()), arguments: Some(String::new()) },
                        }]),
                    },
                    logprobs: None,
                    finish_reason: None,
                }],
                usage: usage_field,
            };
            yield Ok(frame(&opening));

            for frag in chunk_text(&tc.arguments, spec.chunk_by) {
                if let Some(d) = delay(spec.inter_token_ms) {
                    sleep(d).await;
                }
                let arg_chunk = Chunk {
                    id: &id, created, model: &model, system_fingerprint: &fingerprint,
                    choices: vec![ChunkChoice {
                        index: 0,
                        delta: Delta {
                            role: None,
                            content: None,
                            tool_calls: Some(vec![ToolCallDelta {
                                index: idx as u32,
                                id: None,
                                call_type: None,
                                function: FunctionDelta { name: None, arguments: Some(frag) },
                            }]),
                        },
                        logprobs: None,
                        finish_reason: None,
                    }],
                    usage: usage_field,
                };
                yield Ok(frame(&arg_chunk));
            }
        }

        // 3. final chunk: empty delta + finish_reason
        let final_chunk = Chunk {
            id: &id, created, model: &model, system_fingerprint: &fingerprint,
            choices: vec![ChunkChoice {
                index: 0,
                delta: Delta { role: None, content: None, tool_calls: None },
                logprobs: None,
                finish_reason: Some(finish),
            }],
            usage: usage_field,
        };
        yield Ok(frame(&final_chunk));

        // 4. usage-only chunk (empty choices), only when requested
        if include_usage {
            let usage_chunk = Chunk {
                id: &id, created, model: &model, system_fingerprint: &fingerprint,
                choices: vec![],
                usage: UsageField::Value(usage),
            };
            yield Ok(frame(&usage_chunk));
        }

        // 5. terminator
        yield Ok(Bytes::from_static(b"data: [DONE]\n\n"));
    });

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .header("connection", "keep-alive")
        .body(body)
        .expect("valid streaming response")
}

/// One `chat.completion.chunk`. `Serialize` is hand-written so the constant
/// `object` field appears in the right position without naming it at every
/// construction site, and so `usage` can be omitted entirely when absent.
struct Chunk<'a> {
    id: &'a str,
    created: u64,
    model: &'a str,
    system_fingerprint: &'a str,
    choices: Vec<ChunkChoice>,
    usage: UsageField,
}

impl Serialize for Chunk<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let has_usage = !self.usage.is_absent();
        let fields = if has_usage { 7 } else { 6 };
        let mut st = s.serialize_struct("chat.completion.chunk", fields)?;
        st.serialize_field("id", self.id)?;
        st.serialize_field("object", "chat.completion.chunk")?;
        st.serialize_field("created", &self.created)?;
        st.serialize_field("model", self.model)?;
        st.serialize_field("system_fingerprint", self.system_fingerprint)?;
        st.serialize_field("choices", &self.choices)?;
        if has_usage {
            st.serialize_field("usage", &self.usage)?;
        } else {
            st.skip_field("usage")?;
        }
        st.end()
    }
}

#[derive(Serialize)]
struct ChunkChoice {
    index: u32,
    delta: Delta,
    logprobs: Option<()>,
    finish_reason: Option<&'static str>,
}

#[derive(Serialize)]
struct Delta {
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<ToolCallDelta>>,
}

#[derive(Serialize)]
struct ToolCallDelta {
    index: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    call_type: Option<&'static str>,
    function: FunctionDelta,
}

#[derive(Serialize)]
struct FunctionDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    arguments: Option<String>,
}

/// Three-state `usage` field: absent (omit key), explicit null, or an object.
#[derive(Clone, Copy)]
enum UsageField {
    Absent,
    Null,
    Value(Usage),
}

impl UsageField {
    fn is_absent(&self) -> bool {
        matches!(self, UsageField::Absent)
    }
}

impl Serialize for UsageField {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            // Absent is skipped before reaching here; treat as null defensively.
            UsageField::Absent | UsageField::Null => s.serialize_none(),
            UsageField::Value(u) => u.serialize(s),
        }
    }
}
