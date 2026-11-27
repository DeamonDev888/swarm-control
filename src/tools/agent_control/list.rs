//! Action `list` : tous les runs connus.

use super::snapshot::to_json;
use crate::registry;
use serde_json::{Value, json};

pub fn all() -> Value {
    let runs: Vec<Value> = registry::all().iter().map(to_json).collect();
    json!({ "count": runs.len(), "runs": runs })
}
