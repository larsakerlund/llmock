//! Provider-neutral canonical model.
//!
//! Every protocol adapter parses an incoming request into a [`NeutralRequest`]
//! and serializes a [`NeutralResponse`] back into provider-specific wire bytes.
//! Fixtures are authored against this neutral shape, so one fixture can be
//! served by any adapter.

/// A single chat turn, provider-agnostic.
#[derive(Debug, Clone)]
pub(crate) struct Message {
    pub role: String,
    /// Flattened text content. (Multimodal/content-parts come later.)
    pub content: String,
}

/// What an adapter extracted from an incoming request that the fixture engine
/// can match against. Intentionally small for the MVP; grows as needed.
#[derive(Debug, Clone)]
pub(crate) struct NeutralRequest {
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
    pub(crate) fn last_user_message(&self) -> Option<&str> {
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
pub(crate) enum StopReason {
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
pub(crate) struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
}

impl Usage {
    pub(crate) fn total(self) -> u32 {
        self.prompt_tokens + self.completion_tokens
    }
}

/// How to split response text into streamed pieces.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ChunkBy {
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
    pub(crate) fn parse(s: &str) -> Result<ChunkBy, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "word" => Ok(ChunkBy::Word),
            "char" | "character" => Ok(ChunkBy::Char),
            other => other
                .parse::<usize>()
                .ok()
                .filter(|n| *n > 0)
                .map(ChunkBy::Chars)
                .ok_or_else(|| {
                    format!(
                        "invalid chunk_by {s:?} (expected `word`, `char`, or a positive integer)"
                    )
                }),
        }
    }
}

/// Provider-neutral streaming behaviour: how fast and in what granularity to
/// emit deltas. Applies to any adapter's streaming serializer.
#[derive(Debug, Clone, Copy)]
pub(crate) struct StreamSpec {
    /// Delay before the first content delta (time-to-first-token), in ms.
    pub ttft_ms: u64,
    /// Delay between subsequent content deltas, in ms.
    pub inter_token_ms: u64,
    /// Random +/- variation applied to each inter-token delay, in ms, so the
    /// cadence looks real rather than perfectly even.
    pub jitter_ms: u64,
    pub chunk_by: ChunkBy,
}

impl Default for StreamSpec {
    fn default() -> Self {
        StreamSpec {
            ttft_ms: 0,
            inter_token_ms: 0,
            jitter_ms: 0,
            chunk_by: ChunkBy::Word,
        }
    }
}

/// A mid-stream fault to inject (only meaningful for streaming responses).
/// `after` counts content deltas emitted before the fault triggers.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Fault {
    /// Emit `after` content deltas, then drop the connection without the final
    /// chunk or `[DONE]` — simulates a truncated/dropped stream.
    Truncate { after: usize },
    /// Emit `after` content deltas, then send a malformed SSE frame and stop —
    /// exercises a client's parse-error handling.
    Malformed { after: usize },
    /// Emit `after` content deltas, then stall for `hold_ms` and stop — exercises
    /// a client's read timeout.
    Hang { after: usize, hold_ms: u64 },
}

/// A provider-neutral HTTP error to inject. Adapters render this into their own
/// error-envelope shape and status.
#[derive(Debug, Clone)]
pub(crate) struct InjectError {
    pub status: u16,
    pub error_type: String,
    pub message: String,
    pub code: Option<String>,
    pub param: Option<String>,
}

/// A function/tool call the assistant "made". `arguments` is the JSON arguments
/// as a string (exactly how the providers represent it on the wire).
#[derive(Debug, Clone)]
pub(crate) struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// A canned response in neutral form, produced by the fixture engine.
#[derive(Debug, Clone)]
pub(crate) struct NeutralResponse {
    pub model: String,
    pub content: String,
    /// Tool calls the assistant returns. When non-empty, `content` is typically
    /// empty and the stop reason is `ToolCalls`.
    pub tool_calls: Vec<ToolCall>,
    pub stop_reason: StopReason,
    pub usage: Usage,
    /// How this response should stream, if the request asked for streaming.
    pub stream: StreamSpec,
    /// A mid-stream fault to inject, if any (ignored for non-streaming).
    pub fault: Option<Fault>,
}

/// What the fixture engine decided to do with a request: serve a response or
/// fail with an injected HTTP error.
#[derive(Debug, Clone)]
pub(crate) enum Outcome {
    Respond(NeutralResponse),
    Error(InjectError),
}
