//! Tests for the unified record/replay engine. Replay is exercised offline;
//! record is exercised against an in-process mock upstream (no real API keys).
//! Cassettes are matched by the same `Match` as fixtures (model + last user
//! message), scoped to endpoint + streaming mode.

use std::path::{Path, PathBuf};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::routing::any;
use http_body_util::BodyExt;
use tower::ServiceExt;

use crate::build_app;
use crate::cassette::{Cassette, Cassettes, RecordConfig};
use crate::core::StreamDefaults;
use crate::fixtures::Fixtures;
use crate::state::AppState;

const FIXTURES: &str = r#"
rules:
  - match: {}
    respond:
      content: "fallback fixture response"
"#;

fn app_with(dir: &Path, record: Option<RecordConfig>) -> Router {
    app_with_speed(dir, record, 1.0)
}

fn app_with_speed(dir: &Path, record: Option<RecordConfig>, speed: f64) -> Router {
    let store = Cassettes::load(dir).expect("load cassettes");
    let fixtures = Fixtures::from_yaml(FIXTURES).expect("valid fixtures");
    let state =
        AppState::new(fixtures, StreamDefaults::instant()).with_cassettes(store, record, speed);
    build_app(state)
}

async fn post(app: Router, uri: &str, body: &str) -> (StatusCode, String, String) {
    let req = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let ct = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, ct, String::from_utf8(bytes.to_vec()).unwrap())
}

async fn serve(app: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

async fn spawn_upstream(body: &'static str) -> String {
    let app = Router::new().fallback(any(move || async move {
        ([(header::CONTENT_TYPE, "application/json")], body)
    }));
    serve(app).await
}

async fn spawn_slow_upstream(body: &'static str, delay: Duration) -> String {
    let app = Router::new().fallback(any(move || async move {
        tokio::time::sleep(delay).await;
        ([(header::CONTENT_TYPE, "application/json")], body)
    }));
    serve(app).await
}

async fn spawn_sse_upstream(chunks: &'static [&'static str], gap: Duration) -> String {
    let app = Router::new().fallback(any(move || async move {
        let body = Body::from_stream(async_stream::stream! {
            for (i, c) in chunks.iter().enumerate() {
                if i > 0 { tokio::time::sleep(gap).await; }
                yield Ok::<_, std::convert::Infallible>(axum::body::Bytes::from(*c));
            }
        });
        axum::response::Response::builder()
            .header(header::CONTENT_TYPE, "text/event-stream")
            .body(body)
            .unwrap()
    }));
    serve(app).await
}

fn record_to(dir: &Path, upstream: &str) -> RecordConfig {
    RecordConfig {
        dir: dir.to_path_buf(),
        upstream: Some(upstream.to_string()),
    }
}

/// The single `.json` cassette written to `dir`.
fn find_cassette_file(dir: &Path) -> PathBuf {
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .find(|e| e.path().extension().and_then(|s| s.to_str()) == Some("json"))
        .unwrap()
        .path()
}

