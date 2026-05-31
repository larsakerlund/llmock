//! Command-line / environment configuration.

use std::net::IpAddr;
use std::path::PathBuf;

use clap::Parser;

#[derive(Debug, Parser)]
#[command(name = "llmock", version, about = "Emulate LLM provider HTTP APIs with canned fixtures.")]
pub struct Config {
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

    /// Default streaming granularity: `word`, `char`, or a positive integer
    /// (characters per chunk). Overridden per-rule by `stream.chunk_by`.
    #[arg(long, env = "LLMOCK_CHUNK_BY", default_value = "word")]
    pub default_chunk_by: String,
}

impl Config {
    /// Resolve the global streaming defaults, validating `default_chunk_by`.
    pub fn stream_defaults(&self) -> Result<crate::core::StreamSpec, String> {
        Ok(crate::core::StreamSpec {
            ttft_ms: self.default_ttft_ms,
            inter_token_ms: self.default_inter_token_ms,
            chunk_by: crate::core::ChunkBy::parse(&self.default_chunk_by)?,
        })
    }
}
