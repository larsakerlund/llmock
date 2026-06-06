# Contributing to llmock

Thanks for your interest in improving llmock. This guide covers the dev setup,
the checks every change must pass, and the conventions the project follows.

## Reporting bugs and asking questions

Open an [issue](https://github.com/larsakerlund/llmock/issues); the templates
guide bug reports and feature requests. The most useful bug report is the request
you sent, the response or error you got, and what you expected. A minimal fixture
or recorded cassette that reproduces it helps most.

## Development setup

llmock is a Rust project. The toolchain is pinned in
[`rust-toolchain.toml`](rust-toolchain.toml) (Rust 1.96+); rustup picks it up
automatically.

```sh
git clone https://github.com/larsakerlund/llmock
cd llmock
cargo build
cargo run -- --fixtures fixtures/example.yaml
```

## The gate

Install [pre-commit](https://pre-commit.com) once:

```sh
pre-commit install
```

It runs `cargo fmt` and `cargo clippy` on commit and `cargo test` on push (the
same checks CI enforces), checks your commit message against
[Conventional Commits](#commit-messages), and runs a few file sanity checks.

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
- **The real-SDK end-to-end suite** lives in `e2e/sdk_compat/` and runs the
  genuine provider SDKs against llmock with `./e2e/sdk_compat/run.sh` (only
  [uv](https://docs.astral.sh/uv/) is required; it provisions Python and the SDKs
  from the pinned lockfile). A new adapter or capability needs coverage here;
  don't land one without it.

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
  `deps`, `build`, `ci`, `chore`. Use `deps` for the app's runtime dependencies
  and `build` for dev-dependencies and other build or tooling changes.
- Summary in imperative mood ("add", not "added"/"adds"), lowercase, no trailing
  period, at most 72 characters. It becomes the changelog line for `feat`, `fix`,
  `perf`, and `deps` commits, so write it for a reader who never saw the diff.

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

## Releases

Releases are automated with
[release-please](https://github.com/googleapis/release-please). It reads the
Conventional Commit history on `main` and keeps an open release PR that bumps the
version in `Cargo.toml` and `Cargo.lock` and updates `CHANGELOG.md`. The bump
follows the commit types: a `feat` bumps the minor, a `fix` bumps the patch, and
a `BREAKING CHANGE` bumps the minor while the project is pre-1.0 (so the major
stays `0` until we declare stability). The changelog shows `feat`, `fix`, `perf`,
and `deps`; the rest are hidden. `perf` and `deps` are recorded but don't cut a
release on their own, so they ride the next `feat`/`fix` release. Merging the
release PR tags the release, publishes the GitHub Release, and pushes the
versioned image to `ghcr.io/larsakerlund/llmock`. Don't edit `CHANGELOG.md` or
the version by hand; write good commit messages and merge the release PR when you
want to cut a release.

## Submitting changes

1. Fork the repository and create a branch from `main`.
2. Make your change with tests. A new adapter or capability needs its real-SDK
   e2e coverage (see [Test organization](#test-organization)).
3. Run the gate locally and keep it green: `cargo fmt`, `cargo clippy
   --all-targets`, `cargo test`, and `cargo deny check`, plus
   `./e2e/sdk_compat/run.sh` when you touch an adapter.
4. Write [Conventional Commit](#commit-messages) messages.
5. Open a pull request against `main`. CI runs the same gate; a maintainer
   reviews and merges once it is green.

## License

By contributing, you agree that your contributions are licensed under the
[MIT License](LICENSE).
