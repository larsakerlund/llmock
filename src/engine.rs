//! The single resolution path shared by every adapter. A parsed request is
//! matched against cassettes and fixtures by the **same rules**, then either a
//! recorded response is replayed, a fixture is synthesized, an error is
//! injected, or (in record mode) the miss is proxied to the real upstream.

use axum::body::Bytes;
use axum::http::{HeaderMap, Method};
use axum::response::Response;

use crate::cassette::{Endpoint, record};
use crate::core::{InjectError, NeutralRequest, NeutralResponse, Outcome};
use crate::state::AppState;

/// What to do with a request, once matched.
pub(crate) enum Resolution {
    /// Replay recorded bytes (or just-recorded ones) verbatim.
    Raw(Response),
    /// Synthesize a fixture response; the adapter serializes it to its wire form.
    Synthesize(Box<NeutralResponse>),
    /// Render an injected error in the adapter's envelope.
    Error(InjectError),
    /// Nothing matched and we are not recording.
    NoMatch,
}

/// Resolve a request: cassette replay → record (if on) → fixture → no match.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn resolve(
    state: &AppState,
    endpoint: Endpoint,
    req: &NeutralRequest,
    method: &Method,
    path: &str,
    query: &str,
    raw_body: &Bytes,
    headers: &HeaderMap,
) -> Resolution {
    // 1. A matching cassette replays — same Match as fixtures, endpoint-scoped.
    if let Some(stored) = state.cassettes.find(endpoint, req) {
        return Resolution::Raw(stored.clone().into_response(state.replay_speed));
    }
    // 2. Record mode proxies the miss to the real upstream and captures it.
    if let Some(rec) = &state.record {
        return Resolution::Raw(
            record(
                &state.client,
                rec,
                endpoint,
                req,
                path,
                query,
                method,
                raw_body,
                headers,
            )
            .await,
        );
    }
    // 3. Otherwise the fixture engine decides.
    match state.fixtures.outcome_for(req, &state.stream_defaults) {
        Some(Outcome::Respond(r)) => Resolution::Synthesize(Box::new(r)),
        Some(Outcome::Error(e)) => Resolution::Error(e),
        None => Resolution::NoMatch,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{Message, StreamDefaults};
    use crate::fixtures::Fixtures;

    fn req(model: &str, user: &str) -> NeutralRequest {
        NeutralRequest {
            model: model.to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: user.to_string(),
            }],
            stream: false,
            include_usage: false,
        }
    }

    /// Resolve against a fixture-only state (no cassettes, no record), so no
    /// network is involved. Exercises step 3 of the resolution path.
    async fn resolve_fixture_only(fixtures: Fixtures, req: &NeutralRequest) -> Resolution {
        let state = AppState::new(fixtures, StreamDefaults::instant());
        resolve(
            &state,
            Endpoint::OpenAiChat,
            req,
            &Method::POST,
            "/v1/chat/completions",
            "",
            &Bytes::new(),
            &HeaderMap::new(),
        )
        .await
    }

    #[tokio::test]
    async fn fixture_respond_synthesizes() {
        let fixtures = Fixtures::from_yaml(
            r#"
rules:
  - match: {}
    respond:
      content: "hi from fixture"
"#,
        )
        .expect("valid fixtures");
        match resolve_fixture_only(fixtures, &req("gpt-4o", "anything")).await {
            Resolution::Synthesize(r) => assert_eq!(r.content, "hi from fixture"),
            _ => panic!("expected Synthesize"),
        }
    }

    #[tokio::test]
    async fn fixture_error_resolves_to_error() {
        let fixtures = Fixtures::from_yaml(
            r#"
rules:
  - match: {}
    error:
      status: 503
      message: "down"
"#,
        )
        .expect("valid fixtures");
        match resolve_fixture_only(fixtures, &req("gpt-4o", "x")).await {
            Resolution::Error(e) => assert_eq!(e.status, 503),
            _ => panic!("expected Error"),
        }
    }

    #[tokio::test]
    async fn no_matching_rule_is_no_match() {
        let fixtures = Fixtures::from_yaml(
            r#"
rules:
  - match: { user_contains: "weather" }
    respond:
      content: "matched"
"#,
        )
        .expect("valid fixtures");
        match resolve_fixture_only(fixtures, &req("gpt-4o", "unrelated")).await {
            Resolution::NoMatch => {}
            _ => panic!("expected NoMatch"),
        }
    }
}
