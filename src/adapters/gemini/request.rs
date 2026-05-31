//! Parsing of the Google Gemini `generateContent` request body into the neutral
//! model. The model name and the streaming flag come from the URL, not the body.

use serde::Deserialize;

use crate::core::{Message, NeutralRequest};

#[derive(Debug, Deserialize)]
pub(crate) struct GenerateRequest {
    #[serde(default)]
    pub contents: Vec<GeminiContent>,
    #[serde(default, rename = "systemInstruction", alias = "system_instruction")]
    pub system_instruction: Option<GeminiContent>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct GeminiContent {
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub parts: Vec<GeminiPart>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct GeminiPart {
    #[serde(default)]
    pub text: Option<String>,
}

impl GeminiContent {
    fn text(&self) -> String {
        self.parts
            .iter()
            .filter_map(|p| p.text.as_deref())
            .collect()
    }
}

impl GenerateRequest {
    pub(crate) fn into_neutral(self, model: String, stream: bool) -> NeutralRequest {
        let mut messages = Vec::new();

        if let Some(sys) = &self.system_instruction {
            let text = sys.text();
            if !text.is_empty() {
                messages.push(Message {
                    role: "system".to_string(),
                    content: text,
                });
            }
        }

        for c in &self.contents {
            // Gemini uses "model" for the assistant role; normalise to "user"
            // for anything else so request matching sees user text.
            let role = match c.role.as_deref() {
                Some("model") => "assistant",
                _ => "user",
            };
            messages.push(Message {
                role: role.to_string(),
                content: c.text(),
            });
        }

        NeutralRequest {
            model,
            messages,
            stream,
            include_usage: false,
        }
    }
}
