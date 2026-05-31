//! The fixture engine: match an incoming [`NeutralRequest`] against a list of
//! rules and produce a [`NeutralResponse`].
//!
//! Rules are evaluated top to bottom; the first whose `match` block is
//! satisfied wins. A rule with an empty `match` matches anything, so put your
//! fallback last.

use std::path::Path;

use serde::Deserialize;

use crate::core::{
    ChunkBy, Fault, InjectError, NeutralRequest, NeutralResponse, Outcome, StopReason, StreamSpec,
    Usage,
};

#[derive(Debug, Clone, Deserialize)]
pub struct Fixtures {
    #[serde(default)]
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Rule {
    #[serde(default, rename = "match")]
    pub match_: Match,
    /// Serve a (possibly streaming) response. Required unless `error` is set.
    #[serde(default)]
    pub respond: Option<Respond>,
    /// Fail the request with an injected HTTP error instead of responding.
    #[serde(default)]
    pub error: Option<FixtureError>,
}

/// Conditions to test against a request. All present conditions must hold
/// (logical AND). Absent conditions are ignored.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Match {
    /// Exact model name.
    pub model: Option<String>,
    /// Substring that must appear in the last user message.
    pub user_contains: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Respond {
    pub content: String,
    #[serde(default)]
    pub finish_reason: FinishReason,
    #[serde(default)]
    pub usage: Option<FixtureUsage>,
    /// Per-rule streaming overrides. Any field left out falls back to the
    /// server's global stream defaults.
    #[serde(default)]
    pub stream: Option<FixtureStream>,
    /// A mid-stream fault to inject (truncate / malformed / hang). Only applies
    /// when the request asked for streaming.
    #[serde(default)]
    pub fault: Option<FixtureFault>,
}

/// An injected HTTP error. `status` and `message` are required; `type` defaults
/// to a generic `api_error`.
#[derive(Debug, Clone, Deserialize)]
pub struct FixtureError {
    pub status: u16,
    #[serde(default = "default_error_type", rename = "type")]
    pub error_type: String,
    pub message: String,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub param: Option<String>,
}

fn default_error_type() -> String {
    "api_error".to_string()
}

/// A mid-stream fault. `after_tokens` is how many content deltas to emit before
/// the fault triggers (default 1).
#[derive(Debug, Clone, Deserialize)]
pub struct FixtureFault {
    pub kind: FaultKind,
    #[serde(default)]
    pub after_tokens: Option<usize>,
    /// For `hang`: how long to stall before giving up (default 60_000 ms).
    #[serde(default)]
    pub hold_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FaultKind {
    Truncate,
    Malformed,
    Hang,
}

impl FixtureFault {
    fn resolve(&self) -> Fault {
        let after = self.after_tokens.unwrap_or(1);
        match self.kind {
            FaultKind::Truncate => Fault::Truncate { after },
            FaultKind::Malformed => Fault::Malformed { after },
            FaultKind::Hang => Fault::Hang {
                after,
                hold_ms: self.hold_ms.unwrap_or(60_000),
            },
        }
    }
}

impl FixtureError {
    fn resolve(&self) -> InjectError {
        InjectError {
            status: self.status,
            error_type: self.error_type.clone(),
            message: self.message.clone(),
            code: self.code.clone(),
            param: self.param.clone(),
        }
    }
}

/// Per-rule streaming overrides. Each field is optional; absent fields inherit
/// the global defaults.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct FixtureStream {
    pub ttft_ms: Option<u64>,
    pub inter_token_ms: Option<u64>,
    pub chunk_by: Option<ChunkByConfig>,
}

/// `chunk_by` in YAML may be a string (`word`/`char`) or a number (chars/chunk).
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ChunkByConfig {
    Named(String),
    Size(usize),
}

impl ChunkByConfig {
    fn resolve(&self) -> Result<ChunkBy, String> {
        match self {
            ChunkByConfig::Named(s) => ChunkBy::parse(s),
            ChunkByConfig::Size(n) => ChunkBy::parse(&n.to_string()),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    #[default]
    Stop,
    Length,
    ToolCalls,
    ContentFilter,
}

impl From<FinishReason> for StopReason {
    fn from(fr: FinishReason) -> Self {
        match fr {
            FinishReason::Stop => StopReason::Stop,
            FinishReason::Length => StopReason::Length,
            FinishReason::ToolCalls => StopReason::ToolCalls,
            FinishReason::ContentFilter => StopReason::ContentFilter,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct FixtureUsage {
    #[serde(default)]
    pub prompt_tokens: u32,
    #[serde(default)]
    pub completion_tokens: u32,
}

impl Match {
    fn matches(&self, req: &NeutralRequest) -> bool {
        if let Some(model) = &self.model {
            if &req.model != model {
                return false;
            }
        }
        if let Some(needle) = &self.user_contains {
            match req.last_user_message() {
                Some(msg) if msg.contains(needle.as_str()) => {}
                _ => return false,
            }
        }
        true
    }
}

impl Fixtures {
    /// Load fixtures from a YAML file, validating `chunk_by` values up front so
    /// a bad config fails at startup rather than mid-request.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("reading {}: {e}", path.display()))?;
        let fixtures: Fixtures = serde_yaml::from_str(&text)
            .map_err(|e| format!("parsing {}: {e}", path.display()))?;
        for (i, rule) in fixtures.rules.iter().enumerate() {
            // Each rule must do exactly one thing.
            match (&rule.respond, &rule.error) {
                (None, None) => {
                    return Err(format!("rule {i}: needs a `respond` or an `error` block"))
                }
                (Some(_), Some(_)) => {
                    return Err(format!(
                        "rule {i}: has both `respond` and `error`; use only one"
                    ))
                }
                _ => {}
            }
            // Validate chunk_by up front so a bad value fails at startup.
            if let Some(cb) = rule
                .respond
                .as_ref()
                .and_then(|r| r.stream.as_ref())
                .and_then(|s| s.chunk_by.as_ref())
            {
                cb.resolve().map_err(|e| format!("rule {i}: {e}"))?;
            }
        }
        Ok(fixtures)
    }

    /// A sensible built-in fixture set so the server is useful with no config.
    pub fn builtin_default() -> Self {
        Fixtures {
            rules: vec![Rule {
                match_: Match::default(),
                respond: Some(Respond {
                    content: "This is a mock response from llmock.".to_string(),
                    finish_reason: FinishReason::Stop,
                    usage: None,
                    stream: None,
                    fault: None,
                }),
                error: None,
            }],
        }
    }

    /// Find the first matching rule and turn it into an [`Outcome`].
    /// `defaults` supplies streaming timing/granularity for any field a rule
    /// does not override. Returns `None` if nothing matched.
    pub fn outcome_for(&self, req: &NeutralRequest, defaults: StreamSpec) -> Option<Outcome> {
        let rule = self.rules.iter().find(|r| r.match_.matches(req))?;

        // An `error` rule short-circuits to an injected HTTP error.
        if let Some(err) = &rule.error {
            return Some(Outcome::Error(err.resolve()));
        }

        // Otherwise it's a `respond` rule (load-time validation guarantees one).
        let respond = rule
            .respond
            .as_ref()
            .expect("rule validated to have respond or error");

        // Estimate token counts when the fixture doesn't pin them, so usage
        // looks plausible. A crude word count stands in for tokenization.
        let usage = match respond.usage {
            Some(u) => Usage {
                prompt_tokens: u.prompt_tokens,
                completion_tokens: u.completion_tokens,
            },
            None => Usage {
                prompt_tokens: req.messages.iter().map(|m| word_count(&m.content)).sum(),
                completion_tokens: word_count(&respond.content),
            },
        };

        // Resolve streaming spec: start from defaults, apply per-rule overrides.
        let mut spec = defaults;
        if let Some(fs) = &respond.stream {
            if let Some(t) = fs.ttft_ms {
                spec.ttft_ms = t;
            }
            if let Some(it) = fs.inter_token_ms {
                spec.inter_token_ms = it;
            }
            if let Some(cb) = &fs.chunk_by {
                // Validated at load; fall back to the default on the off chance.
                spec.chunk_by = cb.resolve().unwrap_or(spec.chunk_by);
            }
        }

        Some(Outcome::Respond(NeutralResponse {
            model: req.model.clone(),
            content: respond.content.clone(),
            stop_reason: respond.finish_reason.into(),
            usage,
            stream: spec,
            fault: respond.fault.as_ref().map(|f| f.resolve()),
        }))
    }
}

fn word_count(s: &str) -> u32 {
    s.split_whitespace().count() as u32
}
