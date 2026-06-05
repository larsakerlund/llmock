//! Provider-neutral streaming helpers: splitting response text into pieces and
//! the timing model. The actual SSE framing lives in each adapter (it differs
//! per provider); this module only decides *what* pieces to emit and *when*.

use std::time::Duration;

use rand::Rng;

use crate::core::{ChunkBy, StreamSpec};
use crate::util;

/// Split `content` into the ordered pieces that will become individual deltas.
///
/// `Word` keeps each word's trailing whitespace attached so concatenating the
/// pieces reproduces the original text exactly (no lost or doubled spaces).
pub(crate) fn chunk_text(content: &str, chunk_by: ChunkBy) -> Vec<String> {
    match chunk_by {
        ChunkBy::Char => content.chars().map(|c| c.to_string()).collect(),
        ChunkBy::Chars(n) => {
            let n = n.max(1);
            let chars: Vec<char> = content.chars().collect();
            chars.chunks(n).map(|c| c.iter().collect()).collect()
        }
        ChunkBy::Word => split_keeping_whitespace(content),
    }
}

/// Split into "word + following whitespace" pieces. Leading whitespace attaches
/// to the first piece. Joining all pieces yields the original string.
fn split_keeping_whitespace(s: &str) -> Vec<String> {
    let mut pieces = Vec::new();
    let mut current = String::new();
    let mut in_word = false;

    for c in s.chars() {
        if c.is_whitespace() {
            current.push(c);
            in_word = false;
        } else {
            // A non-space starting a new word after whitespace closes the piece.
            if !in_word && !current.is_empty() {
                pieces.push(std::mem::take(&mut current));
            }
            current.push(c);
            in_word = true;
        }
    }
    if !current.is_empty() {
        pieces.push(current);
    }
    pieces
}

/// Duration helper that treats 0 as "no delay" (skips the sleep entirely).
pub(crate) fn delay(ms: u64) -> Option<Duration> {
    if ms == 0 {
        None
    } else {
        Some(Duration::from_millis(ms))
    }
}

/// Delay before the delta at `index`: TTFT for the first, otherwise a jittered
/// inter-token delay.
pub(crate) fn step_delay(spec: &StreamSpec, index: usize) -> Option<Duration> {
    if index == 0 {
        delay(spec.ttft_ms)
    } else {
        inter_token_delay(spec)
    }
}

/// A single inter-token delay with `jitter_ms` of uniform +/- variation, so a
/// synthesized stream's cadence isn't perfectly even. Jitter is disabled in
/// deterministic mode for reproducible runs.
pub(crate) fn inter_token_delay(spec: &StreamSpec) -> Option<Duration> {
    let base = spec.inter_token_ms;
    if spec.jitter_ms == 0 || util::is_deterministic() {
        return delay(base);
    }
    let lo = base.saturating_sub(spec.jitter_ms);
    let hi = base.saturating_add(spec.jitter_ms);
    delay(rand::thread_rng().gen_range(lo..=hi))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_pieces_roundtrip() {
        for s in ["Hello, world!", "  leading", "trailing  ", "a  b   c", ""] {
            let joined: String = chunk_text(s, ChunkBy::Word).concat();
            assert_eq!(joined, s, "word chunking must be lossless for {s:?}");
        }
    }

    #[test]
    fn char_pieces_roundtrip() {
        let s = "héllo";
        assert_eq!(chunk_text(s, ChunkBy::Char).concat(), s);
        assert_eq!(chunk_text(s, ChunkBy::Char).len(), 5);
    }

    #[test]
    fn fixed_runs() {
        let p = chunk_text("abcde", ChunkBy::Chars(2));
        assert_eq!(p, vec!["ab", "cd", "e"]);
    }
}
