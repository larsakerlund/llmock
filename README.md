# llmock

A fast, **byte-faithful emulator of LLM provider HTTP APIs** for testing your app
without access to a real LLM. Point your SDK's `base_url` at llmock and it serves
canned fixture responses that look exactly like the real provider — same JSON
shapes, same streaming wire format, same error envelopes.

> Status: early but multi-provider. Implements the **OpenAI Chat Completions**,
> **OpenAI Responses**, and **Anthropic Messages** APIs (streaming +
> non-streaming, text + tool calls) with configurable latency and error/failure
> injection, plus the Models endpoints. One fixture set drives all three.

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
| POST | `/v1/responses` | OpenAI Responses API: full `response.*` streaming event lifecycle and non-streaming; text and tool calls. |
| POST | `/v1/messages` | Anthropic Messages API: `message_start`/`content_block_*`/`message_delta`/`message_stop` streaming and non-streaming; text and tool use. `x-api-key`/`anthropic-version` accepted and ignored. |
| GET  | `/v1/models` | Lists a default model catalogue. |
| GET  | `/v1/models/{id}` | Returns a model object for any id (lenient). |
| GET  | `/healthz` | Liveness probe (not part of the emulated surface). |

The same fixture rules drive `/v1/chat/completions`, `/v1/responses`, and
`/v1/messages` — author once, test whichever API (and provider) your app calls.
Point each SDK's `base_url` at llmock: OpenAI → `http://host:8080/v1`,
Anthropic → `http://host:8080`.

## Architecture

Three decoupled layers over a provider-neutral core, so adding a provider is a new
adapter — not a change to the engine:

```
HTTP → Protocol Adapter (per-API wire parse/serialize, incl. SSE framing)
     → Fixture Engine    (match request → canned response)
     → Stream Simulator  (chunk response into deltas with configurable timing)
```

- `src/core/`                    — provider-neutral request/response model
- `src/adapters/openai/`         — OpenAI Chat Completions + Models
- `src/adapters/openai_responses/` — OpenAI Responses API
- `src/adapters/anthropic/`      — Anthropic Messages API
- `src/fixtures.rs`              — matching rules
- `src/stream.rs`               — text chunking + timing
- `src/util.rs`                 — id/timestamp helpers

Adding a provider is a new `adapters/<provider>/` module (request parse + wire
serialize); the core, fixture engine, latency, and fault injection are untouched.

## Fidelity testing

Faithfulness is verified by running the **genuine provider SDKs** against llmock.
One command builds, starts the server, and runs both API suites:

```sh
./tests/sdk_compat/run.sh
```

It exercises the real `openai` and `anthropic` SDKs against all three APIs —
text, streaming, tool calls, usage, and injected errors. If the real SDKs parse
our bytes and yield the expected objects, the format is faithful.

### What "faithful" covers (and what it doesn't)

- **Protocol fidelity — guaranteed, SDK-verified.** The wire shapes are built
  against the providers' own SDK type definitions and validated by running the
  genuine SDKs end-to-end. The protocol does **not** vary by model within a
  provider (`gpt-4o` and `gpt-4o-mini` share identical Chat Completions framing;
  all Claude models share the Messages framing), so one adapter is faithful
  across every model of that provider.
- **Byte-level server fidelity — in progress.** SDK-parse proves the *client*
  accepts our bytes; it does not prove we're byte-identical to the real *server*
  (SDKs ignore unknown fields, field order, null-vs-absent). Record/replay
  cassettes (roadmap) close this by byte-diffing against captured real responses.
- **Token counts are approximate.** `usage` is a word-count heuristic, not a real
  tokenizer, so counts won't match a specific model.
- **Model-specific behaviour is developer-authored.** Things that genuinely vary
  by model — extended thinking/reasoning items, vision, refusals, specific stop
  reasons — aren't emulated automatically; you express whatever you need in your
  (developer-owned) fixtures.

## Roadmap

- [x] M1 — OpenAI Chat Completions (non-streaming) + Models, fixture engine, SDK-compat test
- [x] M2 — Streaming (`chat.completion.chunk` SSE, `[DONE]`, `include_usage`) + configurable latency (TTFT, inter-token, chunking)
- [x] M3 — Error/failure injection (HTTP errors + mid-stream truncate/malformed/hang faults)
- [x] M4 — Tool/function calling (non-streaming + streamed argument fragments)
- [x] M5 — OpenAI Responses API (`/v1/responses`): full `response.*` event lifecycle + non-streaming, text & tool calls
- [x] M7 — Anthropic Messages API (`/v1/messages`): named-event streaming lifecycle + non-streaming, text & tool use, Anthropic error envelope
- [ ] M6 — Record/replay cassettes (proxy a real API once, replay exactly); per-provider path prefixes (`/openai`, `/anthropic`, …)
- [ ] M8 — Google Gemini API

## License

MIT
