# llmock

[![CI](https://github.com/larsakerlund/llmock/actions/workflows/ci.yml/badge.svg)](https://github.com/larsakerlund/llmock/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/larsakerlund/llmock?sort=semver)](https://github.com/larsakerlund/llmock/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.96+-orange.svg)](rust-toolchain.toml)
[![Container](https://img.shields.io/badge/ghcr.io-larsakerlund%2Fllmock-2496ed?logo=docker&logoColor=white)](https://github.com/larsakerlund/llmock/pkgs/container/llmock)
[![OpenSSF Scorecard](https://img.shields.io/ossf-scorecard/github.com/larsakerlund/llmock?label=openssf%20scorecard)](https://scorecard.dev/viewer/?uri=github.com/larsakerlund/llmock)

A fast emulator of LLM provider HTTP APIs, for testing your app without access
to a real LLM. Point your SDK's `base_url` at llmock and it serves
responses that look exactly like the real provider: the same JSON shapes, the
same streaming wire format, and the same error envelopes. A single emulator
covers whichever providers your app calls.

## Why llmock

Testing an LLM-backed app against the real API is slow, costly, and
non-deterministic, and it needs network access and keys. Mocking at the HTTP
layer (rather than stubbing your own client) means you test your real
request/response plumbing, including SDK parsing and streaming, against responses
you control.

## Features

- Multiple providers: OpenAI, Anthropic, and Google Gemini, each behind its own
  endpoint prefix.
- Real wire format: exact JSON shapes, native SSE streaming, and provider error
  envelopes, verified by running the genuine provider SDKs end to end.
- Configurable latency, with per-model defaults for time-to-first-token,
  inter-token gaps, burstiness, and chunking. Override them per rule or
  fleet-wide.
- Error and fault injection: return HTTP errors with faithful envelopes, or
  stream normally and then truncate, corrupt, or hang mid-stream.
- Record and replay: proxy a real provider once and replay its bytes exactly,
  streaming timing included.
- A `--deterministic` mode for reproducible test runs.

## Quick start

Run the published container:

```sh
docker run --rm -p 8080:8080 ghcr.io/larsakerlund/llmock:latest
```

Then point your provider's SDK at it. For example, with OpenAI's Python client:

```python
from openai import OpenAI

client = OpenAI(base_url="http://localhost:8080/openai/v1", api_key="unused")
response = client.chat.completions.create(
    model="gpt-4o",
    messages=[{"role": "user", "content": "Hello!"}],
)
print(response.choices[0].message.content)
# -> "This is a mock response from llmock."
```

To return responses keyed on the request, write your own fixtures and mount them
(see [Fixtures](#fixtures)):

```sh
docker run --rm -p 8080:8080 \
  -v "$PWD/fixtures:/fixtures:ro" \
  ghcr.io/larsakerlund/llmock:latest --fixtures /fixtures/example.yaml
```

Every flag has an environment-variable form too, so it drops into a compose
stack alongside your app. A ready-to-edit
[`docker-compose.yml`](docker-compose.yml) is included:

```yaml
services:
  llmock:
    image: ghcr.io/larsakerlund/llmock:latest
    ports: ["8080:8080"]
    volumes:
      - "./fixtures:/fixtures:ro"
      # - "./cassettes:/cassettes:ro"     # replay recorded responses
    command: ["--fixtures", "/fixtures/example.yaml"]
    # command: ["--cassette-dir", "/cassettes"]   # ...and replay them instead
```

Run `llmock --help` for the full list of flags and environment variables.

### Build from source

llmock is a single Rust binary. Build it with Rust 1.96+, pinned in
[`rust-toolchain.toml`](rust-toolchain.toml):

```sh
git clone https://github.com/larsakerlund/llmock
cd llmock
cargo build --release
# binary at ./target/release/llmock
```

Or run it directly during development with `cargo run --`.

## Fixtures

Fixtures are YAML rules, tried top to bottom; the first whose `match` holds wins.
A rule with an empty `match: {}` matches anything, so keep it last as a fallback.

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

With no `--fixtures` file, llmock uses a single built-in fallback response.

### Streaming & latency

When a request sets `stream: true`, the response is emitted as the provider's
native SSE stream. A rule can shape the timing and granularity:

```yaml
  - match: { user_contains: "weather" }
    respond:
      content: "It's sunny and 22°C with a light breeze."
      stream:
        ttft_ms: 600         # delay before the first token
        inter_token_ms: 20   # average delay between tokens
        burstiness: 0.7      # 0..1: clump tokens into bursts like a real model
        chunk_by: word       # word | char | <positive integer> (chars/chunk)
```

Real streams don't pace evenly; most tokens arrive in bursts with occasional
pauses. `burstiness` models that. With probability `burstiness` a gap is zero,
otherwise it's an exponential pause, sized so the average gap stays
`inter_token_ms` (keeping total duration predictable) while the cadence clumps.
At `0` it falls back to even pacing with `jitter_ms` of uniform variation.
Burstiness and jitter are disabled under `--deterministic` for reproducible runs.

Streaming has realistic defaults, so it feels like a real model out of the box,
derived from measurements of real APIs where time-to-first-token dominates and
per-token gaps are bursty. The defaults are per-model, keyed on the request's
model: gpt-4o, gpt-5 nano/mini, the o-series, claude, haiku, and gemini each get
their own measured pace, with a generic fallback for unknown models. Override per
rule, or fleet-wide via `--default-ttft-ms`, `--default-inter-token-ms`,
`--default-burstiness`, `--default-jitter-ms`, and `--default-chunk-by`; a flag
you set overrides every model, and unset flags resolve per-model. Set the delays
to `0` for instant streaming in a fast test suite. (Recorded cassettes ignore
these and replay their own real timing; see `--replay-speed`.)

Non-streaming responses take the same total time. A provider generates the whole
response server-side before replying, so a non-streamed call is no faster than a
streamed one. llmock waits that equivalent total (time-to-first-token plus an
inter-token gap per later token) before returning the JSON, from the same
per-model defaults and knobs; the `0`-delay escape hatch makes it instant too.

### Error & failure injection

Fixtures are developer-owned, so you compose exactly the failure scenarios you
need to test your app's error handling. A rule can return an HTTP error instead
of a response:

```yaml
  - match: { user_contains: "boom" }
    error:
      status: 429
      type: rate_limit_error                 # defaults to api_error
      message: "Rate limit reached for gpt-4o."
      code: rate_limit_exceeded              # optional
```

The error is returned with the given status and a faithful provider error
envelope, for both streaming and non-streaming requests (upfront errors precede
the stream, as the real API does). The genuine SDK raises the matching typed
exception, such as `RateLimitError` or `AuthenticationError`.

Or stream normally, then misbehave mid-stream with a fault (streaming only):

```yaml
  - match: { user_contains: "truncate" }
    respond:
      content: "This response will be cut off partway through the stream."
      fault:
        kind: truncate        # truncate | malformed | hang
        after_tokens: 4       # emit N deltas, then fault (default 1)
        # hold_ms: 60000      # for kind: hang, stall this long then give up
```

- `truncate`: drop the connection after N deltas (no final chunk, no `[DONE]`).
- `malformed`: emit a broken SSE frame after N deltas, to test parse-error paths.
- `hang`: stall for `hold_ms` after N deltas, to test client read timeouts.

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
the `function.arguments` as fragments, exactly matching OpenAI's wire behaviour,
so the genuine SDK reassembles them into valid JSON. Argument fragmentation
follows the same `chunk_by` granularity as text (use `chunk_by: char` for
fine-grained fragments).

## Record & replay

For the highest fidelity, replay real captured responses instead of hand-written
fixtures. Replay is byte-for-byte exact.

### Record your own

1. Start the container in record mode, mounting a writable directory for the
   cassettes:
   ```sh
   docker run --rm -p 8080:8080 -v "$PWD/cassettes:/cassettes" \
     ghcr.io/larsakerlund/llmock:latest --cassette-dir /cassettes --record
   ```
2. Point your app or SDK's base URL at llmock, using your real API key as usual
   (llmock forwards your auth header upstream):

   | Provider | SDK base URL | Auth header forwarded |
   |----------|--------------|-----------------------|
   | OpenAI | `http://localhost:8080/openai/v1` | `Authorization: Bearer` |
   | Azure OpenAI | `http://localhost:8080/openai/v1` + `--upstream-openai https://<resource>.openai.azure.com/openai` | `api-key` (or `Authorization`) |
   | Anthropic | `http://localhost:8080/anthropic` | `x-api-key` |
   | Gemini (`google-genai`) | `http://localhost:8080/gemini` via `HttpOptions(base_url=…)` | `x-goog-api-key` |

3. Make your normal calls. Each request with no matching cassette is proxied to
   the real provider (chosen by endpoint, or by `--upstream`), saved under
   `--cassette-dir`, and the genuine bytes are returned to your app. Fire as many
   as you like; each distinct request becomes its own cassette.
4. Replay offline by restarting without `--record`:
   ```sh
   docker run --rm -p 8080:8080 -v "$PWD/cassettes:/cassettes:ro" \
     ghcr.io/larsakerlund/llmock:latest --cassette-dir /cassettes
   ```
   Now there is no key and no network, and replays are byte-for-byte the real
   responses.

`--upstream` overrides the real provider for every endpoint. Point it at a proxy,
a gateway, or (for Azure) your resource, so that `<upstream>/v1/chat/completions`
is the real URL. To relocate providers independently in one run, use the
per-provider flags, which take precedence over `--upstream`: `--upstream-openai`
(covers Chat and Responses, e.g. an Azure resource), `--upstream-anthropic`, and
`--upstream-gemini`.

Record mode forwards your real API key upstream and is unauthenticated, so don't
expose it on an untrusted network. Replay needs no key and no network.

A cassette matches on `model` plus the last user message (the fixture `Match`),
scoped to its `endpoint` and streaming mode. You record a few real responses and
they replay whenever the prompt is close enough. The recorded `match` is derived
for you, and you can hand-edit it:

```json
{
  "endpoint": "openai.chat",
  "stream": false,
  "match":    { "model": "gpt-4o", "user_contains": "weather" },
  "response": { "status": 200, "content_type": "application/json",
                "body": "{ ...exact server bytes... }" }
}
```

Streaming is captured with its real timing. For an SSE response, each network
chunk is recorded with the actual delay since the previous one, including the
time-to-first-token (the first frame's delay), and replay re-applies them, so a
replayed stream paces like the real one. Streaming cassettes use timed `frames`
in place of `body`:

```json
  "response": { "status": 200, "content_type": "text/event-stream",
                "frames": [ { "delay_ms": 740, "data": "data: {...}\n\n" },
                            { "delay_ms": 25,  "data": "data: {...}\n\n" } ] }
```

Timing granularity is network-chunk level (a provider may coalesce several SSE
events into one read), so TTFT and total duration are faithful while per-token
cadence is approximate. `--replay-speed` scales it: `1.0` is real time, `2.0` is
twice as fast, `0.5` is half speed, and `0` is instant (handy for fast test
suites).

Non-streaming cassettes record their latency too: the request-to-full-response
time is saved as `delay_ms` and replayed before the body (also scaled by
`--replay-speed`), so a replayed non-streamed call is as slow as the real one
was. Cassettes recorded before this carry no `delay_ms` and replay instantly.

Misses fall through to the fixture engine, so cassettes and fixtures compose. A
common pattern is to record the happy paths and hand-author the errors and edge
cases.

## Endpoints

Every provider is served under its own `/{provider}` prefix.

| Method | Path | Notes |
|--------|------|-------|
| POST | `/openai/v1/chat/completions` | Streaming (`stream: true`, incl. `stream_options.include_usage`) and non-streaming; text, tool calls, errors, and faults. |
| POST | `/openai/v1/responses` | OpenAI Responses API: full `response.*` streaming event lifecycle and non-streaming; text and tool calls. |
| POST | `/anthropic/v1/messages` | Anthropic Messages API: `message_start`/`content_block_*`/`message_delta`/`message_stop` streaming and non-streaming; text and tool use. `x-api-key`/`anthropic-version` accepted and ignored. |
| POST | `/gemini/v1beta/models/{model}:generateContent` | Google Gemini: non-streaming `generateContent`. |
| POST | `/gemini/v1beta/models/{model}:streamGenerateContent` | Google Gemini: streaming (`?alt=sse`); text and function calls. |
| GET  | `/openai/v1/models` | Lists a default model catalogue. |
| GET  | `/openai/v1/models/{id}` | Returns a model object for any id (lenient). |
| GET  | `/healthz` | Liveness probe. |

The same fixture rules drive every endpoint, so you author once and test
whichever API (and provider) your app calls. Point each SDK's base URL at its
provider prefix: OpenAI at `http://localhost:8080/openai/v1`, Anthropic at
`http://localhost:8080/anthropic`, and Gemini (google-genai) at
`http://localhost:8080/gemini` via `HttpOptions(base_url=…)`.

## Contributing

Contributions are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md) to get started.

## License

[MIT](LICENSE)
