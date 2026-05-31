//! Parsing of the OpenAI Chat Completions request body into the neutral model.

use serde::Deserialize;

use crate::core::{Message, NeutralRequest};

#[derive(Debug, Deserialize)]
pub(crate) struct ChatCompletionRequest {
    pub model: String,
    #[serde(default)]
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub stream_options: Option<StreamOptions>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ChatMessage {
    pub role: String,
    /// OpenAI allows `content` to be a string or an array of content parts.
    /// For the MVP we accept either and flatten to text.
    #[serde(default)]
    pub content: Content,
}

#[derive(Debug, Deserialize)]
pub(crate) struct StreamOptions {
    #[serde(default)]
    pub include_usage: bool,
}

/// `content` is either a bare string or an array of `{type, text}` parts.
#[derive(Debug, Deserialize, Default)]
#[serde(untagged)]
pub(crate) enum Content {
    #[default]
    Null,
    Text(String),
    Parts(Vec<ContentPart>),
}

#[derive(Debug, Deserialize)]
pub(crate) struct ContentPart {
    #[serde(default)]
    pub text: String,
}

impl Content {
    /// Collapse string or content-parts into plain text.
    pub(crate) fn flatten(&self) -> String {
        match self {
            Content::Null => String::new(),
            Content::Text(s) => s.clone(),
            Content::Parts(parts) => parts
                .iter()
                .map(|p| p.text.as_str())
                .collect::<Vec<_>>()
                .join(""),
        }
    }
}

impl ChatCompletionRequest {
    pub(crate) fn into_neutral(self) -> NeutralRequest {
        let include_usage = self
            .stream_options
            .as_ref()
            .is_some_and(|o| o.include_usage);

        NeutralRequest {
            model: self.model,
            messages: self
                .messages
                .into_iter()
                .map(|m| Message {
                    role: m.role,
                    content: m.content.flatten(),
                })
                .collect(),
            stream: self.stream,
            include_usage,
        }
    }
}
