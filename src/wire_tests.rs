//! In-process wire golden tests.
//!
//! Drive the full router with `tower::oneshot` and assert the **exact bytes**
//! each adapter emits (with volatile ids/timestamps redacted to `<ID>`/`<TS>`).
//! Spec correctness is verified separately by the real-SDK e2e suites; these
//! tests pin the wire format byte-for-byte so a refactor cannot silently change
//! it. Regenerate the expected strings with `cargo test wire_tests::dump --
//! --ignored --nocapture`.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use regex::Regex;
use tower::ServiceExt;

use crate::build_app;
use crate::fixtures::Fixtures;
use crate::state::AppState;

const FIXTURES: &str = r#"
rules:
  - match: { user_contains: "forecast" }
    respond:
      tool_calls:
        - name: get_weather
          arguments: { location: Tokyo, unit: celsius }
  - match: {}
    respond:
      content: "Hello there, friend."
"#;

fn app() -> Router {
    let fixtures = Fixtures::from_yaml(FIXTURES).expect("valid fixtures");
    build_app(AppState::new(fixtures, crate::core::StreamSpec::default()))
}

/// Replace random ids and timestamps with stable placeholders.
fn redact(s: &str) -> String {
    let ids = Regex::new(r"(chatcmpl-|fp_|call_|resp_|msg_|fc_|toolu_)[A-Za-z0-9]+").unwrap();
    let s = ids.replace_all(s, "${1}<ID>");
    let ts = Regex::new(r#""(created|created_at)":\d+"#).unwrap();
    ts.replace_all(&s, r#""$1":<TS>"#).into_owned()
}

async fn post(uri: &str, body: &str) -> (StatusCode, String) {
    let req = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let text = std::str::from_utf8(&bytes).unwrap();
    (status, redact(text))
}

#[tokio::test]
async fn openai_chat_non_stream_text() {
    let (status, out) = post(
        "/v1/chat/completions",
        r#"{"model":"gpt-4o","messages":[{"role":"user","content":"hi"}]}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        out,
        r#"{"id":"chatcmpl-<ID>","object":"chat.completion","created":<TS>,"model":"gpt-4o","system_fingerprint":"fp_<ID>","choices":[{"index":0,"message":{"role":"assistant","content":"Hello there, friend.","refusal":null},"logprobs":null,"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":3,"total_tokens":4}}"#
    );
}

#[tokio::test]
async fn openai_chat_stream_text() {
    let (status, out) = post(
        "/v1/chat/completions",
        r#"{"model":"gpt-4o","stream":true,"messages":[{"role":"user","content":"hi"}]}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let expected = concat!(
        "data: {\"id\":\"chatcmpl-<ID>\",\"object\":\"chat.completion.chunk\",\"created\":<TS>,\"model\":\"gpt-4o\",\"system_fingerprint\":\"fp_<ID>\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"\"},\"logprobs\":null,\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"chatcmpl-<ID>\",\"object\":\"chat.completion.chunk\",\"created\":<TS>,\"model\":\"gpt-4o\",\"system_fingerprint\":\"fp_<ID>\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hello \"},\"logprobs\":null,\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"chatcmpl-<ID>\",\"object\":\"chat.completion.chunk\",\"created\":<TS>,\"model\":\"gpt-4o\",\"system_fingerprint\":\"fp_<ID>\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"there, \"},\"logprobs\":null,\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"chatcmpl-<ID>\",\"object\":\"chat.completion.chunk\",\"created\":<TS>,\"model\":\"gpt-4o\",\"system_fingerprint\":\"fp_<ID>\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"friend.\"},\"logprobs\":null,\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"chatcmpl-<ID>\",\"object\":\"chat.completion.chunk\",\"created\":<TS>,\"model\":\"gpt-4o\",\"system_fingerprint\":\"fp_<ID>\",\"choices\":[{\"index\":0,\"delta\":{},\"logprobs\":null,\"finish_reason\":\"stop\"}]}\n\n",
        "data: [DONE]\n\n",
    );
    assert_eq!(out, expected);
}

#[tokio::test]
async fn openai_chat_stream_tool() {
    let (status, out) = post(
        "/v1/chat/completions",
        r#"{"model":"gpt-4o","stream":true,"messages":[{"role":"user","content":"forecast"}]}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let expected = concat!(
        "data: {\"id\":\"chatcmpl-<ID>\",\"object\":\"chat.completion.chunk\",\"created\":<TS>,\"model\":\"gpt-4o\",\"system_fingerprint\":\"fp_<ID>\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\"},\"logprobs\":null,\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"chatcmpl-<ID>\",\"object\":\"chat.completion.chunk\",\"created\":<TS>,\"model\":\"gpt-4o\",\"system_fingerprint\":\"fp_<ID>\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_<ID>\",\"type\":\"function\",\"function\":{\"name\":\"get_weather\",\"arguments\":\"\"}}]},\"logprobs\":null,\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"chatcmpl-<ID>\",\"object\":\"chat.completion.chunk\",\"created\":<TS>,\"model\":\"gpt-4o\",\"system_fingerprint\":\"fp_<ID>\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"location\\\":\\\"Tokyo\\\",\\\"unit\\\":\\\"celsius\\\"}\"}}]},\"logprobs\":null,\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"chatcmpl-<ID>\",\"object\":\"chat.completion.chunk\",\"created\":<TS>,\"model\":\"gpt-4o\",\"system_fingerprint\":\"fp_<ID>\",\"choices\":[{\"index\":0,\"delta\":{},\"logprobs\":null,\"finish_reason\":\"tool_calls\"}]}\n\n",
        "data: [DONE]\n\n",
    );
    assert_eq!(out, expected);
}

#[tokio::test]
async fn anthropic_non_stream_text() {
    let (status, out) = post(
        "/v1/messages",
        r#"{"model":"claude-opus-4-8","max_tokens":16,"messages":[{"role":"user","content":"hi"}]}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        out,
        r#"{"id":"msg_<ID>","type":"message","role":"assistant","model":"claude-opus-4-8","content":[{"type":"text","text":"Hello there, friend."}],"stop_reason":"end_turn","stop_sequence":null,"usage":{"input_tokens":1,"output_tokens":3}}"#
    );
}

#[tokio::test]
async fn anthropic_stream_text() {
    let (status, out) = post(
        "/v1/messages",
        r#"{"model":"claude-opus-4-8","max_tokens":16,"stream":true,"messages":[{"role":"user","content":"hi"}]}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let expected = concat!(
        "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_<ID>\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"claude-opus-4-8\",\"content\":[],\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{\"input_tokens\":1,\"output_tokens\":1}}}\n\n",
        "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: ping\ndata: {\"type\":\"ping\"}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello \"}}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"there, \"}}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"friend.\"}}\n\n",
        "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":3}}\n\n",
        "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
    );
    assert_eq!(out, expected);
}

