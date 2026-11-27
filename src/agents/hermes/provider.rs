//! Déduction du provider Hermes à partir du nom du modèle.

pub fn detect_provider(model: &str) -> &'static str {
    let m = model.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| m.contains(w));
    if has(&["glm", "zai"]) {
        "zai"
    } else if has(&["minimax"]) {
        "minimax-cn"
    } else if has(&["claude", "sonnet", "opus", "haiku"]) {
        "anthropic"
    } else if has(&["gpt"]) || m.starts_with("o1") || m.starts_with("o3") {
        "openai-api"
    } else if has(&["deepseek"]) {
        "deepseek"
    } else if has(&["gemini"]) {
        "gemini"
    } else if has(&["qwen"]) {
        "qwen-oauth"
    } else if has(&["grok", "xai"]) {
        "xai"
    } else {
        "openai-api"
    }
}

#[cfg(test)]
mod tests {
    use super::detect_provider;

    #[test]
    fn detects_known_families() {
        assert_eq!(detect_provider("MiniMax-M3"), "minimax-cn");
        assert_eq!(detect_provider("glm-5.3-flash"), "zai");
        assert_eq!(detect_provider("claude-opus-4-7"), "anthropic");
        assert_eq!(detect_provider("deepseek-v4-flash"), "deepseek");
        assert_eq!(detect_provider("inconnu"), "openai-api");
    }
}
