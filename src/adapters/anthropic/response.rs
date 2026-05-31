//! The Anthropic `message` object and its content blocks (non-streaming), plus
//! shared structs reused by the streaming serializer. Shapes mirror the
//! `anthropic` SDK's pydantic models.

use serde::Serialize;
use serde_json::Value;

use crate::core::{NeutralResponse, StopReason};
use crate::util;

#[derive(Debug, Serialize)]
pub struct MessageObject {
    pub id: String,
    #[serde(rename = "type")]
    pub message_type: &'static str, // "message"
    pub role: &'static str, // "assistant"
    pub model: String,
    pub content: Vec<ContentBlock>,
    pub stop_reason: &'static str,
    pub stop_sequence: Option<String>,
    pub usage: Usage,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum ContentBlock {
    Text(TextBlock),
    ToolUse(ToolUseBlock),
}

#[derive(Debug, Clone, Serialize)]
pub struct TextBlock {
    #[serde(rename = "type")]
    pub block_type: &'static str, // "text"
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolUseBlock {
    #[serde(rename = "type")]
    pub block_type: &'static str, // "tool_use"
    pub id: String,
    pub name: String,
    pub input: Value,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

/// Map a neutral stop reason to Anthropic's vocabulary.
pub fn stop_reason_str(stop: StopReason) -> &'static str {
    match stop {
        StopReason::Stop => "end_turn",
        StopReason::Length => "max_tokens",
        StopReason::ToolCalls => "tool_use",
        StopReason::ContentFilter => "refusal",
    }
}

/// Parse tool-call arguments (a JSON string) into a JSON value for the
/// `tool_use.input` field. Falls back to an empty object.
pub fn parse_input(arguments: &str) -> Value {
    serde_json::from_str(arguments).unwrap_or_else(|_| Value::Object(Default::default()))
}

/// Stable ids for the content blocks of one response (tool_use ids), so the
/// non-streaming message and the streamed blocks agree.
pub fn tool_use_ids(resp: &NeutralResponse) -> Vec<String> {
    resp.tool_calls.iter().map(|_| util::tool_use_id()).collect()
}

/// Build the ordered content blocks: a text block (if any content) followed by
/// one tool_use block per tool call.
pub fn content_blocks(resp: &NeutralResponse, tool_ids: &[String]) -> Vec<ContentBlock> {
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

pub fn message_object(resp: &NeutralResponse, id: &str, tool_ids: &[String]) -> MessageObject {
    MessageObject {
        id: id.to_string(),
        message_type: "message",
        role: "assistant",
        model: resp.model.clone(),
        content: content_blocks(resp, tool_ids),
        stop_reason: stop_reason_str(resp.stop_reason),
        stop_sequence: None,
        usage: Usage {
            input_tokens: resp.usage.prompt_tokens,
            output_tokens: resp.usage.completion_tokens,
        },
    }
}
