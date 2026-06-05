//! Shared request-content parsing. Several providers encode a message's
//! `content` (and Anthropic's `system`) as either a bare string or an array of
//! `{type, text}` parts; this collapses both to plain text for fixture
//! matching. Lives here, not in any one vendor, since OpenAI and Anthropic both
//! reuse it.

use serde::Deserialize;

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
            Content::Parts(parts) => parts.iter().map(|p| p.text.as_str()).collect(),
        }
    }
}
