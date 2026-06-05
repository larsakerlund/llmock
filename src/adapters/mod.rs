//! Protocol adapters. Each adapter owns one vendor's wire formats — request
//! parsing and exact response serialization (including SSE framing). A vendor
//! that exposes several formats keeps them as submodules (e.g. `openai::chat`
//! and `openai::responses`). They share the provider-neutral [`crate::core`]
//! model and the fixture engine, so adding a provider is a new adapter, not a
//! change to the engine.

pub(crate) mod anthropic;
pub(crate) mod content;
pub(crate) mod gemini;
pub(crate) mod openai;
