//! Détection d'une session reprise devenue invalide.

use super::RunResult;

const DEAD_SESSION_PHRASES: [&str; 9] = [
    "no conversation found",
    "conversation not found",
    "invalid conversation",
    "unknown conversation",
    "invalid session",
    "session not found",
    "could not find session",
    "unknown session",
    "no session found",
];

/// Le run a-t-il échoué parce que la session reprise n'existe plus ?
pub fn is_dead(result: &RunResult) -> bool {
    result.error.as_deref().is_some_and(|e| mentions_dead_session(e, &result.raw))
}

fn mentions_dead_session(error: &str, raw: &str) -> bool {
    let hay = format!("{error}\n{raw}").to_lowercase();
    DEAD_SESSION_PHRASES.iter().any(|p| hay.contains(p))
}

#[cfg(test)]
mod tests {
    use super::mentions_dead_session;

    #[test]
    fn detects_dead_sessions_only() {
        assert!(mentions_dead_session("EXIT_CODE_1", "No conversation found with session ID"));
        assert!(mentions_dead_session("EXIT_CODE_1", "Conversation not found: conv-1"));
        assert!(mentions_dead_session("Session not found: x", ""));
        assert!(!mentions_dead_session("TIMEOUT", ""));
    }
}
