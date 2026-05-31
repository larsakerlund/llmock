//! Parsing of the Anthropic Messages API request body into the neutral model.

use serde::Deserialize;

use crate::adapters::openai::request::Content;
use crate::core::{Message, NeutralRequest};

#[derive(Debug, Deserialize)]
pub(crate) struct MessagesRequest {
    pub model: String,
    #[serde(default)]
    pub messages: Vec<AnthropicMessage>,
    /// `system` may be a string or an array of text blocks.
    #[serde(default)]
    pub system: Content,
    #[serde(default)]
    pub stream: bool,
    // `max_tokens` is required by the real API but llmock doesn't need it; it is
    // accepted and ignored (unknown fields like `tools` are ignored too).
}

#[derive(Debug, Deserialize)]
pub(crate) struct AnthropicMessage {
    pub role: String,
    #[serde(default)]
    pub content: Content,
}

impl MessagesRequest {
    pub(crate) fn into_neutral(self) -> NeutralRequest {
        let mut messages = Vec::new();

        let system = self.system.flatten();
        if !system.is_empty() {
            messages.push(Message {
                role: "system".to_string(),
                content: system,
            });
        }

        for m in self.messages {
            messages.push(Message {
                role: m.role,
                content: m.content.flatten(),
            });
        }

        NeutralRequest {
            model: self.model,
            messages,
            stream: self.stream,
            include_usage: false, // Anthropic always reports usage in the stream.
        }
    }
}
