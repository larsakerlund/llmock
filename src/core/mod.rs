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

/// How to split response text into streamed pieces.
#[derive(Debug, Clone, Copy)]
pub enum ChunkBy {
    /// One delta per whitespace-delimited word (whitespace kept with the word).
    Word,
    /// One delta per character.
    Char,
    /// One delta per fixed run of `n` characters.
    Chars(usize),
}

impl ChunkBy {
    /// Parse from a config string: `word`, `char`, or a positive integer
    /// (characters per chunk).
    pub fn parse(s: &str) -> Result<ChunkBy, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "word" => Ok(ChunkBy::Word),
            "char" | "character" => Ok(ChunkBy::Char),
            other => other
                .parse::<usize>()
                .ok()
                .filter(|n| *n > 0)
                .map(ChunkBy::Chars)
                .ok_or_else(|| {
                    format!("invalid chunk_by {s:?} (expected `word`, `char`, or a positive integer)")
                }),
        }
    }
}

/// Provider-neutral streaming behaviour: how fast and in what granularity to
/// emit deltas. Applies to any adapter's streaming serializer.
#[derive(Debug, Clone, Copy)]
pub struct StreamSpec {
    /// Delay before the first content delta (time-to-first-token), in ms.
    pub ttft_ms: u64,
    /// Delay between subsequent content deltas, in ms.
    pub inter_token_ms: u64,
    pub chunk_by: ChunkBy,
}

impl Default for StreamSpec {
    fn default() -> Self {
        StreamSpec {
            ttft_ms: 0,
            inter_token_ms: 0,
            chunk_by: ChunkBy::Word,
        }
    }
}

/// A canned response in neutral form, produced by the fixture engine.
#[derive(Debug, Clone)]
pub struct NeutralResponse {
    pub model: String,
    pub content: String,
    pub stop_reason: StopReason,
    pub usage: Usage,
    /// How this response should stream, if the request asked for streaming.
    pub stream: StreamSpec,
}
