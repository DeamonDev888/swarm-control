//! Une entrée de session `{ id, ts }`.

use serde_json::Value;

/// Durée de vie d'une session : 30 jours.
pub const TTL_MS: u64 = 30 * 24 * 60 * 60 * 1000;

/// Identifiant d'une entrée : objet `{id}` ou ancienne chaîne nue.
pub fn extract_id(value: &Value) -> Option<String> {
    match value {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Object(o) => {
            o.get("id").and_then(Value::as_str).filter(|s| !s.is_empty()).map(String::from)
        }
        _ => None,
    }
}

/// Entrée plus vieille que le TTL (une entrée sans `ts` n'expire jamais).
pub fn is_expired(value: &Value, now: u64) -> bool {
    value.get("ts").and_then(Value::as_u64).is_some_and(|ts| now.saturating_sub(ts) >= TTL_MS)
}