fn load_cassette(path: &Path) -> Cassette {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[tokio::test]
async fn replay_matches_like_a_fixture_and_misses_fall_through() {
    let dir = tempfile::tempdir().unwrap();
    let cassette = serde_json::json!({
        "endpoint": "openai.chat",
        "stream": false,
        "match": { "model": "gpt-4o", "user_contains": "weather" },
        "response": {
            "status": 200,
            "content_type": "application/json",
            "body": "{\"id\":\"chatcmpl-REAL\",\"object\":\"chat.completion\"}"
        }
    });
    std::fs::write(
        dir.path().join("a.json"),
        serde_json::to_string(&cassette).unwrap(),
    )
    .unwrap();

    // Same model + a message *containing* "weather" replays (not exact body).
    let (status, ct, body) = post(
        app_with(dir.path(), None),
        "/openai/v1/chat/completions",
        r#"{"model":"gpt-4o","messages":[{"role":"user","content":"what is the weather today?"}]}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ct, "application/json");
    assert_eq!(
        body,
        "{\"id\":\"chatcmpl-REAL\",\"object\":\"chat.completion\"}"
    );

    // Different model misses (endpoint+model scope), falls to the fixture.
    let (_, _, body) = post(
        app_with(dir.path(), None),
        "/openai/v1/chat/completions",
        r#"{"model":"gpt-4o-mini","messages":[{"role":"user","content":"weather?"}]}"#,
    )
    .await;
    assert!(body.contains("fallback fixture response"), "{body}");

    // A streaming request does not replay a non-streaming cassette.
    let (_, ct, _) = post(
        app_with(dir.path(), None),
        "/openai/v1/chat/completions",
        r#"{"model":"gpt-4o","stream":true,"messages":[{"role":"user","content":"weather?"}]}"#,
    )
    .await;
    assert!(
        ct.contains("text/event-stream"),
        "should fall to the streaming fixture, got {ct}"
    );
}

#[tokio::test]
async fn record_proxies_saves_with_derived_match_then_replays() {
    let dir = tempfile::tempdir().unwrap();
    let upstream = spawn_upstream("{\"recorded\":true}").await;

    let (status, _, body) = post(
        app_with(dir.path(), Some(record_to(dir.path(), &upstream))),
        "/openai/v1/chat/completions",
        r#"{"model":"gpt-4o","messages":[{"role":"user","content":"record me"}]}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "{\"recorded\":true}");

    // One cassette saved, with a match derived from the request.
    let cassette = load_cassette(&find_cassette_file(dir.path()));
    assert_eq!(cassette.match_.model.as_deref(), Some("gpt-4o"));
    assert_eq!(cassette.match_.user_contains.as_deref(), Some("record me"));

    // Replay-only: a request that *contains* the recorded message replays.
    let (_, _, body) = post(
        app_with(dir.path(), None),
        "/openai/v1/chat/completions",
        r#"{"model":"gpt-4o","messages":[{"role":"user","content":"please record me now"}]}"#,
    )
    .await;
    assert_eq!(body, "{\"recorded\":true}");
}

#[tokio::test]
async fn record_streaming_captures_timed_frames_and_replays() {
    let dir = tempfile::tempdir().unwrap();
    let chunks: &[&str] = &[
        "data: {\"delta\":\"Hello \"}\n\n",
        "data: {\"delta\":\"world\"}\n\n",
        "data: [DONE]\n\n",
    ];
    let upstream = spawn_sse_upstream(chunks, Duration::from_millis(40)).await;

    let (status, ct, body) = post(
        app_with(dir.path(), Some(record_to(dir.path(), &upstream))),
        "/openai/v1/chat/completions",
        r#"{"model":"gpt-4o","stream":true,"messages":[{"role":"user","content":"hi"}]}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(ct.contains("text/event-stream"));
    assert_eq!(body, chunks.concat());

    let cassette = load_cassette(&find_cassette_file(dir.path()));
    assert!(
        cassette.stream,
        "recorded cassette should be marked streaming"
    );
    assert!(cassette.response.body.is_none());
    assert!(cassette.response.frames.len() >= 2);
    assert!(
        cassette
            .response
            .frames
            .iter()
            .skip(1)
            .any(|f| f.delay_ms > 0)
    );

    // Replay at real speed (1.0) re-applies the recorded timing.
    let body_req =
        r#"{"model":"gpt-4o","stream":true,"messages":[{"role":"user","content":"hi"}]}"#;
    let start = std::time::Instant::now();
    let (_, _, body) = post(
        app_with(dir.path(), None),
        "/openai/v1/chat/completions",
        body_req,
    )
    .await;
    assert_eq!(body, chunks.concat());
    assert!(start.elapsed() >= Duration::from_millis(40));

    // replay-speed 0 replays instantly: same bytes, no waiting.
    let start = std::time::Instant::now();
    let (_, _, body) = post(
        app_with_speed(dir.path(), None, 0.0),
        "/openai/v1/chat/completions",
        body_req,
    )
    .await;
    assert_eq!(body, chunks.concat());
    assert!(
        start.elapsed() < Duration::from_millis(30),
        "speed 0 should skip the recorded delays"
    );
}

#[tokio::test]
async fn record_non_stream_captures_latency_without_doubling_it() {
    let dir = tempfile::tempdir().unwrap();
    let upstream = spawn_slow_upstream("{\"recorded\":true}", Duration::from_millis(200)).await;

    let start = std::time::Instant::now();
    let (status, _ct, body) = post(
        app_with(dir.path(), Some(record_to(dir.path(), &upstream))),
        "/openai/v1/chat/completions",
        r#"{"model":"gpt-4o","messages":[{"role":"user","content":"slow one"}]}"#,
    )
    .await;
    let elapsed = start.elapsed();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "{\"recorded\":true}");

    // The captured latency reflects the real upstream wait.
    let cassette = load_cassette(&find_cassette_file(dir.path()));
    assert!(
        cassette.response.delay_ms >= 150,
        "delay_ms = {}",
        cassette.response.delay_ms
    );

    // Record already waited the real latency live; serving the body must not sleep
    // delay_ms again. One ~200ms wait, not two.
    assert!(
        elapsed < Duration::from_millis(380),
        "record served in {elapsed:?}; non-streaming latency was applied twice"
    );
}
