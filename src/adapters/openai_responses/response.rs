//! The OpenAI Responses API `response` object and its output items.
//!
//! Shapes mirror the `openai` SDK's pydantic models exactly (verified against
//! openai-python). The same structs are reused by the streaming serializer,
//! which embeds a `response` object in its `response.created` /
//! `response.in_progress` / `response.completed` events.

use serde::Serialize;

use crate::core::NeutralResponse;
use crate::util;

/// A `response` object. Fields cover the SDK's required set plus the commonly
/// inspected `status`/`usage`; optional fields we don't model are emitted as
/// null/empty so the SDK validates them.
#[derive(Debug, Serialize)]
pub(crate) struct ResponseObject {
    pub id: String,
    pub object: &'static str,
    pub created_at: u64,
    pub model: String,
    pub status: &'static str,
    pub error: Option<()>,
    pub incomplete_details: Option<()>,
    pub instructions: Option<String>,
    pub metadata: serde_json::Map<String, serde_json::Value>,
    pub output: Vec<OutputItem>,
    pub parallel_tool_calls: bool,
    pub tool_choice: &'static str,
    pub tools: Vec<()>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub(crate) enum OutputItem {
    Message(MessageItem),
    FunctionCall(FunctionCallItem),
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct MessageItem {
    pub id: String,
    #[serde(rename = "type")]
    pub item_type: &'static str, // "message"
    pub status: &'static str,
    pub role: &'static str, // "assistant"
    pub content: Vec<OutputText>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct OutputText {
    #[serde(rename = "type")]
    pub part_type: &'static str, // "output_text"
    pub text: String,
    pub annotations: Vec<()>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct FunctionCallItem {
    pub id: String,
    #[serde(rename = "type")]
    pub item_type: &'static str, // "function_call"
    pub status: &'static str,
    pub call_id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub(crate) struct Usage {
    pub input_tokens: u32,
    pub input_tokens_details: InputTokensDetails,
    pub output_tokens: u32,
    pub output_tokens_details: OutputTokensDetails,
    pub total_tokens: u32,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub(crate) struct InputTokensDetails {
    pub cached_tokens: u32,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub(crate) struct OutputTokensDetails {
    pub reasoning_tokens: u32,
}

/// Stable ids for one response, shared between the items and the streaming
/// events that reference them.
pub(crate) struct ResponseIds {
    pub response_id: String,
    /// One item id per output item, in order.
    pub item_ids: Vec<String>,
}

impl ResponseIds {
    pub(crate) fn for_response(resp: &NeutralResponse) -> Self {
        let mut item_ids = Vec::new();
        // A text message item (if any content) comes first.
        if !resp.content.is_empty() || resp.tool_calls.is_empty() {
            item_ids.push(util::message_item_id());
        }
        for _ in &resp.tool_calls {
            item_ids.push(util::function_item_id());
        }
        ResponseIds {
            response_id: util::response_id(),
            item_ids,
        }
    }
}

pub(crate) fn usage_for(resp: &NeutralResponse) -> Usage {
    Usage {
        input_tokens: resp.usage.prompt_tokens,
        input_tokens_details: InputTokensDetails { cached_tokens: 0 },
        output_tokens: resp.usage.completion_tokens,
        output_tokens_details: OutputTokensDetails {
            reasoning_tokens: 0,
        },
        total_tokens: resp.usage.total(),
    }
}

/// Build the ordered output items for a response: an optional text message item
/// followed by one function-call item per tool call.
pub(crate) fn output_items(resp: &NeutralResponse, ids: &ResponseIds) -> Vec<OutputItem> {
    let mut items = Vec::new();
    let mut item_pos = 0;

    let has_message = !resp.content.is_empty() || resp.tool_calls.is_empty();
    if has_message {
        items.push(OutputItem::Message(MessageItem {
            id: ids.item_ids[item_pos].clone(),
            item_type: "message",
            status: "completed",
            role: "assistant",
            content: vec![OutputText {
                part_type: "output_text",
                text: resp.content.clone(),
                annotations: Vec::new(),
            }],
        }));
        item_pos += 1;
    }

    for tc in &resp.tool_calls {
        items.push(OutputItem::FunctionCall(FunctionCallItem {
            id: ids.item_ids[item_pos].clone(),
            item_type: "function_call",
            status: "completed",
            call_id: tc.id.clone(),
            name: tc.name.clone(),
            arguments: tc.arguments.clone(),
        }));
        item_pos += 1;
    }

    items
}

/// Build the initial (`in_progress`, empty output, no usage) response object
/// embedded in the `response.created` / `response.in_progress` stream events.
pub(crate) fn initial_response(
    resp: &NeutralResponse,
    ids: &ResponseIds,
    created_at: u64,
) -> ResponseObject {
    ResponseObject {
        id: ids.response_id.clone(),
        object: "response",
        created_at,
        model: resp.model.clone(),
        status: "in_progress",
        error: None,
        incomplete_details: None,
        instructions: None,
        metadata: serde_json::Map::new(),
        output: Vec::new(),
        parallel_tool_calls: true,
        tool_choice: "auto",
        tools: Vec::new(),
        temperature: None,
        top_p: None,
        usage: None,
    }
}

/// Build the full (completed) response object (non-streaming path, and the
/// object embedded in the streaming `response.completed` event).
pub(crate) fn completed_response(
    resp: &NeutralResponse,
    ids: &ResponseIds,
    created_at: u64,
) -> ResponseObject {
    ResponseObject {
        id: ids.response_id.clone(),
        object: "response",
        created_at,
        model: resp.model.clone(),
        status: "completed",
        error: None,
        incomplete_details: None,
        instructions: None,
        metadata: serde_json::Map::new(),
        output: output_items(resp, ids),
        parallel_tool_calls: true,
        tool_choice: "auto",
        tools: Vec::new(),
        temperature: None,
        top_p: None,
        usage: Some(usage_for(resp)),
    }
}
