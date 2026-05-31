//! Protocol adapters. Each adapter owns one provider's wire format — request
//! parsing and exact response serialization (including SSE framing). They share
//! the provider-neutral [`crate::core`] model and the fixture engine, so adding
//! a provider is a new adapter, not a change to the engine.

pub mod anthropic;
pub mod openai;
pub mod openai_responses;
