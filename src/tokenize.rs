//! Token-count estimation for synthesized `usage`.
//!
//! OpenAI models use the real `tiktoken` encodings (exact, including the
//! chat-message overhead that `api.openai.com` reports). Providers without a
//! public tokenizer — Anthropic, Gemini — fall back to a ~4-chars/token
//! estimate; for exact counts there, record a cassette (it carries the real
//! `usage`).

use std::sync::OnceLock;

use tiktoken_rs::CoreBPE;

use crate::core::{Message, ToolCall, Usage};

fn o200k() -> &'static CoreBPE {
    static E: OnceLock<CoreBPE> = OnceLock::new();
    E.get_or_init(|| tiktoken_rs::o200k_base().expect("o200k_base"))
}

fn cl100k() -> &'static CoreBPE {
    static E: OnceLock<CoreBPE> = OnceLock::new();
    E.get_or_init(|| tiktoken_rs::cl100k_base().expect("cl100k_base"))
}

/// Whether the model name belongs to the OpenAI family (so tiktoken applies).
fn is_openai(model: &str) -> bool {
    let m = model.to_ascii_lowercase();
    m.starts_with("gpt")
        || m.starts_with("o1")
        || m.starts_with("o3")
        || m.starts_with("o4")
        || m.starts_with("chatgpt")
}

/// The tiktoken encoding for an OpenAI model: `cl100k_base` for the GPT-4/3.5
/// generation, `o200k_base` for gpt-4o / gpt-4.1 / gpt-5 / o-series (and as the
/// modern default).
fn openai_encoding(model: &str) -> &'static CoreBPE {
    let m = model.to_ascii_lowercase();
    if m.starts_with("gpt-3.5") || m == "gpt-4" || m.starts_with("gpt-4-") {
        cl100k()
    } else {
        o200k()
    }
}

#[allow(clippy::cast_possible_truncation)]
pub(crate) fn estimate_usage(
    model: &str,
    messages: &[Message],
    completion: &str,
    tool_calls: &[ToolCall],
) -> Usage {
    if is_openai(model) {
        let enc = openai_encoding(model);
        // Chat-format accounting: 3 tokens per message + role + content, plus 3
        // tokens priming the assistant reply (matches num_tokens_from_messages).
        let mut prompt = 3usize;
        for m in messages {
            prompt += 3 + count(enc, &m.role) + count(enc, &m.content);
        }
        let mut completion_tokens = count(enc, completion);
        for t in tool_calls {
            completion_tokens += count(enc, &t.name) + count(enc, &t.arguments);
        }
        Usage {
            prompt_tokens: prompt as u32,
            completion_tokens: completion_tokens as u32,
        }
    } else {
        // No public tokenizer: ~4 chars/token, plus a few tokens of per-message
        // framing overhead (real APIs count message structure too). Lands within
        // a token or two of real Anthropic counts in practice.
        let prompt: usize = 3 + messages
            .iter()
            .map(|m| 4 + m.content.chars().count() / 4)
            .sum::<usize>();
        let completion = completion.chars().count() / 4
            + tool_calls
                .iter()
                .map(|t| t.arguments.chars().count() / 4)
                .sum::<usize>();
        Usage {
            prompt_tokens: prompt as u32,
            completion_tokens: completion as u32,
        }
    }
}

fn count(enc: &CoreBPE, text: &str) -> usize {
    enc.encode_ordinary(text).len()
}
