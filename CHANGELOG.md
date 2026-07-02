# Changelog

## [0.1.2](https://github.com/larsakerlund/llmock/compare/v0.1.1...v0.1.2) (2026-07-02)


### Dependencies

* bump regex in the cargo-minor group across 1 directory ([#24](https://github.com/larsakerlund/llmock/issues/24)) ([ac81da4](https://github.com/larsakerlund/llmock/commit/ac81da44639c407067ac3f9b4bb791a294d2e68f))

## [0.1.1](https://github.com/larsakerlund/llmock/compare/v0.1.0...v0.1.1) (2026-06-07)


### Dependencies

* bump rand from 0.9.4 to 0.10.1 ([#19](https://github.com/larsakerlund/llmock/issues/19)) ([165eedd](https://github.com/larsakerlund/llmock/commit/165eeddc9eb97b809a08fa2ccc1708c5096bd0d4))

## 0.1.0 (2026-06-06)


### Features

* add --replay-speed to scale replayed stream timing ([20d0866](https://github.com/larsakerlund/llmock/commit/20d0866c24d711c0dbeb125adb6bf316ebe58d7a))
* add deterministic mode for reproducible output ([1c20a38](https://github.com/larsakerlund/llmock/commit/1c20a3807dc0c1704b6ab2d0579be6cf3b7c8563))
* add google gemini adapter ([bdd3b54](https://github.com/larsakerlund/llmock/commit/bdd3b548a61f9d9b7d0286e738a2e6b576a8e88c))
* add jitter to synthesized stream timing ([49b259b](https://github.com/larsakerlund/llmock/commit/49b259bb79c4d17688f56e379d40ba3ddd817140))
* add per-provider upstream overrides for record mode ([25ad2f3](https://github.com/larsakerlund/llmock/commit/25ad2f3d3a426fb3861a8a92b043276881f6bbed))
* add record/replay cassettes ([50e6819](https://github.com/larsakerlund/llmock/commit/50e6819de8767131b31661a7500f944c0a5f69c0))
* apply realistic latency to non-streaming responses ([9da649a](https://github.com/larsakerlund/llmock/commit/9da649a2ef6d4ac6c6e4ff9379eaff6d3cbeef97))
* count usage with a real tokenizer where available ([5da7a63](https://github.com/larsakerlund/llmock/commit/5da7a63c5b66d31187523f0b1315ec85240cdafc))
* default streaming timing per model with a fallback ([bdb62e9](https://github.com/larsakerlund/llmock/commit/bdb62e9c69d2866be78ad84fa863d24f0cc10a5a))
* forward the api-key header for azure openai recording ([8f15040](https://github.com/larsakerlund/llmock/commit/8f150409cb2e8b34f7b466a235b7a3b5d76bea57))
* match the real anthropic wire bytes exactly ([b991a8d](https://github.com/larsakerlund/llmock/commit/b991a8de36e5f06bdfd1358b4c3c9a398972c935))
* model bursty stream cadence with a mean-preserving mixture ([8af2551](https://github.com/larsakerlund/llmock/commit/8af2551e3e645702b6d204ac7edf5eca9bc01c8e))
* reject oversized request bodies with a configurable limit ([24a135b](https://github.com/larsakerlund/llmock/commit/24a135b7d107ea18180583061b73d0f8bd8c358a))
* replay streaming cassettes with their real timing ([10ad3c0](https://github.com/larsakerlund/llmock/commit/10ad3c05a94e19c9b9425e155318e4f2f8b3f1ca))
* serve providers only under their /{provider} prefix ([d87db1a](https://github.com/larsakerlund/llmock/commit/d87db1a59295d82bee6cc3f7b3ca04683cd2ea46))
* warn when recording on a non-loopback bind ([fd0f5e3](https://github.com/larsakerlund/llmock/commit/fd0f5e3c775d30258ff42d25d7b5ca419cd65b71))


### Bug Fixes

* capture time-to-first-token when recording streams ([4f98b50](https://github.com/larsakerlund/llmock/commit/4f98b50f77bac59f2a1d40b6b03b4150fbcf7132))
* correct non-streaming latency and read errors when recording ([c5271d5](https://github.com/larsakerlund/llmock/commit/c5271d563bfb11c3476fe1a75f042b802ea5e050))
* forward x-goog-api-key when recording ([eb67373](https://github.com/larsakerlund/llmock/commit/eb67373a730e7af41a977e8f0b06c14b992f0d0e))
* harden cassette recording and storage ([b081b01](https://github.com/larsakerlund/llmock/commit/b081b013fbd9b06bf961d6216350fa0f2b933d8a))


### Performance

* skip tiktoken for oversized openai inputs ([216ab80](https://github.com/larsakerlund/llmock/commit/216ab80125505c916363c9a05a37f307557b492b))


### Dependencies

* bump rand from 0.8.6 to 0.9.4 ([d69473a](https://github.com/larsakerlund/llmock/commit/d69473a4f2b0baafb25382a279378c94e0828379)), closes [#8](https://github.com/larsakerlund/llmock/issues/8)
* bump tiktoken-rs from 0.6.0 to 0.12.0 ([#9](https://github.com/larsakerlund/llmock/issues/9)) ([957a72d](https://github.com/larsakerlund/llmock/commit/957a72d987d169cbf9c08831bc58ab49aa335326))
