//! Jeton Anthropic.

use crate::config;

/// `ANTHROPIC_AUTH_TOKEN`, sinon le premier `ANTHROPIC_AUTH_TOKEN_<N>`.
pub fn anthropic_token() -> Option<String> {
    if let Some(t) = config::var("ANTHROPIC_AUTH_TOKEN").filter(|s| !s.is_empty()) {
        return Some(t);
    }
    let mut keys: Vec<String> = std::env::vars()
        .map(|(k, _)| k)
        .chain(config::get().dotenv().keys().cloned())
        .filter(|k| is_numbered_token(k))
        .collect();
    keys.sort();
    keys.dedup();
    keys.iter().find_map(|k| config::var(k).filter(|s| !s.is_empty()))
}

fn is_numbered_token(key: &str) -> bool {
    key.strip_prefix("ANTHROPIC_AUTH_TOKEN_")
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::is_numbered_token;

    #[test]
    fn numbered_tokens() {
        assert!(is_numbered_token("ANTHROPIC_AUTH_TOKEN_2"));
        assert!(!is_numbered_token("ANTHROPIC_AUTH_TOKEN_"));
        assert!(!is_numbered_token("ANTHROPIC_AUTH_TOKEN_FALLBACK"));
    }
}
