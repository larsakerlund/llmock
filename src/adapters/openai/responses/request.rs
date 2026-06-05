//! Parsing of the OpenAI Responses API request body into the neutral model.
//!
//! The Responses API uses `input` (a string or an array of typed items) plus an
//! optional `instructions` string, rather than Chat Completions' `messages`. We
//! flatten both into neutral messages so the same fixture matching applies.

use serde::Deserialize;

use crate::adapters::content::Content;
use crate::core::{Message, NeutralRequest};

#[derive(Debug, Deserialize)]
pub(crate) struct ResponsesRequest {
    pub model: String,
    #[serde(default)]
    pub input: Input,
    #[serde(default)]
    pub instructions: Option<String>,
    #[serde(default)]
    pub stream: bool,
}

/// `input` is either a bare string or an array of input items.
#[derive(Debug, Deserialize, Default)]
#[serde(untagged)]
pub(crate) enum Input {
    #[default]
    Empty,
    Text(String),
    Items(Vec<InputItem>),
}

#[derive(Debug, Deserialize)]
pub(crate) struct InputItem {
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub content: Content,
}

impl ResponsesRequest {
    pub(crate) fn into_neutral(self) -> NeutralRequest {
        let mut messages = Vec::new();

        // `instructions` acts like a system prompt.
        if let Some(instr) = self.instructions {
            if !instr.is_empty() {
                messages.push(Message {
                    role: "system".to_string(),
                    content: instr,
                });
            }
        }

        match self.input {
            Input::Empty => {}
            Input::Text(text) => messages.push(Message {
                role: "user".to_string(),
                content: text,
            }),
            Input::Items(items) => {
                for item in items {
                    messages.push(Message {
                        role: item.role.unwrap_or_else(|| "user".to_string()),
                        content: item.content.flatten(),
                    });
                }
            }
        }

        NeutralRequest {
            model: self.model,
            messages,
            stream: self.stream,
            include_usage: false, // Responses always reports usage on completion.
        }
    }
}
