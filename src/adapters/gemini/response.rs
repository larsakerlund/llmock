//! The Gemini `GenerateContentResponse` object and its parts, shared between the
//! non-streaming and streaming serializers. Field names are camelCase to match
//! the Google API wire format.

use serde::Serialize;
use serde_json::Value;

use crate::core::{NeutralResponse, StopReason};

#[derive(Debug, Serialize)]
pub(crate) struct GenerateContentResponse {
    pub candidates: Vec<Candidate>,
    #[serde(rename = "usageMetadata", skip_serializing_if = "Option::is_none")]
    pub usage_metadata: Option<UsageMetadata>,
    #[serde(rename = "modelVersion")]
    pub model_version: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct Candidate {
    pub content: ContentOut,
    #[serde(rename = "finishReason", skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<&'static str>,
    pub index: u32,
}

#[derive(Debug, Serialize)]
pub(crate) struct ContentOut {
    pub parts: Vec<Part>,
    pub role: &'static str, // "model"
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub(crate) enum Part {
    Text {
        text: String,
    },
    FunctionCall {
        #[serde(rename = "functionCall")]
        function_call: FunctionCall,
    },
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct FunctionCall {
    pub name: String,
    pub args: Value,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub(crate) struct UsageMetadata {
    #[serde(rename = "promptTokenCount")]
    pub prompt_token_count: u32,
    #[serde(rename = "candidatesTokenCount")]
    pub candidates_token_count: u32,
    #[serde(rename = "totalTokenCount")]
    pub total_token_count: u32,
}

/// Map a neutral stop reason to Gemini's vocabulary. Gemini finishes function
/// calls with `STOP` (the function-call part itself signals the tool use).
pub(crate) fn finish_reason_str(stop: StopReason) -> &'static str {
    match stop {
        StopReason::Stop | StopReason::ToolCalls => "STOP",
        StopReason::Length => "MAX_TOKENS",
        StopReason::ContentFilter => "SAFETY",
    }
}

pub(crate) fn usage_metadata(resp: &NeutralResponse) -> UsageMetadata {
    UsageMetadata {
        prompt_token_count: resp.usage.prompt_tokens,
        candidates_token_count: resp.usage.completion_tokens,
        total_token_count: resp.usage.total(),
    }
}

pub(crate) fn parse_args(arguments: &str) -> Value {
    serde_json::from_str(arguments).unwrap_or_else(|_| Value::Object(serde_json::Map::new()))
}

/// Build the ordered parts: a text part (if any content) followed by one
/// `functionCall` part per tool call.
pub(crate) fn parts(resp: &NeutralResponse) -> Vec<Part> {
    let mut parts = Vec::new();
    if !resp.content.is_empty() {
        parts.push(Part::Text {
            text: resp.content.clone(),
        });
    }
    for tc in &resp.tool_calls {
        parts.push(Part::FunctionCall {
            function_call: FunctionCall {
                name: tc.name.clone(),
                args: parse_args(&tc.arguments),
            },
        });
    }
    parts
}

/// Build the full (non-streaming) response object.
pub(crate) fn generate_response(resp: &NeutralResponse) -> GenerateContentResponse {
    GenerateContentResponse {
        candidates: vec![Candidate {
            content: ContentOut {
                parts: parts(resp),
                role: "model",
            },
            finish_reason: Some(finish_reason_str(resp.stop_reason)),
            index: 0,
        }],
        usage_metadata: Some(usage_metadata(resp)),
        model_version: resp.model.clone(),
    }
}
