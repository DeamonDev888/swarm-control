//! Interpolation de `$VAR` et `${VAR}` dans les chaînes et le JSON.

use super::env;
use serde_json::Value;

/// Remplace `$VAR` et `${VAR}` par leur valeur (vide si absente).
pub fn interpolate(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' {
            if let Some((name, consumed)) = variable_at(input, i) {
                out.push_str(&env::var(name).unwrap_or_default());
                i += consumed;
                continue;
            }
        }
        let ch = input[i..].chars().next().expect("frontière de caractère");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Nom de variable commençant en `start` (sur le `$`) et octets consommés.
fn variable_at(input: &str, start: usize) -> Option<(&str, usize)> {
    let bytes = input.as_bytes();
    let (name, consumed) = if bytes.get(start + 1) == Some(&b'{') {
        let end = input[start + 2..].find('}')?;
        (&input[start + 2..start + 2 + end], end + 3)
    } else {
        let len = input[start + 1..]
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_')
            .count();
        (&input[start + 1..start + 1 + len], len + 1)
    };
    let valid = !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
    valid.then_some((name, consumed))
}

/// Interpole récursivement toutes les chaînes d'une valeur JSON.
pub fn interpolate_json(value: &Value) -> Value {
    match value {
        Value::String(s) => Value::String(interpolate(s)),
        Value::Array(items) => Value::Array(items.iter().map(interpolate_json).collect()),
        Value::Object(map) => {
            Value::Object(map.iter().map(|(k, v)| (k.clone(), interpolate_json(v))).collect())
        }
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::interpolate;

    #[test]
    fn keeps_unrelated_dollars() {
        assert_eq!(interpolate("prix 5$ et $"), "prix 5$ et $");
    }

    #[test]
    fn absent_variables_become_empty() {
        assert_eq!(interpolate("${SWARM_TEST_SURELY_ABSENT}x"), "x");
        assert_eq!(interpolate("é$SWARM_TEST_SURELY_ABSENT-é"), "é-é");
    }
}
