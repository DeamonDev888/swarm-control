//! Masquage des secrets avant affichage.

use serde_json::{Value, json};

/// Masque les valeurs d'un `.env` ; les noms de variables restent visibles.
pub fn mask_env(content: &str) -> String {
    content
        .lines()
        .map(|l| match l.split_once('=') {
            Some((k, v)) if !k.trim_start().starts_with('#') && !v.trim().is_empty() => {
                format!("{k}=***")
            }
            _ => l.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Masque jetons et clés d'un `settings.json` ; les références `$VAR` restent lisibles.
pub fn mask_settings(raw: &str) -> String {
    let Ok(mut v) = serde_json::from_str::<Value>(raw) else {
        return raw.to_string();
    };
    if let Some(env) = v.get_mut("env").and_then(Value::as_object_mut) {
        for (k, val) in env.iter_mut() {
            if is_secret_name(k) && is_literal(val) {
                *val = json!("***");
            }
        }
    }
    serde_json::to_string_pretty(&v).unwrap_or_else(|_| raw.to_string())
}

fn is_secret_name(name: &str) -> bool {
    let upper = name.to_uppercase();
    ["TOKEN", "KEY", "SECRET", "PASSWORD"].iter().any(|w| upper.contains(w))
}

fn is_literal(value: &Value) -> bool {
    value.as_str().is_some_and(|s| !s.is_empty() && !s.starts_with('$'))
}

#[cfg(test)]
mod tests {
    use super::{mask_env, mask_settings};

    #[test]
    fn settings_secrets_are_masked_but_references_kept() {
        let masked = mask_settings(
            r#"{"env":{"ANTHROPIC_AUTH_TOKEN":"sk-real","ANTHROPIC_BASE_URL":"x","K":"$REF","SOME_KEY":"$REF"}}"#,
        );
        assert!(!masked.contains("sk-real"));
        assert!(masked.contains("$REF"));
    }

    #[test]
    fn env_values_are_masked() {
        assert_eq!(mask_env("A=secret\n# c\nB="), "A=***\n# c\nB=");
    }
}
