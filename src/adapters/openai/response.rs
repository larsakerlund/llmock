//! Serialization of a [`NeutralResponse`] into the exact OpenAI
//! `chat.completion` (non-streaming) wire object.
//!
//! Struct field order below is deliberate: `serde_json` serializes in
//! declaration order, so this is also the on-the-wire byte order. Keep it
//! matching the real API.

use serde::Serialize;

use crate::core::{NeutralResponse, StopReason};
use crate::util;

#[derive(Debug, Serialize)]
pub struct ChatCompletion {
    pub id: String,
    pub object: &'static str,
    pub created: u64,
    pub model: String,
    pub system_fingerprint: String,
    pub choices: Vec<Choice>,
    pub usage: Usage,
}

#[derive(Debug, Serialize)]
pub struct Choice {
    pub index: u32,
    pub message: ResponseMessage,
    pub logprobs: Option<()>,
    pub finish_reason: &'static str,
}

#[derive(Debug, Serialize)]
pub struct ResponseMessage {
    pub role: &'static str,
    pub content: String,
    pub refusal: Option<()>,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// Map a neutral stop reason to OpenAI's `finish_reason` vocabulary.
pub fn finish_reason_str(stop: StopReason) -> &'static str {
    match stop {
        StopReason::Stop => "stop",
        StopReason::Length => "length",
        StopReason::ToolCalls => "tool_calls",
        StopReason::ContentFilter => "content_filter",
    }
}

impl ChatCompletion {
    pub fn from_neutral(resp: &NeutralResponse) -> Self {
        ChatCompletion {
            id: util::completion_id(),
            object: "chat.completion",
            created: util::unix_now(),
            model: resp.model.clone(),
            system_fingerprint: util::system_fingerprint(),
            choices: vec![Choice {
                index: 0,
                message: ResponseMessage {
                    role: "assistant",
                    content: resp.content.clone(),
                    refusal: None,
                },
                logprobs: None,
                finish_reason: finish_reason_str(resp.stop_reason),
            }],
            usage: Usage {
                prompt_tokens: resp.usage.prompt_tokens,
                completion_tokens: resp.usage.completion_tokens,
                total_tokens: resp.usage.total(),
            },
        }
    }
}
