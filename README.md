# llmock

A fast, **byte-faithful emulator of LLM provider HTTP APIs** for testing your app
without access to a real LLM. Point your SDK's `base_url` at llmock and it serves
canned fixture responses that look exactly like the real provider — same JSON
shapes, same streaming wire format, same error envelopes.

> Status: early. Milestone 1 implements **OpenAI Chat Completions (non-streaming)**
> plus the Models endpoints. Streaming, error injection, configurable latency, and
> the Anthropic Messages API are on the roadmap below.

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

## Endpoints (milestone 1)

| Method | Path | Notes |
|--------|------|-------|
| POST | `/v1/chat/completions` | Non-streaming. `stream: true` returns 501 for now. |
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
- [ ] M2 — Streaming (`chat.completion.chunk` SSE, `[DONE]`, `include_usage`)
- [ ] M3 — Error/failure injection (429/500/timeouts/malformed streams) + configurable latency (TTFT, inter-token)
- [ ] M4 — Tool/function calling (streamed argument fragments)
- [ ] M5 — Record/replay cassettes (proxy a real API once, replay exactly)
- [ ] M6 — OpenAI Responses API; per-provider path prefixes (`/openai`, `/anthropic`, …)
- [ ] M7 — Anthropic Messages API; then Gemini

## License

MIT
