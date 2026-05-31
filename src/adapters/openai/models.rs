//! The `/v1/models` listing/retrieval endpoints.
//!
//! Behaviour is intentionally lenient: retrieving any model id returns a valid
//! model object (owned_by `llmock`), so tests can use arbitrary model names
//! without first registering them.

use axum::extract::Path;
use axum::Json;
use serde::Serialize;

use crate::util;

#[derive(Debug, Serialize)]
pub struct Model {
    pub id: String,
    pub object: &'static str,
    pub created: u64,
    pub owned_by: &'static str,
}

#[derive(Debug, Serialize)]
pub struct ModelList {
    pub object: &'static str,
    pub data: Vec<Model>,
}

fn model(id: impl Into<String>) -> Model {
    Model {
        id: id.into(),
        object: "model",
        created: util::unix_now(),
        owned_by: "llmock",
    }
}

/// A small default catalogue so `GET /v1/models` returns something useful.
const DEFAULT_MODELS: &[&str] = &[
    "gpt-4o",
    "gpt-4o-mini",
    "gpt-4-turbo",
    "gpt-3.5-turbo",
];

pub async fn list_models() -> Json<ModelList> {
    Json(ModelList {
        object: "list",
        data: DEFAULT_MODELS.iter().map(|id| model(*id)).collect(),
    })
}

pub async fn get_model(Path(model_id): Path<String>) -> Json<Model> {
    Json(model(model_id))
}
