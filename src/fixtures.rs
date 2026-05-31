//! The fixture engine: match an incoming [`NeutralRequest`] against a list of
//! rules and produce a [`NeutralResponse`].
//!
//! Rules are evaluated top to bottom; the first whose `match` block is
//! satisfied wins. A rule with an empty `match` matches anything, so put your
//! fallback last.

use std::path::Path;

use serde::Deserialize;

use crate::core::{ChunkBy, NeutralRequest, NeutralResponse, StopReason, StreamSpec, Usage};

#[derive(Debug, Clone, Deserialize)]
pub struct Fixtures {
    #[serde(default)]
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Rule {
    #[serde(default, rename = "match")]
    pub match_: Match,
    pub respond: Respond,
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
            if let Some(cb) = rule.respond.stream.as_ref().and_then(|s| s.chunk_by.as_ref()) {
                cb.resolve()
                    .map_err(|e| format!("rule {i}: {e}"))?;
            }
        }
        Ok(fixtures)
    }

    /// A sensible built-in fixture set so the server is useful with no config.
    pub fn builtin_default() -> Self {
        Fixtures {
            rules: vec![Rule {
                match_: Match::default(),
                respond: Respond {
                    content: "This is a mock response from llmock.".to_string(),
                    finish_reason: FinishReason::Stop,
                    usage: None,
                    stream: None,
                },
            }],
        }
    }

    /// Find the first matching rule and build a neutral response from it.
    /// `defaults` supplies streaming timing/granularity for any field a rule
    /// does not override. Returns `None` if nothing matched.
    pub fn respond_to(
        &self,
        req: &NeutralRequest,
        defaults: StreamSpec,
    ) -> Option<NeutralResponse> {
        let rule = self.rules.iter().find(|r| r.match_.matches(req))?;

        // Estimate token counts when the fixture doesn't pin them, so usage
        // looks plausible. A crude word count stands in for tokenization.
        let usage = match rule.respond.usage {
            Some(u) => Usage {
                prompt_tokens: u.prompt_tokens,
                completion_tokens: u.completion_tokens,
            },
            None => Usage {
                prompt_tokens: req
                    .messages
                    .iter()
                    .map(|m| word_count(&m.content))
                    .sum(),
                completion_tokens: word_count(&rule.respond.content),
            },
        };

        // Resolve streaming spec: start from defaults, apply per-rule overrides.
        let mut spec = defaults;
        if let Some(fs) = &rule.respond.stream {
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

        Some(NeutralResponse {
            model: req.model.clone(),
            content: rule.respond.content.clone(),
            stop_reason: rule.respond.finish_reason.into(),
            usage,
            stream: spec,
        })
    }
}

fn word_count(s: &str) -> u32 {
    s.split_whitespace().count() as u32
}
