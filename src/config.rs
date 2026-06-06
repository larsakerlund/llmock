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

    /// Time-to-first-token for streaming, in ms. Unset uses a realistic
    /// per-model default (TTFT dominates streaming latency); set `0` for instant
    /// streaming in fast test suites. Overridden per-rule by `stream.ttft_ms`.
    #[arg(long, env = "LLMOCK_TTFT_MS")]
    pub default_ttft_ms: Option<u64>,

    /// Delay between streamed deltas, in ms. Unset uses the per-model default.
    /// Overridden per-rule by `stream.inter_token_ms`.
    #[arg(long, env = "LLMOCK_INTER_TOKEN_MS")]
    pub default_inter_token_ms: Option<u64>,

    /// Random +/- variation on each inter-token delay, in ms (used only when
    /// burstiness is 0). Overridden per-rule by `stream.jitter_ms`.
    #[arg(long, env = "LLMOCK_JITTER_MS")]
    pub default_jitter_ms: Option<u64>,

    /// Stream burstiness (0..1): 0 = even pacing, higher clumps tokens into
    /// bursts with occasional pauses like a real model, keeping the average gap
    /// at `inter_token_ms`. Unset uses the per-model default. Overridden per-rule
    /// by `stream.burstiness`.
    #[arg(long, env = "LLMOCK_BURSTINESS")]
    pub default_burstiness: Option<f64>,

    /// Streaming granularity: `word`, `char`, or a positive integer (characters
    /// per chunk). Unset uses the per-model default. Overridden per-rule by
    /// `stream.chunk_by`.
    #[arg(long, env = "LLMOCK_CHUNK_BY")]
    pub default_chunk_by: Option<String>,

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

    /// Silence the warning about record mode on a non-loopback bind. Record mode
    /// forwards your real provider API key to the upstream and is itself
    /// unauthenticated, so a public bind is unauthenticated key-spending and
    /// warns by default. Pass this when the bind is deliberately reachable and
    /// protected by other means.
    #[arg(long, env = "LLMOCK_RECORD_ALLOW_REMOTE", default_value_t = false)]
    pub record_allow_remote: bool,

    /// Override the upstream base URL for all providers in record mode (default:
    /// each provider's real API, chosen by request path). A per-provider
    /// override below takes precedence over this.
    #[arg(long, env = "LLMOCK_UPSTREAM")]
    pub upstream: Option<String>,

    /// Override the upstream base URL for OpenAI requests (chat and responses),
    /// e.g. an Azure OpenAI resource. Takes precedence over --upstream.
    #[arg(long, env = "LLMOCK_UPSTREAM_OPENAI")]
    pub upstream_openai: Option<String>,

    /// Override the upstream base URL for Anthropic requests. Takes precedence
    /// over --upstream.
    #[arg(long, env = "LLMOCK_UPSTREAM_ANTHROPIC")]
    pub upstream_anthropic: Option<String>,

    /// Override the upstream base URL for Gemini requests. Takes precedence over
    /// --upstream.
    #[arg(long, env = "LLMOCK_UPSTREAM_GEMINI")]
    pub upstream_gemini: Option<String>,

    /// Speed factor applied to a streamed cassette's recorded timing on replay:
    /// `1.0` reproduces the real timing, `2.0` is twice as fast, `0.5` half
    /// speed, `0` replays instantly (useful for fast test suites).
    #[arg(long, env = "LLMOCK_REPLAY_SPEED", default_value_t = 1.0)]
    pub replay_speed: f64,
}

impl Config {
    /// Collect the global streaming defaults, validating `default_chunk_by` if
    /// it was set. Unset fields stay `None` (resolved per-model at request time).
    pub(crate) fn stream_defaults(&self) -> Result<crate::core::StreamDefaults, String> {
        let chunk_by = self
            .default_chunk_by
            .as_ref()
            .map(|s| crate::core::ChunkBy::parse(s))
            .transpose()?;
        Ok(crate::core::StreamDefaults {
            ttft_ms: self.default_ttft_ms,
            inter_token_ms: self.default_inter_token_ms,
            jitter_ms: self.default_jitter_ms,
            burstiness: self.default_burstiness,
            chunk_by,
        })
    }

    /// True when record mode should warn: recording onto a bind that is not
    /// loopback, without the explicit remote opt-in. Record mode forwards the
    /// real provider key upstream and is unauthenticated, so a public bind is
    /// unauthenticated key-spending. `IpAddr::is_loopback` covers 127.0.0.0/8 and
    /// `::1`; the wildcard (`0.0.0.0` / `::`) is not loopback, so it warns, which
    /// matters because the container binds the wildcard. Pure so it is
    /// unit-tested without binding a socket.
    pub(crate) fn record_warns_on_public_bind(
        record: bool,
        host: std::net::IpAddr,
        allow_remote: bool,
    ) -> bool {
        record && !allow_remote && !host.is_loopback()
    }
}

#[cfg(test)]
mod tests {
    use super::Config;
    use std::net::IpAddr;

    fn ip(s: &str) -> IpAddr {
        s.parse().expect("valid IP literal")
    }

    #[test]
    fn loopback_v4_does_not_warn() {
        assert!(!Config::record_warns_on_public_bind(
            true,
            ip("127.0.0.1"),
            false
        ));
    }

    #[test]
    fn loopback_v6_does_not_warn() {
        assert!(!Config::record_warns_on_public_bind(true, ip("::1"), false));
    }

    #[test]
    fn wildcard_warns() {
        assert!(Config::record_warns_on_public_bind(
            true,
            ip("0.0.0.0"),
            false
        ));
    }

    #[test]
    fn lan_address_warns() {
        assert!(Config::record_warns_on_public_bind(
            true,
            ip("192.168.1.10"),
            false
        ));
    }

    #[test]
    fn allow_remote_silences_public_bind() {
        assert!(!Config::record_warns_on_public_bind(
            true,
            ip("192.168.1.10"),
            true
        ));
    }

    #[test]
    fn not_recording_never_warns() {
        assert!(!Config::record_warns_on_public_bind(
            false,
            ip("0.0.0.0"),
            false
        ));
    }
}
