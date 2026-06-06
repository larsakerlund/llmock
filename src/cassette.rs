//! Record/replay cassettes, matched by the **same engine as fixtures**.
//!
//! A cassette captures one real provider exchange. It is matched on the same
//! [`Match`] a fixture uses (`model` + last user message), scoped to the
//! endpoint and streaming flag it was recorded for (so a Chat Completions
//! cassette never answers an Anthropic, or a non-streaming, request). So there
//! is one matching model for everything: a request resolves to a replayed
//! cassette or a synthesized fixture by identical rules.
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
use axum::http::{HeaderMap, StatusCode, header};
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
// `into_response` streams via `async_stream::stream!`, whose expansion contains
// `unsafe` (from pin-project). This crate writes no unsafe itself (it's
// forbidden); the lint only sees the macro's, so silence it on this type.
#[allow(clippy::unsafe_derive_deserialize)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct StoredResponse {
    pub status: u16,
    pub content_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// Non-streaming response latency (request to full body), replayed before the
    /// body so a non-streamed cassette takes as long as the real call did. A
    /// provider generates the whole response server-side before replying, so this
    /// is the equivalent of a stream's total time.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub delay_ms: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frames: Vec<Frame>,
}

#[allow(clippy::trivially_copy_pass_by_ref)] // serde skip_serializing_if needs &T
fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// One captured streamed chunk: its bytes and the delay since the previous one.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Frame {
    #[serde(default)]
    pub delay_ms: u64,
    pub data: String,
}

