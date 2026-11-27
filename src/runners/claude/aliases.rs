//! Alias de modèles : `<workspace>/model-aliases.json`.

use crate::store::read_json;
use serde_json::Value;
use std::path::Path;

/// `{"surnom": "vrai-modèle"}` ; la recherche ignore la casse.
pub fn resolve_model(ws: &Path, model: &str) -> String {
    let aliases = read_json(&ws.join("model-aliases.json"));
    aliases
        .as_ref()
        .and_then(Value::as_object)
        .and_then(|m| {
            m.iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(model))
                .and_then(|(_, v)| v.as_str().map(String::from))
        })
        .unwrap_or_else(|| model.to_string())
}
