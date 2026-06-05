# Contributing to llmock

Thanks for your interest in improving llmock. This guide covers the dev setup,
the checks every change must pass, and the conventions the project follows.

## Development setup

llmock is a Rust project. The toolchain is pinned in
[`rust-toolchain.toml`](rust-toolchain.toml) (Rust 1.88+); rustup picks it up
automatically.

```sh
git clone https://github.com/larsakerlund/llmock
cd llmock
cargo build
cargo run -- --fixtures fixtures/example.yaml
```

## The gate

Run all three before every commit. CI runs the same and treats warnings as
errors:

```sh
cargo fmt
cargo clippy --all-targets
cargo test
```

Linting is strict: `clippy::all` and `clippy::pedantic` are denied via `[lints]`
in `Cargo.toml`, with a small documented allow-list. Treat a new warning as a
build failure and fix it rather than suppress it. If a suppression is genuinely
warranted, add it with a comment justifying why. `unsafe_code` is forbidden.

## Supply-chain gate

`cargo deny check` must pass. CI runs it to enforce advisories, licenses, bans,
and sources from [`deny.toml`](deny.toml). Run it locally when you change
dependencies:

```sh
cargo deny check
```

## Manifest

Keep the `[package]` table in `Cargo.toml` complete and current:
`description`, `license`, `repository`, `readme`, `keywords`, `categories`, and
`rust-version`. The MSRV (`rust-version = "1.88"`) tracks
[`rust-toolchain.toml`](rust-toolchain.toml), which stays the single source for
the toolchain channel; bump both together.

## Fidelity is verified by the real SDKs

The correctness gate is driving llmock with the genuine provider SDKs: if the
real SDK parses our bytes into the expected objects, the protocol is faithful.
Run the suites end to end:

```sh
./e2e/sdk_compat/run.sh
```

The only prerequisite is [uv](https://docs.astral.sh/uv/): it provisions the
Python interpreter and the provider SDKs from the pinned
`e2e/sdk_compat/uv.lock`, so the suite needs no preinstalled Python or manual
venv. The first run downloads the SDKs from PyPI and caches them.

Add SDK-compat coverage for every new adapter and capability (text, streaming,
tool calls, usage, and error injection). Never land an adapter without its
real-SDK end-to-end test.

## Test organization

Tests live in three places, split by what they need to see:

- **Unit tests** live inline in the module they cover, under
  `#[cfg(test)] mod tests`.
- **In-crate black-box router tests** live under `src/tests/`: `src/tests/mod.rs`
  declares `mod wire;` and `mod cassette;`, gated by `#[cfg(test)] mod tests;` in
  `src/main.rs`. They drive the assembled router and reach private items such as
  `build_app`, so they sit in-crate rather than in a top-level `tests/`
  directory. The crate is binary-only and exposes no library API, so a real
  `tests/` integration dir would see nothing to call.
- **The real-SDK end-to-end suite** lives in `e2e/` (`e2e/sdk_compat/`) and is
  the fidelity gate, run via `./e2e/sdk_compat/run.sh`.

## Adding a provider

A new provider is a new `src/adapters/<provider>/` module: request parse, plus
response and stream serialize, plus the error envelope, over the unchanged
neutral core and fixture engine. See [ARCHITECTURE.md](ARCHITECTURE.md) for the
layer model and the per-adapter file layout.

## Commit messages

Every commit follows
[Conventional Commits](https://www.conventionalcommits.org/) with a
Linux-kernel-style body.

**Subject:** `<type>: <summary>`

- Allowed types (no scopes): `feat`, `fix`, `docs`, `test`, `refactor`, `perf`,
  `build`, `ci`, `chore`.
- Summary in imperative mood ("add", not "added"/"adds"), lowercase, no trailing
  period, at most 72 characters.

**Body** (after a blank line): wrap at 72 columns; explain what and why, not how;
imperative mood; reference behaviour or spec, not implementation trivia. Don't
reference internal planning artifacts (milestone numbers like `M1`/`M7`, plan
steps, "phase N") in the subject or body; describe the change for a reader who
never saw the plan.

**Breaking changes:** append `!` after the type (for example, `feat!: …`) and add
a `BREAKING CHANGE: <what broke and how to migrate>` footer after the body. The
footer may accompany any type, not just `feat`.

```
feat: add anthropic messages adapter

Emulate the Anthropic Messages API at POST /v1/messages, reusing the
neutral core and fixture engine unchanged. Streaming uses the named-event
lifecycle (message_start … message_stop) with text_delta / input_json_delta
deltas; errors use the Anthropic {"type":"error",...} envelope so the SDK
raises typed exceptions.
```

## Workflow

Each feature is done on its own branch, then fast-forward merged to `main` and
pushed.

## License

By contributing, you agree that your contributions are licensed under the
[MIT License](LICENSE).
