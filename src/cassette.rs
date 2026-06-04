//! Record/replay cassettes, matched by the **same engine as fixtures**.
//!
//! A cassette captures one real provider exchange. It is matched on the same
//! [`Match`](crate::fixtures::Match) a fixture uses — `model` + last user
//! message — scoped to the endpoint and streaming flag it was recorded for (so
//! a Chat Completions cassette never answers an Anthropic, or a non-streaming,
//! request). So there is one matching model for everything: a request resolves
//! to a replayed cassette or a synthesized fixture by identical rules.
//!
//! Recording proxies a miss to the real upstream, captures the exact bytes (and
//! the real inter-chunk timing for streams), and saves a cassette whose match
//! is derived from the request.

use std::collections::hash_map::DefaultHasher;
use std::convert::Infallible;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use axum::body::{Body, Bytes};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use tokio::time::sleep;

use crate::core::NeutralRequest;
use crate::fixtures::Match;

/// Which provider/wire-format an exchange belongs to. Scopes cassette matching
/// and selects the real upstream when recording.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Endpoint {
    #[serde(rename = "openai.chat")]
    OpenAiChat,
    #[serde(rename = "openai.responses")]
    OpenAiResponses,
    #[serde(rename = "anthropic")]
    Anthropic,
    #[serde(rename = "gemini")]
    Gemini,
}

impl Endpoint {
    pub(crate) fn upstream_base(self) -> &'static str {
        match self {
            Endpoint::OpenAiChat | Endpoint::OpenAiResponses => "https://api.openai.com",
            Endpoint::Anthropic => "https://api.anthropic.com",
            Endpoint::Gemini => "https://generativelanguage.googleapis.com",
        }
    }
}

/// One recorded exchange.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Cassette {
    pub endpoint: Endpoint,
    /// Whether the recorded request was streaming (replays only same-mode).
    #[serde(default)]
    pub stream: bool,
    #[serde(rename = "match")]
    pub match_: Match,
    pub response: StoredResponse,
}

/// The captured response. Non-streaming uses `body`; streaming uses `frames`,
/// each replayed after its recorded inter-chunk delay so the original timing
/// (including time-to-first-byte) is reproduced.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct StoredResponse {
    pub status: u16,
    pub content_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frames: Vec<Frame>,
}

/// One captured streamed chunk: its bytes and the delay since the previous one.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Frame {
    #[serde(default)]
    pub delay_ms: u64,
    pub data: String,
}

impl StoredResponse {
    /// Build the HTTP response. For streaming, each recorded inter-chunk delay is
    /// divided by `speed`: `1.0` reproduces the real timing, `2.0` is twice as
    /// fast, `0.5` half speed, and `0` (or less) replays instantly.
    pub(crate) fn into_response(self, speed: f64) -> Response {
        let status = StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        if self.frames.is_empty() {
            let body = self.body.unwrap_or_default();
            return (status, [(header::CONTENT_TYPE, self.content_type)], body).into_response();
        }
        let frames = self.frames;
        let body = Body::from_stream(async_stream::stream! {
            for frame in frames {
                let ms = scale_delay(frame.delay_ms, speed);
                if ms > 0 {
                    sleep(Duration::from_millis(ms)).await;
                }
                yield Ok::<_, Infallible>(Bytes::from(frame.data));
            }
        });
        Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, self.content_type)
            .header(header::CACHE_CONTROL, "no-cache")
            .body(body)
            .expect("valid streaming response")
    }
}

/// Scale a recorded delay by a replay-speed factor (`<= 0` means instant).
fn scale_delay(delay_ms: u64, speed: f64) -> u64 {
    if speed <= 0.0 {
        return 0;
    }
    if (speed - 1.0).abs() < f64::EPSILON {
        return delay_ms;
    }
    #[allow(clippy::cast_precision_loss, clippy::cast_sign_loss)]
    {
        (delay_ms as f64 / speed).round() as u64
    }
}

/// A loaded set of cassettes, ordered most-specific-first for deterministic
/// matching.
#[derive(Debug, Default)]
pub(crate) struct Cassettes {
    entries: Vec<Cassette>,
}

