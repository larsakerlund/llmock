//! Provider-neutral canonical model.
//!
//! Every protocol adapter parses an incoming request into a [`NeutralRequest`]
//! and serializes a [`NeutralResponse`] back into provider-specific wire bytes.
//! Fixtures are authored against this neutral shape, so one fixture can be
//! served by any adapter.

/// A single chat turn, provider-agnostic.
#[derive(Debug, Clone)]
pub struct Message {
    pub role: String,
    /// Flattened text content. (Multimodal/content-parts come later.)
    pub content: String,
}

/// What an adapter extracted from an incoming request that the fixture engine
/// can match against. Intentionally small for the MVP; grows as needed.
#[derive(Debug, Clone)]
pub struct NeutralRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub stream: bool,
    /// True when the caller asked for usage on the final stream chunk
    /// (OpenAI `stream_options.include_usage`). Consumed by the streaming
    /// milestone.
    #[allow(dead_code)]
    pub include_usage: bool,
}

impl NeutralRequest {
    /// The text of the last user message, if any — the most common match key.
    pub fn last_user_message(&self) -> Option<&str> {
        self.messages
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .map(|m| m.content.as_str())
    }
}

/// Why generation stopped, in neutral terms. Adapters map these to their own
/// vocabulary (OpenAI `finish_reason`, Anthropic `stop_reason`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// Natural end of turn.
    Stop,
    /// Hit the max token limit.
    Length,
    /// Model emitted tool calls.
    ToolCalls,
    /// Output filtered.
    ContentFilter,
}

/// Token accounting. Adapters render this into their own usage object.
#[derive(Debug, Clone, Copy, Default)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
}

impl Usage {
    pub fn total(&self) -> u32 {
        self.prompt_tokens + self.completion_tokens
    }
}

/// A canned response in neutral form, produced by the fixture engine.
#[derive(Debug, Clone)]
pub struct NeutralResponse {
    pub model: String,
    pub content: String,
    pub stop_reason: StopReason,
    pub usage: Usage,
}
