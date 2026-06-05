//! Command-line / environment configuration.

use std::net::IpAddr;
use std::path::PathBuf;

use clap::Parser;

#[derive(Debug, Parser)]
#[command(
    name = "llmock",
    version,
    about = "Emulate LLM provider HTTP APIs with canned fixtures."
)]
pub(crate) struct Config {
    /// Address to bind.
    #[arg(long, env = "LLMOCK_HOST", default_value = "127.0.0.1")]
    pub host: IpAddr,

    /// Port to listen on.
    #[arg(long, env = "LLMOCK_PORT", default_value_t = 8080)]
    pub port: u16,

    /// Path to a YAML fixtures file. If omitted, a built-in default fixture is
    /// used (a single fallback response).
    #[arg(long, env = "LLMOCK_FIXTURES")]
    pub fixtures: Option<PathBuf>,

    /// Default time-to-first-token for streaming, in ms. Overridden per-rule by
    /// a fixture's `stream.ttft_ms`.
    #[arg(long, env = "LLMOCK_TTFT_MS", default_value_t = 0)]
    pub default_ttft_ms: u64,

    /// Default delay between streamed deltas, in ms. Overridden per-rule by a
    /// fixture's `stream.inter_token_ms`.
    #[arg(long, env = "LLMOCK_INTER_TOKEN_MS", default_value_t = 0)]
    pub default_inter_token_ms: u64,

    /// Default random +/- variation on each inter-token delay, in ms, so
    /// synthesized streams don't pace perfectly evenly. Overridden per-rule by
    /// `stream.jitter_ms`.
    #[arg(long, env = "LLMOCK_JITTER_MS", default_value_t = 0)]
    pub default_jitter_ms: u64,

    /// Default streaming granularity: `word`, `char`, or a positive integer
    /// (characters per chunk). Overridden per-rule by `stream.chunk_by`.
    #[arg(long, env = "LLMOCK_CHUNK_BY", default_value = "word")]
    pub default_chunk_by: String,

    /// Make ids and timestamps reproducible (monotonic counter, fixed time) so
    /// responses are byte-stable — useful for snapshot testing.
    #[arg(long, env = "LLMOCK_DETERMINISTIC", default_value_t = false)]
    pub deterministic: bool,

    /// Directory of record/replay cassettes. When set, a request matching a
    /// stored cassette is replayed byte-for-byte before fixtures are consulted.
    #[arg(long, env = "LLMOCK_CASSETTE_DIR")]
    pub cassette_dir: Option<PathBuf>,

    /// Record mode: proxy a request with no matching cassette to the real
    /// upstream, save the exchange under `--cassette-dir`, and return the real
    /// bytes. Requires `--cassette-dir`.
    #[arg(long, env = "LLMOCK_RECORD", default_value_t = false)]
    pub record: bool,

    /// Override the upstream base URL used in record mode (default: the real
    /// provider, chosen by request path). Mainly for testing.
    #[arg(long, env = "LLMOCK_UPSTREAM")]
    pub upstream: Option<String>,

    /// Speed factor applied to a streamed cassette's recorded timing on replay:
    /// `1.0` reproduces the real timing, `2.0` is twice as fast, `0.5` half
    /// speed, `0` replays instantly (useful for fast test suites).
    #[arg(long, env = "LLMOCK_REPLAY_SPEED", default_value_t = 1.0)]
    pub replay_speed: f64,
}

impl Config {
    /// Resolve the global streaming defaults, validating `default_chunk_by`.
    pub(crate) fn stream_defaults(&self) -> Result<crate::core::StreamSpec, String> {
        Ok(crate::core::StreamSpec {
            ttft_ms: self.default_ttft_ms,
            inter_token_ms: self.default_inter_token_ms,
            jitter_ms: self.default_jitter_ms,
            chunk_by: crate::core::ChunkBy::parse(&self.default_chunk_by)?,
        })
    }
}
