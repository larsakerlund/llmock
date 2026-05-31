# llmock

A fast, **byte-faithful emulator of LLM provider HTTP APIs** for testing your app
without access to a real LLM. Point your SDK's `base_url` at llmock and it serves
canned fixture responses that look exactly like the real provider — same JSON
shapes, same streaming wire format, same error envelopes.

> Status: early. Implements **OpenAI Chat Completions** (streaming + non-streaming)
> with configurable latency and error/failure injection, plus the Models
> endpoints. The Anthropic Messages API and more are on the roadmap below.

## Why

Testing an LLM-backed app against the real API is slow, costly, non-deterministic,
and needs network + keys. Mocking at the HTTP layer (rather than stubbing your own
client) means you test your real request/response plumbing — including SDK parsing
and streaming — against responses you control.

## Quick start

```sh
cargo run -- --port 8080 --fixtures fixtures/example.yaml
```

Then point any OpenAI client at it:

```python
from openai import OpenAI
client = OpenAI(base_url="http://127.0.0.1:8080/v1", api_key="sk-llmock-dummy")
print(client.chat.completions.create(
    model="gpt-4o",
    messages=[{"role": "user", "content": "what's the weather?"}],
).choices[0].message.content)
# -> "It's sunny and 22°C with a light breeze."
```

## Fixtures

Fixtures are YAML rules, tried top to bottom; the first whose `match` holds wins.
A rule with an empty `match: {}` matches anything — keep it last as a fallback.

```yaml
rules:
  - match:
      user_contains: "weather"     # substring of the last user message
    respond:
      content: "It's sunny and 22°C with a light breeze."
      finish_reason: stop          # stop | length | tool_calls | content_filter
      usage: { prompt_tokens: 12, completion_tokens: 9 }

  - match: { model: "gpt-4o-mini" } # exact model name
    respond:
      content: "Mock reply from the gpt-4o-mini fixture."

  - match: {}                       # fallback
    respond:
      content: "This is a default mock response from llmock."
```

With no `--fixtures` file, a single built-in fallback response is used.

### Streaming & latency

When a request sets `stream: true`, the response is emitted as the provider's
native SSE stream. A rule can shape the timing and granularity:

```yaml
  - match: { user_contains: "weather" }
    respond:
      content: "It's sunny and 22°C with a light breeze."
      stream:
        ttft_ms: 50          # delay before the first token
        inter_token_ms: 10   # delay between tokens
        chunk_by: word       # word | char | <positive integer> (chars/chunk)
```

Server-wide defaults (used when a rule doesn't override) come from flags/env:
`--default-ttft-ms`, `--default-inter-token-ms`, `--default-chunk-by` — handy for
applying realistic latency fleet-wide without editing fixtures.

### Error & failure injection

Fixtures are developer-owned: you compose exactly the failure scenarios you need
to test your app's error handling. A rule can return an HTTP **error** instead of
a response:

```yaml
  - match: { user_contains: "boom" }
    error:
      status: 429
      type: rate_limit_error                 # defaults to api_error
      message: "Rate limit reached for gpt-4o."
      code: rate_limit_exceeded              # optional
```

The error is returned with the given status and a faithful provider error
envelope — for both streaming and non-streaming requests (upfront errors precede
the stream, as the real API does). The genuine SDK raises the matching typed
exception (e.g. `RateLimitError`, `AuthenticationError`).

Or stream normally, then misbehave mid-stream with a **fault** (streaming only):

```yaml
  - match: { user_contains: "truncate" }
    respond:
      content: "This response will be cut off partway through the stream."
      fault:
        kind: truncate        # truncate | malformed | hang
        after_tokens: 4       # emit N deltas, then fault (default 1)
        # hold_ms: 60000      # for kind: hang — stall this long, then give up
```

- `truncate` — drop the connection after N deltas (no final chunk, no `[DONE]`).
- `malformed` — emit a broken SSE frame after N deltas (tests parse-error paths).
- `hang` — stall for `hold_ms` after N deltas (tests client read timeouts).

### Tool / function calling

Return tool calls instead of text. `finish_reason` defaults to `tool_calls` and
`content` becomes `null`, matching the real API:

```yaml
  - match: { user_contains: "forecast" }
    respond:
      tool_calls:
        - name: get_weather
          arguments:               # a mapping → serialized to a JSON string
            location: Tokyo
            unit: celsius
        # - name: another_tool     # multiple calls supported (each its own index)
        #   arguments: '{"x":1}'   # …or give arguments as a raw JSON string
```

When streamed, this emits the opening tool-call delta (id/type/name) followed by
the `function.arguments` as fragments — exactly OpenAI's wire behaviour — so the
genuine SDK reassembles them into valid JSON. Argument fragmentation follows the
same `chunk_by` granularity as text (use `chunk_by: char` for fine-grained
fragments).

## Endpoints

| Method | Path | Notes |
|--------|------|-------|
| POST | `/v1/chat/completions` | Streaming (`stream: true`, incl. `stream_options.include_usage`) and non-streaming; text, tool calls, errors, and faults. |
| GET  | `/v1/models` | Lists a default model catalogue. |
| GET  | `/v1/models/{id}` | Returns a model object for any id (lenient). |
| GET  | `/healthz` | Liveness probe (not part of the emulated surface). |

## Architecture

Three decoupled layers over a provider-neutral core, so adding a provider is a new
adapter — not a change to the engine:

```
HTTP → Protocol Adapter (per-API wire parse/serialize, incl. SSE framing)
     → Fixture Engine    (match request → canned response)
     → Stream Simulator  (chunk response into deltas with configurable timing)
```

- `src/core/`            — provider-neutral request/response model
- `src/adapters/openai/` — OpenAI wire format (request, response, models, errors)
- `src/fixtures.rs`      — matching rules
- `src/util.rs`          — id/timestamp helpers

## Fidelity testing

Faithfulness is verified by running the **genuine provider SDKs** against llmock:

```sh
python3 -m venv tests/sdk_compat/.venv
tests/sdk_compat/.venv/bin/pip install openai
cargo run -- --fixtures fixtures/example.yaml &        # start server
tests/sdk_compat/.venv/bin/python tests/sdk_compat/test_openai.py
```

If the real SDK parses our bytes and yields the expected objects, the format is
faithful. Golden byte-diff tests against captured real responses come next.

## Roadmap

- [x] M1 — OpenAI Chat Completions (non-streaming) + Models, fixture engine, SDK-compat test
- [x] M2 — Streaming (`chat.completion.chunk` SSE, `[DONE]`, `include_usage`) + configurable latency (TTFT, inter-token, chunking)
- [x] M3 — Error/failure injection (HTTP errors + mid-stream truncate/malformed/hang faults)
- [x] M4 — Tool/function calling (non-streaming + streamed argument fragments)
- [ ] M5 — Record/replay cassettes (proxy a real API once, replay exactly)
- [ ] M6 — OpenAI Responses API; per-provider path prefixes (`/openai`, `/anthropic`, …)
- [ ] M7 — Anthropic Messages API; then Gemini

## License

MIT
