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
    ToolCall, Usage,
};
use crate::util;

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Fixtures {
    #[serde(default)]
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Rule {
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
pub(crate) struct Match {
    /// Exact model name.
    pub model: Option<String>,
    /// Substring that must appear in the last user message.
    pub user_contains: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Respond {
    #[serde(default)]
    pub content: String,
    /// Tool/function calls to return. When present, `finish_reason` defaults to
    /// `tool_calls` and `content` defaults to empty (null on the wire).
    #[serde(default)]
    pub tool_calls: Vec<FixtureToolCall>,
    #[serde(default)]
    pub finish_reason: Option<FinishReason>,
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

/// A tool/function call to return. `arguments` may be given as a JSON string or
/// as a YAML mapping (which is serialized to a compact JSON string). `id` is
/// generated if omitted.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct FixtureToolCall {
    pub name: String,
    #[serde(default)]
    pub arguments: Option<Args>,
    #[serde(default)]
    pub id: Option<String>,
}

/// `arguments` accepts either a ready-made JSON string or a structured mapping.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(crate) enum Args {
    /// Used verbatim as the arguments JSON string.
    Str(String),
    /// Any structured YAML value, serialized to a compact JSON string.
    Value(serde_yaml::Value),
}

impl Args {
    fn to_json_string(&self) -> String {
        match self {
            Args::Str(s) => s.clone(),
            Args::Value(v) => serde_json::to_string(v).unwrap_or_else(|_| "{}".to_string()),
        }
    }
}

impl FixtureToolCall {
    fn resolve(&self) -> ToolCall {
        ToolCall {
            id: self.id.clone().unwrap_or_else(util::tool_call_id),
            name: self.name.clone(),
            arguments: self
                .arguments
                .as_ref()
                .map_or_else(|| "{}".to_string(), Args::to_json_string),
        }
    }
}

/// An injected HTTP error. `status` and `message` are required; `type` defaults
/// to a generic `api_error`.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct FixtureError {
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
pub(crate) struct FixtureFault {
    pub kind: FaultKind,
    #[serde(default)]
    pub after_tokens: Option<usize>,
    /// For `hang`: how long to stall before giving up (default 60_000 ms).
    #[serde(default)]
    pub hold_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FaultKind {
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
pub(crate) struct FixtureStream {
    pub ttft_ms: Option<u64>,
    pub inter_token_ms: Option<u64>,
    pub chunk_by: Option<ChunkByConfig>,
}

/// `chunk_by` in YAML may be a string (`word`/`char`) or a number (chars/chunk).
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(crate) enum ChunkByConfig {
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
pub(crate) enum FinishReason {
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
pub(crate) struct FixtureUsage {
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
    pub(crate) fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("reading {}: {e}", path.display()))?;
        let fixtures: Fixtures =
            serde_yaml::from_str(&text).map_err(|e| format!("parsing {}: {e}", path.display()))?;
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
            if let Some(respond) = &rule.respond {
                // A respond rule must produce something.
                if respond.content.is_empty() && respond.tool_calls.is_empty() {
                    return Err(format!(
                        "rule {i}: `respond` needs `content` or `tool_calls`"
                    ));
                }
                // Validate chunk_by up front so a bad value fails at startup.
                if let Some(cb) = respond.stream.as_ref().and_then(|s| s.chunk_by.as_ref()) {
                    cb.resolve().map_err(|e| format!("rule {i}: {e}"))?;
                }
            }
        }
        Ok(fixtures)
    }

    /// A sensible built-in fixture set so the server is useful with no config.
    pub(crate) fn builtin_default() -> Self {
        Fixtures {
            rules: vec![Rule {
                match_: Match::default(),
                respond: Some(Respond {
                    content: "This is a mock response from llmock.".to_string(),
                    tool_calls: Vec::new(),
                    finish_reason: None,
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
    pub(crate) fn outcome_for(
        &self,
        req: &NeutralRequest,
        defaults: StreamSpec,
    ) -> Option<Outcome> {
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

        let tool_calls: Vec<ToolCall> = respond
            .tool_calls
            .iter()
            .map(FixtureToolCall::resolve)
            .collect();

        // Estimate token counts when the fixture doesn't pin them, so usage
        // looks plausible. A crude word count stands in for tokenization.
        let usage = if let Some(u) = respond.usage {
            Usage {
                prompt_tokens: u.prompt_tokens,
                completion_tokens: u.completion_tokens,
            }
        } else {
            let completion: u32 = word_count(&respond.content)
                + tool_calls
                    .iter()
                    .map(|t| word_count(&t.arguments) + 1)
                    .sum::<u32>();
            Usage {
                prompt_tokens: req.messages.iter().map(|m| word_count(&m.content)).sum(),
                completion_tokens: completion,
            }
        };

        // finish_reason defaults to `tool_calls` when tool calls are present,
        // otherwise `stop` — unless the fixture pins it explicitly.
        let stop_reason: StopReason = match respond.finish_reason {
            Some(fr) => fr.into(),
            None if !tool_calls.is_empty() => StopReason::ToolCalls,
            None => StopReason::Stop,
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
            tool_calls,
            stop_reason,
            usage,
            stream: spec,
            fault: respond.fault.as_ref().map(FixtureFault::resolve),
        }))
    }
}

fn word_count(s: &str) -> u32 {
    s.split_whitespace().count() as u32
}
