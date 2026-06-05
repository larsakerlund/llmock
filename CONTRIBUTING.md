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

## Fidelity is verified by the real SDKs

The correctness gate is driving llmock with the genuine provider SDKs: if the
real SDK parses our bytes into the expected objects, the protocol is faithful.
Run the suites end to end:

```sh
./tests/sdk_compat/run.sh
```

Add SDK-compat coverage for every new adapter and capability (text, streaming,
tool calls, usage, and error injection). Never land an adapter without its
real-SDK end-to-end test.

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
