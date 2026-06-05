# llmock — project guide

A fast, byte-faithful emulator of LLM provider HTTP APIs for testing apps
without a real LLM. Rust (axum/tokio), served as a standalone HTTP server.

## Architecture

`ARCHITECTURE.md` is the fuller, human-facing treatment (layers, resolution path,
module map, adding a provider). The condensed version for working in the code:

Three layers over a provider-neutral core, so adding a provider is a new adapter
— not a change to the engine:

- `src/core/` — provider-neutral request/response model (`NeutralRequest`,
  `NeutralResponse`, `ToolCall`, `Usage`, `Fault`, `InjectError`, `Outcome`).
- `src/adapters/<vendor>/` — one module per vendor: request parse +
  response/stream serialize (incl. exact SSE framing) + error envelope. A vendor
  with several wire formats keeps each as a submodule. Current: `openai`
  (`chat`, `responses`, plus Models, sharing one `error.rs`), `anthropic`,
  `gemini`.
- `src/fixtures.rs` — match a request to a canned `Outcome`.
- `src/stream.rs` — text chunking + streaming timing.
- `src/sse.rs` — shared SSE byte framing + fault execution (used by all adapters).
- `src/engine.rs` — single resolution path: cassette replay → record → fixture.
- `src/cassette.rs` — record/replay cassettes, matched by the **same** `Match` as
  fixtures (model + last user message, scoped to endpoint + stream). Record proxies
  misses to the real upstream and saves them (streams captured with real timing).

`src/sse.rs` formats SSE frames directly (`data: <json>\n\n`) and streams them as
a raw body (`Body::from_stream`), instead of axum's `Sse`/`Event` helper, so we
control the exact bytes. Serde struct field order is the on-the-wire order; keep
it matching the real API.

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
- No internal planning artifacts. Don't reference milestones (`M1`, `M7`), plan
  steps, or "phase N" in the subject or body. Describe the change for a reader
  who never saw the plan: what behaviour it adds or fixes, and why.

Breaking changes:
- Append `!` after the type in the subject (e.g. `feat!: ...`) **and** add a
  `BREAKING CHANGE: <what broke and how to migrate>` footer after the body,
  separated by a blank line.
- A `BREAKING CHANGE:` footer may accompany any `<type>`, not just `feat`.

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
- `cargo deny check` is the supply-chain gate (advisories, licenses, bans,
  sources from `deny.toml`); CI enforces it.
- Keep `[package]` in `Cargo.toml` complete: `description`, `license`,
  `repository`, `readme`, `keywords`, `categories`, `rust-version`. MSRV
  (`rust-version = "1.88"`) tracks `rust-toolchain.toml`, the single channel
  source; bump both together.

## Test organization

- Unit tests live inline under `#[cfg(test)] mod tests` in the module they cover.
- In-crate black-box router tests live under `src/tests/` (`src/tests/mod.rs`
  declares `mod wire;`/`mod cassette;`, gated by `#[cfg(test)] mod tests;` in
  `src/main.rs`). They reach private items like `build_app`, so they stay
  in-crate: the crate is binary-only with no public library API, and a top-level
  `tests/` dir would see nothing to call.
- The real-SDK end-to-end suite lives in `e2e/sdk_compat/` and is the fidelity
  gate (see below). `e2e/` is not Cargo's integration-test dir.

## Fidelity is verified by the real SDKs

The correctness gate is **driving llmock with the genuine provider SDKs** — if
the real SDK parses our bytes into the expected objects, the protocol is
faithful. Run all suites end-to-end:

```sh
./e2e/sdk_compat/run.sh
```

Add SDK-compat coverage for **every** new adapter and capability (text,
streaming, tool calls, usage, error injection). Never land an adapter without
its real-SDK e2e test.

See `README.md` for the fidelity scope (protocol vs byte-level vs model-specific
behaviour).

## Docs writing style

Project docs (`README.md`, `ARCHITECTURE.md`, `CONTRIBUTING.md`) share one voice.
Match it when editing or adding docs:

- Lead with what and why, not how. State what a feature does and the problem it
  solves before any mechanism. Justify non-obvious behaviour (why SSE framing
  bypasses axum's helper, why burstiness is mean-preserving); a claim without its
  reason is half a sentence.
- Show, then explain. Pair each capability with a minimal, copy-pasteable example
  (a YAML rule, a shell command, an SDK snippet), then a tight prose explanation.
  Examples must run as written.
- Be precise and concrete. Name exact flags, fields, paths, and defaults. Prefer
  specifics ("~4-chars/token", "deny `clippy::pedantic`") over vague qualifiers.
  Don't overstate fidelity; call out the residual gaps honestly.
- Tight, declarative prose. Short sentences, active voice, present tense. Use
  tables for enumerable facts (endpoints, provider base URLs). Wrap prose around
  80 columns.
- One home per topic. Reference docs and architecture each live in one place;
  cross-link rather than duplicate.
- Write for a reader who never saw the plan. Don't put internal planning or
  process artifacts in published docs: no milestone numbers (`M1`, `M7`), no plan
  steps, no "phase 2" framing. Document the capability that exists, not the
  sequence we built it in. A roadmap should list real upcoming work or be omitted.

### Avoid AI-writing tells

The docs are human-written technical prose. Keep these machine-writing tics out:

- **Em dashes.** Don't use them as prose asides. Use a comma for a light aside, a
  colon to introduce, parentheses for a true aside, or a period to split. (An em
  dash as a list-label separator, like the `M1 — …` roadmap rows, is fine.)
- **The "X, not Y" / "X — not Y" swerve.** State the positive claim plainly;
  don't manufacture a contrast the reader didn't expect.
- **Emphasis bolding mid-sentence.** Reserve bold for genuine labels (the lead-in
  of a definition-style bullet), not for punching up a random phrase.
- **Inflated vocabulary.** No "leverage, robust, seamless, comprehensive,
  powerful, delve, foster, underscore, showcase, realm, testament." Use the plain
  word or a concrete spec.
- **Filler triads and antithesis.** Don't pad to three adjectives for rhythm, and
  drop "not only X but Y" / "it's not just X, it's Y." Enumerating real things
  (four providers, five test dimensions) is fine; rhythmic filler is not.
- **Throat-clearing and hedging.** Cut "it's worth noting", "it's important to
  note", "in today's world", conjunctive-adverb openers ("Moreover",
  "Furthermore", "Additionally"), and empty intensifiers ("very", "extremely",
  "simply", "seamlessly").
- **No emoji in headings, and sentence-case headings.**

## Workflow

- Each feature is done on its own branch, then fast-forward merged to `main` and
  pushed.
