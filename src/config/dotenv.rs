//! Lecture des fichiers `.env`.

use std::collections::HashMap;
use std::path::Path;

/// Charge un `.env` : n'ajoute que les clés absentes de l'env système et de
/// la table (le premier fichier lu gagne, comme `dotenv` sans `override`).
pub fn load_into(path: &Path, into: &mut HashMap<String, String>) {
    let Ok(content) = std::fs::read_to_string(path) else {
        return;
    };
    for (key, value) in parse(&content) {
        if std::env::var_os(&key).is_none() && !into.contains_key(&key) {
            into.insert(key, value);
        }
    }
}

pub fn parse(content: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            continue;
        }
        out.push((key.to_string(), unquote(value.trim()).to_string()));
    }
    out
}

fn unquote(value: &str) -> &str {
    let quoted = value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')));
    if quoted {
        &value[1..value.len() - 1]
    } else if let Some(idx) = value.find(" #") {
        value[..idx].trim_end()
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn parses_comments_quotes_and_export() {
        let parsed = parse("# c\nA=1\nexport B=\"deux mots\"\nC='x' \nD=v # note\nbad line\n");
        assert_eq!(
            parsed,
            vec![
                ("A".into(), "1".into()),
                ("B".into(), "deux mots".into()),
                ("C".into(), "x".into()),
                ("D".into(), "v".into()),
            ]
        );
    }
}
