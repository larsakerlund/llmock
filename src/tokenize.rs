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

/// OpenAI model-name prefixes for which tiktoken applies.
const OPENAI_PREFIXES: &[&str] = &["gpt", "o1", "o3", "o4", "chatgpt"];

/// Whether the model name belongs to the OpenAI family (so tiktoken applies).
/// Expects an already-lowercased name (see [`estimate_usage`]).
fn is_openai(model: &str) -> bool {
    OPENAI_PREFIXES.iter().any(|p| model.starts_with(p))
}

/// The tiktoken encoding for an OpenAI model: `cl100k_base` for the GPT-4/3.5
/// generation, `o200k_base` for gpt-4o / gpt-4.1 / gpt-5 / o-series (and as the
/// modern default). Expects an already-lowercased name (see [`estimate_usage`]).
fn openai_encoding(model: &str) -> &'static CoreBPE {
    if model.starts_with("gpt-3.5") || model == "gpt-4" || model.starts_with("gpt-4-") {
        cl100k()
    } else {
        o200k()
    }
}

/// tiktoken BPE is roughly linear but heavy; above ~1M input chars fall back to
/// the cheap chars/4 estimate. The request body cap (`--max-body-bytes`) is the
/// primary bound on input size; this is a second-order guard so a single
/// max-body request cannot pin a CPU running BPE.
const TIKTOKEN_CHAR_LIMIT: usize = 1_000_000;

#[allow(clippy::cast_possible_truncation)]
pub(crate) fn estimate_usage(
    model: &str,
    messages: &[Message],
    completion: &str,
    tool_calls: &[ToolCall],
) -> Usage {
    // Normalize the model name once; both helpers expect lowercase.
    let lower = model.to_ascii_lowercase();
    let total_chars: usize = messages
        .iter()
        .map(|m| m.role.chars().count() + m.content.chars().count())
        .sum::<usize>()
        + completion.chars().count()
        + tool_calls
            .iter()
            .map(|t| t.name.chars().count() + t.arguments.chars().count())
            .sum::<usize>();
    if is_openai(&lower) && total_chars <= TIKTOKEN_CHAR_LIMIT {
        let enc = openai_encoding(&lower);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// An OpenAI-model input above the tiktoken char limit must skip BPE and use
    /// the chars/4 fallback, still returning a sane non-zero estimate. This proves
    /// the short-circuit engages without running BPE over a huge string.
    #[test]
    fn huge_openai_input_uses_chars4_fallback_without_panicking() {
        let big = "a".repeat(TIKTOKEN_CHAR_LIMIT + 1);
        let messages = [Message {
            role: "user".to_string(),
            content: big.clone(),
        }];
        let usage = estimate_usage("gpt-4o", &messages, "", &[]);
        // Fallback: 3 + (4 + content_chars/4) for the single message.
        let expected_prompt = 3 + 4 + big.chars().count() / 4;
        assert_eq!(usage.prompt_tokens, u32::try_from(expected_prompt).unwrap());
        assert_eq!(usage.completion_tokens, 0);
    }
}