#[tokio::test]
async fn gemini_non_stream_text() {
    let (status, out) = post(
        "/v1beta/models/gemini-2.0-flash:generateContent",
        r#"{"contents":[{"role":"user","parts":[{"text":"hi"}]}]}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        out,
        r#"{"candidates":[{"content":{"parts":[{"text":"Hello there, friend."}],"role":"model"},"finishReason":"STOP","index":0}],"usageMetadata":{"promptTokenCount":1,"candidatesTokenCount":3,"totalTokenCount":4},"modelVersion":"gemini-2.0-flash"}"#
    );
}

#[tokio::test]
async fn gemini_stream_text() {
    let (status, out) = post(
        "/v1beta/models/gemini-2.0-flash:streamGenerateContent?alt=sse",
        r#"{"contents":[{"role":"user","parts":[{"text":"hi"}]}]}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let expected = concat!(
        "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Hello \"}],\"role\":\"model\"},\"index\":0}],\"modelVersion\":\"gemini-2.0-flash\"}\n\n",
        "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"there, \"}],\"role\":\"model\"},\"index\":0}],\"modelVersion\":\"gemini-2.0-flash\"}\n\n",
        "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"friend.\"}],\"role\":\"model\"},\"index\":0}],\"modelVersion\":\"gemini-2.0-flash\"}\n\n",
        "data: {\"candidates\":[{\"content\":{\"parts\":[],\"role\":\"model\"},\"finishReason\":\"STOP\",\"index\":0}],\"usageMetadata\":{\"promptTokenCount\":1,\"candidatesTokenCount\":3,\"totalTokenCount\":4},\"modelVersion\":\"gemini-2.0-flash\"}\n\n",
    );
    assert_eq!(out, expected);
}

