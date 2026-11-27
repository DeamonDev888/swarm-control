//! Édition du `.env` d'un profil Hermes.

use crate::agents::files::io;
use serde_json::{Map, Value};
use std::path::Path;

pub fn apply(
    profile_dir: &Path,
    env: &Map<String, Value>,
    changes: &mut Vec<String>,
) -> Result<(), String> {
    let path = profile_dir.join(".env");
    let mut content = std::fs::read_to_string(&path).unwrap_or_default();
    for (key, value) in env {
        let value = value.as_str().unwrap_or_default();
        validate(key, value)?;
        content = set_line(&content, key, value);
        changes.push(format!("Env '{key}' → {}", if value.is_empty() { "(vide)" } else { "***" }));
    }
    std::fs::write(&path, content).map_err(io)
}

fn validate(key: &str, value: &str) -> Result<(), String> {
    if key.is_empty() || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return Err(format!("Nom de variable invalide: {key}"));
    }
    if value.contains(['\n', '\r']) {
        return Err(format!("Valeur multiligne refusée pour {key}"));
    }
    Ok(())
}

/// Remplace la ligne `KEY=…` ou l'ajoute en fin de fichier.
fn set_line(content: &str, key: &str, value: &str) -> String {
    let prefix = format!("{key}=");
    let mut found = false;
    let mut lines: Vec<String> = content
        .lines()
        .map(|l| {
            if l.starts_with(&prefix) {
                found = true;
                format!("{key}={value}")
            } else {
                l.to_string()
            }
        })
        .collect();
    if !found {
        lines.push(format!("{key}={value}"));
    }
    lines.join("\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::{set_line, validate};

    #[test]
    fn replaces_or_appends() {
        assert_eq!(set_line("A=1\nB=2\n", "A", "9"), "A=9\nB=2\n");
        assert_eq!(set_line("A=1\n", "C", "3"), "A=1\nC=3\n");
        assert_eq!(set_line("", "C", "3"), "C=3\n");
    }

    #[test]
    fn rejects_bad_names_and_multiline_values() {
        assert!(validate("A B", "x").is_err());
        assert!(validate("A", "x\ny").is_err());
        assert!(validate("A_B", "x").is_ok());
    }
}
