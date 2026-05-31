//! The fixture engine: match an incoming [`NeutralRequest`] against a list of
//! rules and produce a [`NeutralResponse`].
//!
//! Rules are evaluated top to bottom; the first whose `match` block is
//! satisfied wins. A rule with an empty `match` matches anything, so put your
//! fallback last.

use std::path::Path;

use serde::Deserialize;

use crate::core::{NeutralRequest, NeutralResponse, StopReason, Usage};

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
    /// Load fixtures from a YAML file.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("reading {}: {e}", path.display()))?;
        let fixtures: Fixtures = serde_yaml::from_str(&text)
            .map_err(|e| format!("parsing {}: {e}", path.display()))?;
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
                },
            }],
        }
    }

    /// Find the first matching rule and build a neutral response from it.
    /// Returns `None` if nothing matched (no fallback rule present).
    pub fn respond_to(&self, req: &NeutralRequest) -> Option<NeutralResponse> {
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

        Some(NeutralResponse {
            model: req.model.clone(),
            content: rule.respond.content.clone(),
            stop_reason: rule.respond.finish_reason.into(),
            usage,
        })
    }
}

fn word_count(s: &str) -> u32 {
    s.split_whitespace().count() as u32
}