#[tokio::test]
async fn responses_non_stream_text() {
    let (status, out) = post("/v1/responses", r#"{"model":"gpt-4o","input":"hi"}"#).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        out,
        r#"{"id":"resp_<ID>","object":"response","created_at":<TS>,"model":"gpt-4o","status":"completed","error":null,"incomplete_details":null,"instructions":null,"metadata":{},"output":[{"id":"msg_<ID>","type":"message","status":"completed","role":"assistant","content":[{"type":"output_text","text":"Hello there, friend.","annotations":[]}]}],"parallel_tool_calls":true,"tool_choice":"auto","tools":[],"temperature":null,"top_p":null,"usage":{"input_tokens":1,"input_tokens_details":{"cached_tokens":0},"output_tokens":3,"output_tokens_details":{"reasoning_tokens":0},"total_tokens":4}}"#
    );
}

#[tokio::test]
async fn responses_stream_text() {
    let (status, out) = post(
        "/v1/responses",
        r#"{"model":"gpt-4o","stream":true,"input":"hi"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let expected = concat!(
        "event: response.created\ndata: {\"type\":\"response.created\",\"sequence_number\":0,\"response\":{\"id\":\"resp_<ID>\",\"object\":\"response\",\"created_at\":<TS>,\"model\":\"gpt-4o\",\"status\":\"in_progress\",\"error\":null,\"incomplete_details\":null,\"instructions\":null,\"metadata\":{},\"output\":[],\"parallel_tool_calls\":true,\"tool_choice\":\"auto\",\"tools\":[],\"temperature\":null,\"top_p\":null}}\n\n",
        "event: response.in_progress\ndata: {\"type\":\"response.in_progress\",\"sequence_number\":1,\"response\":{\"id\":\"resp_<ID>\",\"object\":\"response\",\"created_at\":<TS>,\"model\":\"gpt-4o\",\"status\":\"in_progress\",\"error\":null,\"incomplete_details\":null,\"instructions\":null,\"metadata\":{},\"output\":[],\"parallel_tool_calls\":true,\"tool_choice\":\"auto\",\"tools\":[],\"temperature\":null,\"top_p\":null}}\n\n",
        "event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"sequence_number\":2,\"output_index\":0,\"item\":{\"id\":\"msg_<ID>\",\"type\":\"message\",\"status\":\"in_progress\",\"role\":\"assistant\",\"content\":[]}}\n\n",
        "event: response.content_part.added\ndata: {\"type\":\"response.content_part.added\",\"sequence_number\":3,\"item_id\":\"msg_<ID>\",\"output_index\":0,\"content_index\":0,\"part\":{\"type\":\"output_text\",\"text\":\"\",\"annotations\":[]}}\n\n",
        "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"sequence_number\":4,\"item_id\":\"msg_<ID>\",\"output_index\":0,\"content_index\":0,\"delta\":\"Hello \",\"logprobs\":[]}\n\n",
        "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"sequence_number\":5,\"item_id\":\"msg_<ID>\",\"output_index\":0,\"content_index\":0,\"delta\":\"there, \",\"logprobs\":[]}\n\n",
        "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"sequence_number\":6,\"item_id\":\"msg_<ID>\",\"output_index\":0,\"content_index\":0,\"delta\":\"friend.\",\"logprobs\":[]}\n\n",
        "event: response.output_text.done\ndata: {\"type\":\"response.output_text.done\",\"sequence_number\":7,\"item_id\":\"msg_<ID>\",\"output_index\":0,\"content_index\":0,\"text\":\"Hello there, friend.\",\"logprobs\":[]}\n\n",
        "event: response.content_part.done\ndata: {\"type\":\"response.content_part.done\",\"sequence_number\":8,\"item_id\":\"msg_<ID>\",\"output_index\":0,\"content_index\":0,\"part\":{\"type\":\"output_text\",\"text\":\"Hello there, friend.\",\"annotations\":[]}}\n\n",
        "event: response.output_item.done\ndata: {\"type\":\"response.output_item.done\",\"sequence_number\":9,\"output_index\":0,\"item\":{\"id\":\"msg_<ID>\",\"type\":\"message\",\"status\":\"completed\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Hello there, friend.\",\"annotations\":[]}]}}\n\n",
        "event: response.completed\ndata: {\"type\":\"response.completed\",\"sequence_number\":10,\"response\":{\"id\":\"resp_<ID>\",\"object\":\"response\",\"created_at\":<TS>,\"model\":\"gpt-4o\",\"status\":\"completed\",\"error\":null,\"incomplete_details\":null,\"instructions\":null,\"metadata\":{},\"output\":[{\"id\":\"msg_<ID>\",\"type\":\"message\",\"status\":\"completed\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Hello there, friend.\",\"annotations\":[]}]}],\"parallel_tool_calls\":true,\"tool_choice\":\"auto\",\"tools\":[],\"temperature\":null,\"top_p\":null,\"usage\":{\"input_tokens\":1,\"input_tokens_details\":{\"cached_tokens\":0},\"output_tokens\":3,\"output_tokens_details\":{\"reasoning_tokens\":0},\"total_tokens\":4}}}\n\n",
    );
    assert_eq!(out, expected);
}

