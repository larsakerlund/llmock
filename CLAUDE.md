# llmock — project guide

A fast, byte-faithful emulator of LLM provider HTTP APIs for testing apps
without a real LLM. Rust (axum/tokio), served as a standalone HTTP server.

## Architecture

Three layers over a provider-neutral core, so adding a provider is a new adapter
— not a change to the engine:

- `src/core/` — provider-neutral request/response model (`NeutralRequest`,
  `NeutralResponse`, `ToolCall`, `Usage`, `Fault`, `InjectError`, `Outcome`).
- `src/adapters/<provider>/` — one module per wire format: request parse +
  response/stream serialize (incl. exact SSE framing) + error envelope.
  Current: `openai` (Chat Completions + Models), `openai_responses`, `anthropic`.
- `src/fixtures.rs` — match a request to a canned `Outcome`.
- `src/stream.rs` — text chunking + streaming timing.
- `src/sse.rs` — shared SSE byte framing + fault execution (used by all adapters).
- `src/engine.rs` — single resolution path: cassette replay → record → fixture.
- `src/cassette.rs` — record/replay cassettes, matched by the **same** `Match` as
  fixtures (model + last user message, scoped to endpoint + stream). Record proxies
  misses to the real upstream and saves them (streams captured with real timing).

SSE streams are **hand-built byte streams** (`Body::from_stream`), never a
framework SSE helper — we need exact control of the bytes. Serde struct field
order is the on-the-wire order; keep it matching the real API.

## Commit messages

Every commit follows **Conventional Commits** with a **Linux-kernel-style body**.

Subject: `<type>: <summary>`
- Allowed `<type>` prefixes (no scopes): `feat`, `fix`, `docs`, `test`,
  `refactor`, `perf`, `build`, `ci`, `chore`.
- Summary: imperative mood ("add", not "added"/"adds"), lowercase, no trailing
  period, ≤ 72 chars.

Body (after a blank line):
- Wrap at 72 columns. Explain **what** and **why**, not how. Imperative mood.
- Reference behaviour/spec, not implementation trivia.

Example:
```
feat: add anthropic messages adapter

Emulate the Anthropic Messages API at POST /v1/messages, reusing the
neutral core and fixture engine unchanged. Streaming uses the named-event
lifecycle (message_start … message_stop) with text_delta / input_json_delta
deltas; errors use the Anthropic {"type":"error",...} envelope so the SDK
raises typed exceptions.
```

## Linting & formatting (strict)

- `clippy::all` and `clippy::pedantic` are **denied** via `[lints]` in
  `Cargo.toml`, with a small documented allow-list. Treat new warnings as build
  failures — fix them, don't suppress without a comment justifying it.
- Run before every commit:
  ```sh
  cargo fmt
  cargo clippy --all-targets
  cargo test
  ```
- `unsafe_code` is forbidden.

## Fidelity is verified by the real SDKs

The correctness gate is **driving llmock with the genuine provider SDKs** — if
the real SDK parses our bytes into the expected objects, the protocol is
faithful. Run all suites end-to-end:

```sh
./tests/sdk_compat/run.sh
```

Add SDK-compat coverage for **every** new adapter and capability (text,
streaming, tool calls, usage, error injection). Never land an adapter without
its real-SDK e2e test.

See `README.md` for the fidelity scope (protocol vs byte-level vs model-specific
behaviour) and the milestone roadmap.

## Workflow

- Each milestone/feature is done on its own branch, then fast-forward merged to
  `main` and pushed.
- Personal repo: commit author email is `lars@aakerlund.se`; commits are signed
  via 1Password (`op-ssh-sign`).