impl StoredResponse {
    /// Build the HTTP response. Recorded delays are divided by `speed`: `1.0`
    /// reproduces the real timing, `2.0` is twice as fast, `0.5` half speed, and
    /// `0` (or less) replays instantly. For streaming that scales each inter-chunk
    /// delay; for non-streaming it scales the single request-to-body latency,
    /// slept before the body so a replay is as slow as the real call.
    pub(crate) async fn into_response(self, speed: f64) -> Response {
        let status = StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        if self.frames.is_empty() {
            let ms = scale_delay(self.delay_ms, speed);
            if ms > 0 {
                sleep(Duration::from_millis(ms)).await;
            }
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
        entries.sort_by_key(|c| std::cmp::Reverse(c.match_.specificity()));
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
    // Cassettes hold the upstream response body verbatim; keep them owner-only.
    // create_dir_all is a no-op when the dir exists, but re-tightening on each
    // save is intended hardening, not a bug.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| format!("securing {}: {e}", dir.display()))?;
    }
    let mut hasher = DefaultHasher::new();
    serde_json::to_string(cassette)
        .unwrap_or_default()
        .hash(&mut hasher);
    let path = dir.join(format!("{:016x}.json", hasher.finish()));
    let text = serde_json::to_string_pretty(cassette).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("writing {}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("securing {}: {e}", path.display()))?;
    }
    Ok(path)
}

/// Configuration for record mode.
#[derive(Debug, Clone)]
pub(crate) struct RecordConfig {
    pub dir: PathBuf,
    /// All-providers override of the upstream base URL.
    pub upstream: Option<String>,
    /// Per-provider overrides, each taking precedence over `upstream`. OpenAI
    /// covers both the chat and responses endpoints.
    pub upstream_openai: Option<String>,
    pub upstream_anthropic: Option<String>,
    pub upstream_gemini: Option<String>,
}

impl RecordConfig {
    /// The upstream base for an endpoint: its per-provider override, else the
    /// all-providers `upstream`, else the provider's real default. Lets one
    /// recording run relocate providers independently (e.g. OpenAI to Azure
    /// while Anthropic stays on its default).
    fn base_for(&self, endpoint: Endpoint) -> &str {
        let per_provider = match endpoint {
            Endpoint::OpenAiChat | Endpoint::OpenAiResponses => &self.upstream_openai,
            Endpoint::Anthropic => &self.upstream_anthropic,
            Endpoint::Gemini => &self.upstream_gemini,
        };
        per_provider
            .as_deref()
            .or(self.upstream.as_deref())
            .unwrap_or_else(|| endpoint.upstream_base())
    }
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

/// Redact secret-bearing query-param values (`key`, `api_key`, `access_token`,
/// case-insensitive) for logging or client-facing messages. The Gemini SDK
/// passes the API key as `?key=SECRET`; never log it raw. Returns the query with
/// those values replaced by `REDACTED`, order and other params preserved. Matches
/// by raw param name and does not url-decode.
fn redact_query(query: &str) -> String {
    const SECRET_PARAMS: &[&str] = &["key", "api_key", "access_token"];
    query
        .split('&')
        .map(|pair| {
            let (name, _val) = pair.split_once('=').unwrap_or((pair, ""));
            if !pair.is_empty() && SECRET_PARAMS.iter().any(|p| name.eq_ignore_ascii_case(p)) {
                format!("{name}=REDACTED")
            } else {
                pair.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("&")
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
    let base = rec.base_for(endpoint);
    let url = if query.is_empty() {
        format!("{base}{path}")
    } else {
        format!("{base}{path}?{query}")
    };
    // A log/error-safe copy of the URL: only the query can carry `?key=SECRET`,
    // so redact its secret params. `url` (with the real key) is used solely for
    // the outbound request; everything human-visible uses `safe_url`.
    let safe_url = if query.is_empty() {
        format!("{base}{path}")
    } else {
        format!("{base}{path}?{}", redact_query(query))
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
            tracing::error!("record: upstream request to {safe_url} failed: {e}");
            // The client body stays generic: the reqwest error may embed the full
            // URL with the key, so it never reaches the caller. Redacted detail
            // lives in the server log above.
            return (
                StatusCode::BAD_GATEWAY,
                "llmock record: upstream request failed",
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
                    // Abort rather than save a cassette truncated by a transient
                    // upstream error: the partial frames would persist under the
                    // healthy upstream status and replay forever as if complete,
                    // with no marker that they were cut short. Mirror the
                    // send-error and non-stream body-read handling: a generic,
                    // redacted BAD_GATEWAY, with detail only in the server log.
                    tracing::error!("record: error reading upstream stream from {safe_url}: {e}");
                    return (
                        StatusCode::BAD_GATEWAY,
                        "llmock record: upstream stream read failed",
                    )
                        .into_response();
                }
            }
        }
        StoredResponse {
            status,
            content_type,
            body: None,
            delay_ms: 0,
            frames,
        }
    } else {
        // Abort rather than save a cassette from a partial read: a swallowed body
        // error would persist an empty body under a healthy status and replay it
        // forever. Mirror the send-error handling above.
        let bytes = match upstream.bytes().await {
            Ok(b) => b,
            Err(e) => {
                tracing::error!("record: error reading upstream body from {safe_url}: {e}");
                return (
                    StatusCode::BAD_GATEWAY,
                    "llmock record: upstream body read failed",
                )
                    .into_response();
            }
        };
        // The whole call's latency: the provider generated the response server-side
        // before replying. Recorded so a later replay is as slow as the real call.
        let delay_ms = u64::try_from(request_start.elapsed().as_millis()).unwrap_or(0);
        StoredResponse {
            status,
            content_type,
            body: Some(String::from_utf8_lossy(&bytes).into_owned()),
            delay_ms,
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

    // Serve the just-captured response. The upstream call already waited the real
    // latency live, so replay a non-streaming body immediately rather than
    // sleeping delay_ms again (which would double it); streaming re-paces its
    // frames as before. The recorded delay applies on later replay.
    let serve_speed = if response.frames.is_empty() { 0.0 } else { 1.0 };
    response.into_response(serve_speed).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Message;

    fn req(model: &str, user: &str, stream: bool) -> NeutralRequest {
        NeutralRequest {
            model: model.to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: user.to_string(),
            }],
            stream,
            include_usage: false,
        }
    }

    fn cassette(endpoint: Endpoint, stream: bool, model: &str, user: &str) -> Cassette {
        Cassette {
            endpoint,
            stream,
            match_: Match {
                model: Some(model.to_string()),
                user_contains: Some(user.to_string()),
            },
            response: StoredResponse {
                status: 200,
                content_type: "application/json".to_string(),
                body: Some(r#"{"ok":true}"#.to_string()),
                delay_ms: 0,
                frames: Vec::new(),
            },
        }
    }

    fn record_cfg(
        upstream: Option<&str>,
        openai: Option<&str>,
        anthropic: Option<&str>,
        gemini: Option<&str>,
    ) -> RecordConfig {
        RecordConfig {
            dir: PathBuf::new(),
            upstream: upstream.map(str::to_string),
            upstream_openai: openai.map(str::to_string),
            upstream_anthropic: anthropic.map(str::to_string),
            upstream_gemini: gemini.map(str::to_string),
        }
    }

    #[test]
    fn base_for_falls_back_to_provider_default() {
        let rec = record_cfg(None, None, None, None);
        assert_eq!(rec.base_for(Endpoint::OpenAiChat), "https://api.openai.com");
        assert_eq!(
            rec.base_for(Endpoint::Anthropic),
            "https://api.anthropic.com"
        );
        assert_eq!(
            rec.base_for(Endpoint::Gemini),
            "https://generativelanguage.googleapis.com"
        );
    }

    #[test]
    fn base_for_per_provider_needs_no_global() {
        // A per-provider override applies even with no global --upstream; the
        // others fall through to their real defaults.
        let rec = record_cfg(None, Some("https://only-openai.example"), None, None);
        assert_eq!(
            rec.base_for(Endpoint::OpenAiChat),
            "https://only-openai.example"
        );
        assert_eq!(
            rec.base_for(Endpoint::Anthropic),
            "https://api.anthropic.com"
        );
    }

    #[test]
    fn base_for_global_upstream_applies_to_all() {
        let rec = record_cfg(Some("https://gw.example"), None, None, None);
        assert_eq!(rec.base_for(Endpoint::OpenAiChat), "https://gw.example");
        assert_eq!(rec.base_for(Endpoint::Anthropic), "https://gw.example");
        assert_eq!(rec.base_for(Endpoint::Gemini), "https://gw.example");
    }

    #[test]
    fn base_for_per_provider_overrides_global_and_default() {
        // OpenAI relocated to Azure, Anthropic to its own gateway, Gemini left
        // on the global fallback. Both OpenAI endpoints follow the OpenAI override.
        let rec = record_cfg(
            Some("https://fallback.example"),
            Some("https://acme.openai.azure.com/openai"),
            Some("https://anthropic.gw.example"),
            None,
        );
        assert_eq!(
            rec.base_for(Endpoint::OpenAiChat),
            "https://acme.openai.azure.com/openai"
        );
        assert_eq!(
            rec.base_for(Endpoint::OpenAiResponses),
            "https://acme.openai.azure.com/openai"
        );
        assert_eq!(
            rec.base_for(Endpoint::Anthropic),
            "https://anthropic.gw.example"
        );
        assert_eq!(rec.base_for(Endpoint::Gemini), "https://fallback.example");
    }

    #[test]
    fn redact_query_masks_gemini_key() {
        assert_eq!(redact_query("key=AIzaSECRET"), "key=REDACTED");
    }

    #[test]
    fn redact_query_keeps_order_and_non_secret_params() {
        assert_eq!(
            redact_query("alt=sse&key=SECRET&foo=bar"),
            "alt=sse&key=REDACTED&foo=bar"
        );
    }

    #[test]
    fn redact_query_is_case_insensitive() {
        assert_eq!(
            redact_query("API_KEY=x&Access_Token=y"),
            "API_KEY=REDACTED&Access_Token=REDACTED"
        );
    }

    #[test]
    fn redact_query_passes_through_when_no_secret() {
        assert_eq!(redact_query(""), "");
        assert_eq!(redact_query("foo=bar"), "foo=bar");
    }

    #[cfg(unix)]
    #[test]
    fn save_writes_owner_only_perms() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = save(
            dir.path(),
            &cassette(Endpoint::OpenAiChat, false, "gpt-4o", "weather"),
        )
        .expect("save");
        let file_mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(file_mode, 0o600);
        let dir_mode = std::fs::metadata(dir.path()).unwrap().permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700);
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let original = cassette(Endpoint::OpenAiChat, false, "gpt-4o", "weather");

        let path = save(dir.path(), &original).expect("save");
        assert_eq!(
            path.extension().and_then(std::ffi::OsStr::to_str),
            Some("json")
        );

        let store = Cassettes::load(dir.path()).expect("load");
        assert_eq!(store.len(), 1);

        // It replays for a request that matches its derived `Match`.
        let stored = store
            .find(
                Endpoint::OpenAiChat,
                &req("gpt-4o", "the weather today", false),
            )
            .expect("cassette should match");
        assert_eq!(stored.status, 200);
        assert_eq!(stored.body.as_deref(), Some(r#"{"ok":true}"#));
    }

    #[test]
    fn non_stream_delay_serdes_and_defaults_to_zero() {
        // delay_ms survives a round-trip.
        let mut c = cassette(Endpoint::OpenAiChat, false, "gpt-4o", "weather");
        c.response.delay_ms = 1234;
        let json = serde_json::to_string(&c).unwrap();
        assert!(json.contains("\"delay_ms\":1234"));
        let back: Cassette = serde_json::from_str(&json).unwrap();
        assert_eq!(back.response.delay_ms, 1234);

        // A cassette recorded before this field loads as 0 (instant).
        let legacy = r#"{"endpoint":"openai.chat","stream":false,
            "match":{"model":"gpt-4o"},
            "response":{"status":200,"content_type":"application/json","body":"{}"}}"#;
        let legacy: Cassette = serde_json::from_str(legacy).unwrap();
        assert_eq!(legacy.response.delay_ms, 0);
    }

    #[tokio::test]
    async fn non_stream_replay_waits_then_speed_zero_skips() {
        let mut c = cassette(Endpoint::OpenAiChat, false, "gpt-4o", "weather");
        c.response.delay_ms = 40;

        // At real speed it waits roughly the recorded latency (lower bound only,
        // to stay robust under scheduling jitter).
        let start = Instant::now();
        let resp = c.response.clone().into_response(1.0).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(start.elapsed() >= Duration::from_millis(30));

        // replay-speed 0 replays instantly despite the recorded latency.
        let start = Instant::now();
        let _ = c.response.clone().into_response(0.0).await;
        assert!(start.elapsed() < Duration::from_millis(30));
    }

    #[test]
    fn load_missing_dir_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("does-not-exist");
        let store = Cassettes::load(&missing).expect("missing dir loads empty");
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn match_is_scoped_by_endpoint_stream_model_and_message() {
        let dir = tempfile::tempdir().unwrap();
        save(
            dir.path(),
            &cassette(Endpoint::OpenAiChat, false, "gpt-4o", "weather"),
        )
        .unwrap();
        let store = Cassettes::load(dir.path()).expect("load");

        // Exact endpoint + stream + model, message contains the needle: hit.
        assert!(
            store
                .find(Endpoint::OpenAiChat, &req("gpt-4o", "weather now", false))
                .is_some()
        );

        // Wrong endpoint: miss.
        assert!(
            store
                .find(Endpoint::Anthropic, &req("gpt-4o", "weather", false))
                .is_none()
        );

        // Wrong streaming mode: miss.
        assert!(
            store
                .find(Endpoint::OpenAiChat, &req("gpt-4o", "weather", true))
                .is_none()
        );

        // Wrong model: miss.
        assert!(
            store
                .find(Endpoint::OpenAiChat, &req("gpt-4o-mini", "weather", false))
                .is_none()
        );

        // Message lacks the needle: miss.
        assert!(
            store
                .find(Endpoint::OpenAiChat, &req("gpt-4o", "forecast", false))
                .is_none()
        );
    }

    #[test]
    fn load_orders_most_specific_first() {
        let dir = tempfile::tempdir().unwrap();
        // Shorter and longer `user_contains` both match "the weather forecast",
        // but the longer (more specific) one must be tried first.
        let mut short = cassette(Endpoint::OpenAiChat, false, "gpt-4o", "weather");
        short.response.body = Some(r#"{"which":"short"}"#.to_string());
        let mut long = cassette(Endpoint::OpenAiChat, false, "gpt-4o", "weather forecast");
        long.response.body = Some(r#"{"which":"long"}"#.to_string());
        save(dir.path(), &short).unwrap();
        save(dir.path(), &long).unwrap();

        let store = Cassettes::load(dir.path()).expect("load");
        assert_eq!(store.len(), 2);
        let stored = store
            .find(
                Endpoint::OpenAiChat,
                &req("gpt-4o", "the weather forecast", false),
            )
            .expect("a cassette should match");
        assert_eq!(stored.body.as_deref(), Some(r#"{"which":"long"}"#));
    }
}
