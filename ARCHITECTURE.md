# Architecture

llmock emulates several LLM provider HTTP APIs from one engine. The guiding
principle is that adding a provider should mean writing a new adapter rather than
changing the engine. The wire format is the only thing that varies between
providers, so it is the only thing isolated per-provider.

## Layers

A request flows through three decoupled layers over a provider-neutral core:

```
HTTP → Protocol Adapter (per-API wire parse/serialize, incl. SSE framing)
     → Fixture Engine    (match request → canned Outcome)
     → Stream Simulator  (chunk a response into deltas with configurable timing)
```

**Protocol Adapter.** Translates a provider's wire format to and from the neutral
core. Parsing turns an incoming request into a `NeutralRequest`; serialization
turns a resolved `Outcome` back into that provider's exact bytes, including SSE
framing and the error envelope. This is the only layer that knows about a
specific provider.

**Fixture Engine.** Matches a `NeutralRequest` to a canned `Outcome`,
independent of provider. One matching model serves both fixtures and cassettes.

**Stream Simulator.** Chunks a response body into deltas and paces them with
configurable timing: time-to-first-token, inter-token gaps, and burstiness.

The core (`src/core/`) defines the provider-neutral vocabulary every layer
speaks, including `NeutralRequest`, `NeutralResponse`, `ToolCall`, `Usage`,
`Fault`, `InjectError`, and `Outcome`, so the engine never names a provider.

## Resolution path

`src/engine.rs` is the single resolution path for every request, in order:

1. **Cassette replay.** If the request matches a recorded cassette, replay its
   bytes exactly (byte-for-byte, with real captured timing for streams).
2. **Record.** In record mode, a miss is proxied to the real upstream, returned
   to the caller, and saved as a new cassette.
3. **Fixture.** Otherwise, match against the fixture rules and synthesize a
   response.

Cassettes and fixtures are matched by the same `Match` (model plus the last user
message, scoped to endpoint and stream), so there is exactly one matching model
in the system. That is what lets recorded happy paths and hand-authored error
cases compose in one run.

## SSE framing bypasses axum's helper

`src/sse.rs` formats the SSE frames directly (`data: <json>\n\n`, or
`event: <name>\ndata: <json>\n\n` for named events) and the response streams them
as a raw body (`Body::from_stream`), instead of going through axum's `Sse`/`Event`
helper. The reason is faithfulness: matching a provider byte-for-byte means
controlling the exact bytes on the wire, including event names, field order,
whitespace, and the terminating frames, all of which a generic helper would decide
for us. For the same reason, serde struct field order is the on-the-wire order:
fields are serialized in the order they appear in the struct, kept matching the
real API.

`src/sse.rs` also holds the shared fault execution (truncate, malformed, and
hang) used by every adapter, so mid-stream failure behaviour is consistent across
providers.

## Module map

```
src/core/                    provider-neutral request/response model
src/adapters/openai/         OpenAI vendor: chat/, responses/, models, shared error.rs
src/adapters/anthropic/      Anthropic Messages API
src/adapters/gemini/         Google Gemini API
src/adapters/content.rs      shared request-content parsing (string or parts)
src/fixtures.rs              matching rules (the Match model)
src/engine.rs                single resolution path: cassette → record → fixture
src/cassette.rs              record/replay, matched by the same Match as fixtures
src/sse.rs                   shared SSE framing + fault handling
src/stream.rs                text chunking + streaming timing
src/tokenize.rs              usage token counts (tiktoken + ~4-chars/token fallback)
src/config.rs                CLI flags + per-model timing defaults
src/state.rs                 shared server state
src/util.rs                  id/timestamp helpers
src/main.rs                  server wiring + routing
```

Adapters are grouped by vendor. A vendor with one wire format (`anthropic`,
`gemini`) is a single module; a vendor with several (`openai`, which serves Chat
Completions, the Responses API, and Models) keeps each wire format as a submodule
and shares the vendor's `error.rs`. A wire-format module follows the same shape:
`request.rs` (parse), `response.rs` (non-streaming serialize), and `sse.rs`
(streaming serialize), with `mod.rs` wiring them together and exposing the routes.

## Adding a provider

A new provider is a new `src/adapters/<vendor>/` module that parses the vendor's
request into a `NeutralRequest` and serializes an `Outcome` back into the vendor's
wire format. The core, fixture engine, stream simulator, and fault injection are
untouched.

Every adapter must land with real-SDK end-to-end coverage in `tests/sdk_compat/`;
see [CONTRIBUTING.md](CONTRIBUTING.md). Fidelity is verified by driving llmock
with the genuine provider SDKs, not by asserting on our own output.
