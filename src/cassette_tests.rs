//! Tests for the record/replay cassette layer. Replay is exercised offline;
//! record is exercised against an in-process mock upstream (no real API keys).

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum::routing::any;
use axum::Router;
use http_body_util::BodyExt;
use tower::ServiceExt;

use crate::build_app;
use crate::cassette::{Cassette, CassetteLayer, Cassettes, RecordConfig};
use crate::core::StreamSpec;
use crate::fixtures::Fixtures;
use crate::state::AppState;

const FIXTURES: &str = r#"
rules:
  - match: {}
    respond:
      content: "fallback fixture response"
"#;

fn app_with_cassettes(dir: &Path, record: Option<RecordConfig>) -> Router {
    let store = Cassettes::load(dir).expect("load cassettes");
    let fixtures = Fixtures::from_yaml(FIXTURES).expect("valid fixtures");
    let base = build_app(AppState::new(fixtures, StreamSpec::default()));
    let layer = CassetteLayer {
        store: Arc::new(store),
        dir: dir.to_path_buf(),
        record,
        client: reqwest::Client::new(),
    };
    base.layer(axum::middleware::from_fn_with_state(
        layer,
        crate::cassette::middleware,
    ))
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

/// Spawn a mock upstream that returns `body` for any request, return its base URL.
async fn spawn_upstream(body: &'static str) -> String {
    let app = Router::new().fallback(any(move || async move {
        ([(header::CONTENT_TYPE, "application/json")], body)
    }));
    serve(app).await
}

/// Spawn a mock upstream that streams SSE chunks with a delay between each.
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

async fn serve(app: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

#[tokio::test]
async fn replay_returns_stored_bytes_and_misses_fall_through() {
    let dir = tempfile::tempdir().unwrap();
    let cassette = serde_json::json!({
        "request": {
            "method": "POST",
            "path": "/v1/chat/completions",
            "query": "",
            "body": {"model": "gpt-4o", "messages": [{"role": "user", "content": "replay me"}]}
        },
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

    // Exact-match request is replayed verbatim.
    let app = app_with_cassettes(dir.path(), None);
    let (status, ct, body) = post(
        app,
        "/v1/chat/completions",
        r#"{"model":"gpt-4o","messages":[{"role":"user","content":"replay me"}]}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ct, "application/json");
    assert_eq!(
        body,
        "{\"id\":\"chatcmpl-REAL\",\"object\":\"chat.completion\"}"
    );

    // A different body misses the cassette and falls through to the fixture.
    let app = app_with_cassettes(dir.path(), None);
    let (_, _, body) = post(
        app,
        "/v1/chat/completions",
        r#"{"model":"gpt-4o","messages":[{"role":"user","content":"something else"}]}"#,
    )
    .await;
    assert!(
        body.contains("fallback fixture response"),
        "expected fixture fallthrough, got {body}"
    );
}

#[tokio::test]
async fn record_proxies_saves_and_then_replays() {
    let dir = tempfile::tempdir().unwrap();
    let upstream = spawn_upstream("{\"recorded\":true}").await;

    // Record: no cassette yet, so proxy to the mock upstream and save.
    let app = app_with_cassettes(
        dir.path(),
        Some(RecordConfig {
            upstream: Some(upstream.clone()),
        }),
    );
    let (status, _, body) = post(
        app,
        "/v1/chat/completions",
        r#"{"model":"gpt-4o","messages":[{"role":"user","content":"record me"}]}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "{\"recorded\":true}");

    // A cassette file was written.
    let files: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("json"))
        .collect();
    assert_eq!(files.len(), 1, "expected one recorded cassette");

    // Replaying (no record) from the same dir returns the recorded bytes.
    let app = app_with_cassettes(dir.path(), None);
    let (_, _, body) = post(
        app,
        "/v1/chat/completions",
        r#"{"model":"gpt-4o","messages":[{"role":"user","content":"record me"}]}"#,
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

    // Record the streaming exchange.
    let app = app_with_cassettes(
        dir.path(),
        Some(RecordConfig {
            upstream: Some(upstream),
        }),
    );
    let (status, ct, body) = post(
        app,
        "/v1/chat/completions",
        r#"{"model":"gpt-4o","stream":true,"messages":[{"role":"user","content":"hi"}]}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(ct.contains("text/event-stream"));
    assert_eq!(body, chunks.concat());

    // The saved cassette stored timed frames, not a flat body.
    let file = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(Result::ok)
        .find(|e| e.path().extension().and_then(|s| s.to_str()) == Some("json"))
        .unwrap()
        .path();
    let cassette: Cassette =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert!(
        cassette.response.body.is_none(),
        "streaming cassette should not use a flat body"
    );
    assert!(
        cassette.response.frames.len() >= 2,
        "expected multiple captured frames, got {}",
        cassette.response.frames.len()
    );
    // A non-first frame should carry a recorded delay reflecting the upstream gap.
    assert!(
        cassette
            .response
            .frames
            .iter()
            .skip(1)
            .any(|f| f.delay_ms > 0),
        "expected a non-zero inter-chunk delay to be captured"
    );

    // Replay reproduces the bytes (and re-applies the timing).
    let app = app_with_cassettes(dir.path(), None);
    let start = std::time::Instant::now();
    let (_, _, body) = post(
        app,
        "/v1/chat/completions",
        r#"{"model":"gpt-4o","stream":true,"messages":[{"role":"user","content":"hi"}]}"#,
    )
    .await;
    assert_eq!(body, chunks.concat());
    assert!(
        start.elapsed() >= Duration::from_millis(40),
        "replay should re-apply the recorded inter-chunk delay"
    );
}
