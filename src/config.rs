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
}
