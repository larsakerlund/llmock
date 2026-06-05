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

/// A single inter-token delay. With `burstiness > 0` the cadence clumps into
/// bursts (most gaps zero, the rest a longer pause) like a real stream;
/// otherwise it's the base delay with `jitter_ms` of uniform variation. Both
/// are disabled in deterministic mode for reproducible runs.
pub(crate) fn inter_token_delay(spec: &StreamSpec) -> Option<Duration> {
    let base = spec.inter_token_ms;
    if util::is_deterministic() {
        return delay(base);
    }
    let mut rng = rand::thread_rng();
    if spec.burstiness > 0.0 {
        delay(burst_gap(base, spec.burstiness, &mut rng))
    } else if spec.jitter_ms == 0 {
        delay(base)
    } else {
        let lo = base.saturating_sub(spec.jitter_ms);
        let hi = base.saturating_add(spec.jitter_ms);
        delay(rng.gen_range(lo..=hi))
    }
}

/// Mean-preserving burst mixture: with probability `b` the gap is 0; otherwise
/// it's an exponential draw with mean `base / (1 - b)`. The average stays
/// `base`, but most tokens fire instantly and the rest pause — matching the
/// measured real shape (median ~0, occasional spikes). Capped at 25x the pause
/// mean to avoid a pathological multi-second stall.
fn burst_gap(base: u64, burstiness: f64, rng: &mut impl Rng) -> u64 {
    let b = burstiness.clamp(0.0, 0.95);
    if base == 0 || rng.gen_range(0.0..1.0) < b {
        return 0;
    }
    #[allow(clippy::cast_precision_loss)]
    let pause_mean = base as f64 / (1.0 - b);
    let u: f64 = rng.gen_range(0.0..1.0);
    let sample = (-pause_mean * (1.0 - u).ln()).min(pause_mean * 25.0);
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    {
        sample.round() as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::cast_precision_loss)]
    fn burst_gap_preserves_mean_and_clumps() {
        let mut rng = rand::thread_rng();
        let (base, b, n) = (20u64, 0.75, 50_000usize);
        let samples: Vec<u64> = (0..n).map(|_| burst_gap(base, b, &mut rng)).collect();
        let zero_frac = samples.iter().filter(|&&x| x == 0).count() as f64 / n as f64;
        let mean = samples.iter().sum::<u64>() as f64 / n as f64;
        // Most gaps are zero (median 0), but the average is preserved at `base`.
        assert!(
            zero_frac > 0.6,
            "expected mostly-zero gaps, got {zero_frac}"
        );
        assert!(
            (mean - base as f64).abs() < 4.0,
            "mean should ~= base, got {mean}"
        );
    }

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