#[tokio::test]
async fn provider_prefixes_route_to_the_right_adapter() {
    // OpenAI under /openai (Chat Completions + Responses).
    let (status, out) = post(
        "/openai/v1/chat/completions",
        r#"{"model":"gpt-4o","messages":[{"role":"user","content":"hi"}]}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(out.contains("\"object\":\"chat.completion\""), "{out}");

    let (status, out) = post("/openai/v1/responses", r#"{"model":"gpt-4o","input":"hi"}"#).await;
    assert_eq!(status, StatusCode::OK);
    assert!(out.contains("\"object\":\"response\""), "{out}");

    // Anthropic under /anthropic.
    let (status, out) = post(
        "/anthropic/v1/messages",
        r#"{"model":"claude-opus-4-8","max_tokens":16,"messages":[{"role":"user","content":"hi"}]}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(out.contains("\"type\":\"message\""), "{out}");

    // Gemini under /gemini.
    let (status, out) = post(
        "/gemini/v1beta/models/gemini-2.0-flash:generateContent",
        r#"{"contents":[{"role":"user","parts":[{"text":"hi"}]}]}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        out.contains("\"modelVersion\":\"gemini-2.0-flash\""),
        "{out}"
    );
}

/// Regeneration helper: prints redacted output for representative cases.
#[tokio::test]
#[ignore = "prints golden output for manual regeneration"]
async fn dump() {
    let cases: &[(&str, &str)] = &[
        ("/v1/responses", r#"{"model":"gpt-4o","input":"hi"}"#),
        (
            "/v1/responses",
            r#"{"model":"gpt-4o","stream":true,"input":"hi"}"#,
        ),
    ];
    for (uri, body) in cases {
        let (status, out) = post(uri, body).await;
        println!("\n===== {uri} [{status}] =====\n{out}");
    }
}
