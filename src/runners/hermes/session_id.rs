//! Identifiant de session dans la sortie de Hermes.

const MARKER: &str = "session_id:";

/// `session_id: local-18e0…` → `local-18e0…` (dernière occurrence).
pub fn find_session_id(text: &str) -> Option<String> {
    text.lines().rev().find_map(|line| {
        let idx = line.to_lowercase().find(MARKER)?;
        let id: String = line[idx + MARKER.len()..]
            .trim()
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
            .collect();
        (!id.is_empty()).then_some(id)
    })
}

/// Stdout sans les lignes `session_id: …`.
pub fn strip_session_line(stdout: &str) -> String {
    stdout
        .lines()
        .filter(|l| !l.trim().to_lowercase().starts_with(MARKER))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::{find_session_id, strip_session_line};

    #[test]
    fn finds_both_id_formats() {
        assert_eq!(
            find_session_id("x\nsession_id: 20260628_114351_3fa469\n").as_deref(),
            Some("20260628_114351_3fa469")
        );
        assert_eq!(find_session_id("session_id: local-18e0ab").as_deref(), Some("local-18e0ab"));
        assert_eq!(find_session_id("rien"), None);
    }

    #[test]
    fn strips_the_id_line() {
        assert_eq!(strip_session_line("hello\nsession_id: abc\nworld"), "hello\nworld");
    }
}
