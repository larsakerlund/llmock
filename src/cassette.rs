//! Record/replay cassettes (à la VCR/Polly).
//!
//! A cassette captures one real provider exchange — the request signature and
//! the exact response bytes — so llmock can replay the genuine server's output
//! byte-for-byte. This is the strongest fidelity guarantee: replay what the real
//! API actually sent, not what we think it sends.
//!
//! Two modes, both as a middleware wrapping the whole router:
//! - **Replay**: if a loaded cassette matches the incoming request, return its
//!   stored response and never touch the adapters.
//! - **Record**: on a miss (and only when recording), proxy the request to the
//!   real upstream, save the exchange as a cassette, and return the real bytes.
//!
//! Matching is provider-agnostic: method + path + query + the request JSON body
//! (compared structurally, so key order and whitespace don't matter).

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One recorded request/response exchange.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Cassette {
    pub request: RequestSig,
    pub response: StoredResponse,
}

/// The request fields a cassette matches on.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RequestSig {
    pub method: String,
    pub path: String,
    #[serde(default)]
    pub query: String,
    pub body: Value,
}

/// The captured response, replayed verbatim.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct StoredResponse {
    pub status: u16,
    pub content_type: String,
    pub body: String,
}

impl StoredResponse {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        (
            status,
            [(header::CONTENT_TYPE, self.content_type)],
            self.body,
        )
            .into_response()
    }
}

/// A directory of loaded cassettes.
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
        Ok(Cassettes { entries })
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    fn find(&self, method: &str, path: &str, query: &str, body: &Value) -> Option<&StoredResponse> {
        self.entries
            .iter()
            .find(|c| {
                c.request.method == method
                    && c.request.path == path
                    && c.request.query == query
                    && &c.request.body == body
            })
            .map(|c| &c.response)
    }
}

/// Persist a cassette to `dir/<hash>.json`.
fn save(dir: &Path, cassette: &Cassette) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    let mut hasher = DefaultHasher::new();
    cassette.request.method.hash(&mut hasher);
    cassette.request.path.hash(&mut hasher);
    cassette.request.query.hash(&mut hasher);
    cassette.request.body.to_string().hash(&mut hasher);
    let name = format!("{:016x}.json", hasher.finish());
    let path = dir.join(name);
    let text = serde_json::to_string_pretty(cassette).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("writing {}: {e}", path.display()))?;
    Ok(path)
}

/// Default real upstream base URL for a request path.
fn default_upstream(path: &str) -> &'static str {
    if path.starts_with("/v1beta/") {
        "https://generativelanguage.googleapis.com"
    } else if path == "/v1/messages" {
        "https://api.anthropic.com"
    } else {
        "https://api.openai.com"
    }
}

/// Configuration for record mode.
#[derive(Debug, Clone)]
pub(crate) struct RecordConfig {
    /// Override the upstream base URL (used for testing against a mock server).
    pub upstream: Option<String>,
}

/// Middleware state: the loaded cassettes plus optional record config.
#[derive(Clone)]
pub(crate) struct CassetteLayer {
    pub store: Arc<Cassettes>,
    pub dir: PathBuf,
    pub record: Option<RecordConfig>,
    pub client: reqwest::Client,
}

/// Replay/record middleware wrapping the whole router.
pub(crate) async fn middleware(
    State(layer): State<CassetteLayer>,
    req: Request,
    next: Next,
) -> Response {
    let (parts, body) = req.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, usize::MAX).await else {
        return (StatusCode::BAD_REQUEST, "could not read request body").into_response();
    };
    let method = parts.method.as_str().to_string();
    let path = parts.uri.path().to_string();
    let query = parts.uri.query().unwrap_or("").to_string();
    let json: Option<Value> = serde_json::from_slice(&bytes).ok();

    // Replay: a matching cassette short-circuits everything.
    if let Some(body_json) = &json {
        if let Some(stored) = layer.store.find(&method, &path, &query, body_json) {
            return stored.clone().into_response();
        }
    }

    // Record: proxy to the real upstream, capture, and serve.
    if let (Some(rec), Some(body_json)) = (&layer.record, &json) {
        return record(
            &layer,
            rec,
            &parts.headers,
            &bytes,
            method,
            path,
            query,
            body_json,
        )
        .await;
    }

    // Otherwise fall through to the normal (fixture-based) adapters.
    next.run(Request::from_parts(parts, Body::from(bytes)))
        .await
}

/// Headers worth forwarding to the upstream (auth + content negotiation).
fn forwardable(headers: &HeaderMap) -> Vec<(reqwest::header::HeaderName, String)> {
    const KEEP: &[&str] = &[
        "authorization",
        "x-api-key",
        "anthropic-version",
        "anthropic-beta",
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
            let v = value.to_str().ok()?.to_string();
            Some((n, v))
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
async fn record(
    layer: &CassetteLayer,
    rec: &RecordConfig,
    headers: &HeaderMap,
    body: &[u8],
    method: String,
    path: String,
    query: String,
    body_json: &Value,
) -> Response {
    let base = rec
        .upstream
        .clone()
        .unwrap_or_else(|| default_upstream(&path).to_string());
    let url = if query.is_empty() {
        format!("{base}{path}")
    } else {
        format!("{base}{path}?{query}")
    };

    let Ok(reqwest_method) = reqwest::Method::from_bytes(method.as_bytes()) else {
        return (StatusCode::BAD_REQUEST, "bad method").into_response();
    };
    let mut builder = layer
        .client
        .request(reqwest_method, &url)
        .body(body.to_vec());
    for (name, value) in forwardable(headers) {
        builder = builder.header(name, value);
    }

    let upstream = match builder.send().await {
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
    let body_bytes = upstream.bytes().await.unwrap_or_default();
    let stored = StoredResponse {
        status,
        content_type,
        body: String::from_utf8_lossy(&body_bytes).into_owned(),
    };

    let cassette = Cassette {
        request: RequestSig {
            method,
            path,
            query,
            body: body_json.clone(),
        },
        response: stored.clone(),
    };
    match save(&layer.dir, &cassette) {
        Ok(path) => tracing::info!("recorded cassette {}", path.display()),
        Err(e) => tracing::error!("record: could not save cassette: {e}"),
    }

    stored.into_response()
}