impl Cassettes {
    /// Load every `*.json` cassette in `dir` (missing dir = empty set).
    pub(crate) fn load(dir: &Path) -> Result<Self, String> {
        let mut entries = Vec::new();
        if dir.is_dir() {
            let rd =
                std::fs::read_dir(dir).map_err(|e| format!("reading {}: {e}", dir.display()))?;
            for entry in rd {
                let path = entry.map_err(|e| e.to_string())?.path();
                if path.extension().and_then(std::ffi::OsStr::to_str) == Some("json") {
                    let text = std::fs::read_to_string(&path)
                        .map_err(|e| format!("{}: {e}", path.display()))?;
                    let cassette: Cassette = serde_json::from_str(&text)
                        .map_err(|e| format!("{}: {e}", path.display()))?;
                    entries.push(cassette);
                }
            }
        }
        // Longer `user_contains` is more specific, so try it first.
        entries.sort_by(|a, b| b.match_.specificity().cmp(&a.match_.specificity()));
        Ok(Cassettes { entries })
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    /// First cassette for this endpoint + streaming mode whose `match` holds.
    pub(crate) fn find(&self, endpoint: Endpoint, req: &NeutralRequest) -> Option<&StoredResponse> {
        self.entries
            .iter()
            .find(|c| c.endpoint == endpoint && c.stream == req.stream && c.match_.matches(req))
            .map(|c| &c.response)
    }
}

/// Persist a cassette to `dir/<hash>.json`.
fn save(dir: &Path, cassette: &Cassette) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    let mut hasher = DefaultHasher::new();
    serde_json::to_string(cassette)
        .unwrap_or_default()
        .hash(&mut hasher);
    let path = dir.join(format!("{:016x}.json", hasher.finish()));
    let text = serde_json::to_string_pretty(cassette).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("writing {}: {e}", path.display()))?;
    Ok(path)
}

/// Configuration for record mode.
#[derive(Debug, Clone)]
pub(crate) struct RecordConfig {
    pub dir: PathBuf,
    /// Override the upstream base URL (default: the endpoint's real provider).
    pub upstream: Option<String>,
}

/// Headers worth forwarding to the upstream (auth + content negotiation).
fn forwardable(headers: &HeaderMap) -> Vec<(reqwest::header::HeaderName, String)> {
    const KEEP: &[&str] = &[
        "authorization",
        "x-api-key", // Anthropic, Azure OpenAI
        "api-key",   // Azure OpenAI
        "anthropic-version",
        "anthropic-beta",
        "x-goog-api-key", // Gemini
        "content-type",
        "accept",
        "openai-organization",
        "openai-beta",
    ];
    headers
        .iter()
        .filter(|(name, _)| KEEP.contains(&name.as_str()))
        .filter_map(|(name, value)| {
            let n = reqwest::header::HeaderName::from_bytes(name.as_str().as_bytes()).ok()?;
            Some((n, value.to_str().ok()?.to_string()))
        })
        .collect()
}

/// Proxy the request to the real upstream and capture the exchange. `path` is
/// the provider path (already prefix-stripped by routing). On success the
/// exchange is saved and the captured response returned to replay to the client.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn record(
    client: &reqwest::Client,
    rec: &RecordConfig,
    endpoint: Endpoint,
    neutral: &NeutralRequest,
    path: &str,
    query: &str,
    method: &reqwest::Method,
    raw_body: &Bytes,
    headers: &HeaderMap,
) -> Response {
    let base = rec
        .upstream
        .clone()
        .unwrap_or_else(|| endpoint.upstream_base().to_string());
    let url = if query.is_empty() {
        format!("{base}{path}")
    } else {
        format!("{base}{path}?{query}")
    };

    let mut builder = client.request(method.clone(), &url).body(raw_body.to_vec());
    for (name, value) in forwardable(headers) {
        builder = builder.header(name, value);
    }
    // Start the clock before sending, so the first frame's delay captures the
    // real time-to-first-byte (TTFT) — for streaming models that's most of the
    // perceived latency, and `send()` blocks through it.
    let request_start = Instant::now();
    let mut upstream = match builder.send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("record: upstream request to {url} failed: {e}");
            return (
                StatusCode::BAD_GATEWAY,
                format!("llmock record: upstream request failed: {e}"),
            )
                .into_response();
        }
    };

    let status = upstream.status().as_u16();
    let content_type = upstream
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/json")
        .to_string();

    let response = if content_type.contains("text/event-stream") {
        let mut frames = Vec::new();
        let mut last = request_start;
        loop {
            match upstream.chunk().await {
                Ok(Some(chunk)) => {
                    let now = Instant::now();
                    let delay_ms = u64::try_from(now.duration_since(last).as_millis()).unwrap_or(0);
                    last = now;
                    frames.push(Frame {
                        delay_ms,
                        data: String::from_utf8_lossy(&chunk).into_owned(),
                    });
                }
                Ok(None) => break,
                Err(e) => {
                    tracing::error!("record: error reading upstream stream: {e}");
                    break;
                }
            }
        }
        StoredResponse {
            status,
            content_type,
            body: None,
            frames,
        }
    } else {
        let bytes = upstream.bytes().await.unwrap_or_default();
        StoredResponse {
            status,
            content_type,
            body: Some(String::from_utf8_lossy(&bytes).into_owned()),
            frames: Vec::new(),
        }
    };

    let cassette = Cassette {
        endpoint,
        stream: neutral.stream,
        match_: Match::for_request(neutral),
        response: response.clone(),
    };
    match save(&rec.dir, &cassette) {
        Ok(p) => tracing::info!("recorded cassette {}", p.display()),
        Err(e) => tracing::error!("record: could not save cassette: {e}"),
    }

    // Serve the just-captured response back at its real timing.
    response.into_response(1.0)
}
