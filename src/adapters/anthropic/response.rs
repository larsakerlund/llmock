//! The Anthropic `message` object and its content blocks (non-streaming), plus
//! shared structs reused by the streaming serializer. Shapes mirror the
//! `anthropic` SDK's pydantic models.

use serde::Serialize;
use serde_json::Value;

use crate::core::{NeutralResponse, StopReason};
use crate::util;

// Field order matches the real Anthropic wire bytes exactly (verified against a
// recorded api.anthropic.com response): model, id, type, role, content, …
#[derive(Debug, Serialize)]
pub(crate) struct MessageObject {
    pub model: String,
    pub id: String,
    #[serde(rename = "type")]
    pub message_type: &'static str, // "message"
    pub role: &'static str, // "assistant"
    pub content: Vec<ContentBlock>,
    pub stop_reason: &'static str,
    pub stop_sequence: Option<String>,
    pub stop_details: Option<()>, // null
    pub usage: Usage,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub(crate) enum ContentBlock {
    Text(TextBlock),
    ToolUse(ToolUseBlock),
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct TextBlock {
    #[serde(rename = "type")]
    pub block_type: &'static str, // "text"
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ToolUseBlock {
    #[serde(rename = "type")]
    pub block_type: &'static str, // "tool_use"
    pub id: String,
    pub name: String,
    pub input: Value,
}

// Full usage shape, field order matching the real API. Cache counts are zero
// (llmock doesn't model prompt caching); `service_tier`/`inference_geo` use the
// real API's stable defaults.
#[derive(Debug, Clone, Copy, Serialize)]
pub(crate) struct Usage {
    pub input_tokens: u32,
    pub cache_creation_input_tokens: u32,
    pub cache_read_input_tokens: u32,
    pub cache_creation: CacheCreation,
    pub output_tokens: u32,
    pub service_tier: &'static str,
    pub inference_geo: &'static str,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub(crate) struct CacheCreation {
    pub ephemeral_5m_input_tokens: u32,
    pub ephemeral_1h_input_tokens: u32,
}

impl Usage {
    pub(crate) fn new(input_tokens: u32, output_tokens: u32) -> Self {
        Usage {
            input_tokens,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
            cache_creation: CacheCreation {
                ephemeral_5m_input_tokens: 0,
                ephemeral_1h_input_tokens: 0,
            },
            output_tokens,
            service_tier: "standard",
            inference_geo: "not_available",
        }
    }
}

/// Map a neutral stop reason to Anthropic's vocabulary.
pub(crate) fn stop_reason_str(stop: StopReason) -> &'static str {
    match stop {
        StopReason::Stop => "end_turn",
        StopReason::Length => "max_tokens",
        StopReason::ToolCalls => "tool_use",
        StopReason::ContentFilter => "refusal",
    }
}

/// Parse tool-call arguments (a JSON string) into a JSON value for the
/// `tool_use.input` field. Falls back to an empty object.
pub(crate) fn parse_input(arguments: &str) -> Value {
    serde_json::from_str(arguments).unwrap_or_else(|_| Value::Object(serde_json::Map::new()))
}

/// Stable ids for the content blocks of one response (tool_use ids), so the
/// non-streaming message and the streamed blocks agree.
pub(crate) fn tool_use_ids(resp: &NeutralResponse) -> Vec<String> {
    resp.tool_calls
        .iter()
        .map(|_| util::tool_use_id())
        .collect()
}

/// Build the ordered content blocks: a text block (if any content) followed by
/// one tool_use block per tool call.
pub(crate) fn content_blocks(resp: &NeutralResponse, tool_ids: &[String]) -> Vec<ContentBlock> {
    let mut blocks = Vec::new();
    if !resp.content.is_empty() {
        blocks.push(ContentBlock::Text(TextBlock {
            block_type: "text",
            text: resp.content.clone(),
        }));
    }
    for (i, tc) in resp.tool_calls.iter().enumerate() {
        blocks.push(ContentBlock::ToolUse(ToolUseBlock {
            block_type: "tool_use",
            id: tool_ids[i].clone(),
            name: tc.name.clone(),
            input: parse_input(&tc.arguments),
        }));
    }
    blocks
}

pub(crate) fn message_object(
    resp: &NeutralResponse,
    id: &str,
    tool_ids: &[String],
) -> MessageObject {
    MessageObject {
        model: resp.model.clone(),
        id: id.to_string(),
        message_type: "message",
        role: "assistant",
        content: content_blocks(resp, tool_ids),
        stop_reason: stop_reason_str(resp.stop_reason),
        stop_sequence: None,
        stop_details: None,
        usage: Usage::new(resp.usage.prompt_tokens, resp.usage.completion_tokens),
    }
}
