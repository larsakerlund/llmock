//! Parsing of the OpenAI Chat Completions request body into the neutral model.

use serde::Deserialize;

use crate::adapters::content::Content;
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
