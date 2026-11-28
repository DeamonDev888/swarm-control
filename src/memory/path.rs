//! Emplacement du fichier mémoire d'un agent et découverte de ses voisins.

use crate::agents::Runner;
use std::path::{Path, PathBuf};

/// Dossier des mémoires d'un workspace.
pub fn dir(ws: &Path) -> PathBuf {
    ws.join("bridge").join("memory")
}

/// Mémoire d'un agent : `bridge/memory/<runner>-<agent>.jsonl`.
pub fn path(ws: &Path, runner: &str, agent: &str) -> PathBuf {
    dir(ws).join(file_name(runner, agent))
}

/// Nom du fichier mémoire d'un agent.
pub fn file_name(runner: &str, agent: &str) -> String {
    format!("{runner}-{agent}.jsonl")
}

/// `(runner, agent)` d'un nom de fichier `<runner>-<agent>.jsonl` ; `None` s'il ne désigne
/// pas un agent (autre extension, runner inconnu, nom vide).
///
/// Le runner est reconnu en tête : un agent nommé `claude-x` sous Hermes donne
/// `hermes-claude-x.jsonl`, et non l'agent `x` sous Claude.
pub fn parse(file: &str) -> Option<(String, String)> {
    let stem = file.strip_suffix(".jsonl")?;
    for runner in [Runner::Claude, Runner::Hermes, Runner::Antigravity] {
        if let Some(agent) = stem.strip_prefix(&format!("{}-", runner.as_str())) {
            if !agent.is_empty() {
                return Some((runner.as_str().to_string(), agent.to_string()));
            }
        }
    }
    None
}

/// Les fichiers mémoire **des autres agents** du même workspace : `(runner, agent, chemin)`,
/// triés par (agent, runner). Le fichier `runner`-`agent` est exclu ; un nom de fichier
/// inattendu est ignoré, comme une annonce illisible dans le registre.
pub fn neighbours(ws: &Path, runner: &str, agent: &str) -> Vec<(String, String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(dir(ws)) else {
        return Vec::new();
    };
    let mut out: Vec<(String, String, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let (r, a) = parse(entry.file_name().to_str()?)?;
            (r != runner || a != agent).then(|| (a, r, entry.path()))
        })
        .collect();
    out.sort();
    out.into_iter().map(|(a, r, p)| (r, a, p)).collect()
}

#[cfg(test)]
mod tests {
    use super::{dir, file_name, neighbours, parse, path};

    fn ws(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("swarm-memory-path-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn le_fichier_derive_du_runner_et_de_l_agent() {
        let ws = std::path::Path::new("ws");
        assert_eq!(dir(ws), ws.join("bridge").join("memory"));
        assert_eq!(path(ws, "claude", "alice"), ws.join("bridge/memory/claude-alice.jsonl"));
        assert_eq!(file_name("hermes", "bob"), "hermes-bob.jsonl");
    }

    #[test]
    fn un_nom_de_fichier_se_redecoupe_en_runner_et_agent() {
        assert_eq!(parse("claude-alice.jsonl"), Some(("claude".into(), "alice".into())));
        assert_eq!(parse("hermes-claude-x.jsonl"), Some(("hermes".into(), "claude-x".into())));
        assert_eq!(
            parse("antigravity-trade_2.jsonl"),
            Some(("antigravity".into(), "trade_2".into()))
        );
        assert_eq!(parse("claude-.jsonl"), None, "nom d'agent vide");
        assert_eq!(parse("claude-alice.json"), None, "autre extension");
        assert_eq!(parse("litemcp-test.jsonl"), None, "runner inconnu");
        assert_eq!(parse("claude-alice.jsonl.bak"), None);
    }

    #[test]
    fn les_voisins_excluent_le_fichier_de_l_agent_et_les_inattendus() {
        let ws = ws("voisins");
        std::fs::create_dir_all(dir(&ws)).unwrap();
        for name in
            ["claude-alice.jsonl", "hermes-bob.jsonl", "claude-bob.jsonl", "notes.txt", "x.jsonl"]
        {
            std::fs::write(dir(&ws).join(name), "").unwrap();
        }
        let found = neighbours(&ws, "claude", "alice");
        let names: Vec<String> =
            found.iter().map(|(runner, agent, _)| format!("{runner}-{agent}")).collect();
        assert_eq!(names, ["claude-bob", "hermes-bob"], "trié par agent puis runner, sans soi");
        assert!(found[0].2.ends_with("claude-bob.jsonl"));
        // Le même nom d'agent sous un autre runner est bien un voisin.
        let same_name = neighbours(&ws, "claude", "bob");
        assert_eq!(same_name.len(), 2, "claude-alice et hermes-bob, pas claude-bob (soi)");
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn sans_dossier_il_n_y_a_pas_de_voisins() {
        assert!(neighbours(&ws("absent"), "claude", "alice").is_empty());
    }
}
