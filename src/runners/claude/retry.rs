//! Retente avec un jeton de secours sur erreur d'authentification, de quota ou serveur.

use super::envelope::last_object;
use crate::process::ProcOutput;
use serde_json::{Map, Value};

/// Variables de l'agent contenant un jeton de secours, par ordre d'essai.
const FALLBACK_KEYS: [&str; 4] =
    ["AUTH_FALLBACK_1", "AUTH_FALLBACK_2", "AUTH_FALLBACK_3", "ANTHROPIC_AUTH_TOKEN_FALLBACK"];

/// Statuts HTTP pour lesquels un autre jeton peut réussir.
const RETRYABLE_STATUS: [u64; 5] = [401, 429, 500, 502, 503];

const RETRYABLE_PHRASES: [&str; 10] = [
    "unauthorized",
    "invalid api key",
    "api key invalid",
    "authentication failed",
    "invalid authentication",
    "rate limit",
    "quota exhausted",
    "limit exhausted",
    "service unavailable",
    "internal server error",
];

pub fn fallbacks(env: &Map<String, Value>) -> Vec<String> {
    FALLBACK_KEYS
        .iter()
        .filter_map(|k| env.get(*k).and_then(Value::as_str))
        .filter(|v| !v.is_empty())
        .map(String::from)
        .collect()
}

/// Erreur d'authentification / quota / serveur : un autre jeton peut réussir.
pub fn is_retryable(out: &ProcOutput) -> bool {
    if let Some(status) =
        last_object(&out.stdout).and_then(|v| v.get("api_error_status").and_then(Value::as_u64))
    {
        return RETRYABLE_STATUS.contains(&status);
    }
    let hay = out.stderr.to_lowercase();
    RETRYABLE_PHRASES.iter().any(|p| hay.contains(p))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::End;
    use serde_json::json;

    fn out(stdout: &str, stderr: &str) -> ProcOutput {
        ProcOutput { stdout: stdout.into(), stderr: stderr.into(), code: Some(1), end: End::Exited }
    }

    #[test]
    fn retries_on_http_status_and_phrases_only() {
        assert!(is_retryable(&out(r#"{"api_error_status":429}"#, "")));
        assert!(!is_retryable(&out(r#"{"api_error_status":400}"#, "rate limit")));
        assert!(is_retryable(&out("", "Rate limit exceeded")));
        assert!(!is_retryable(&out("{}", "boom")));
    }

    #[test]
    fn collects_fallback_tokens_in_order() {
        let env = json!({"AUTH_FALLBACK_2": "b", "AUTH_FALLBACK_1": "a", "AUTH_FALLBACK_3": ""})
            .as_object()
            .unwrap()
            .clone();
        assert_eq!(fallbacks(&env), vec!["a".to_string(), "b".to_string()]);
    }
}
