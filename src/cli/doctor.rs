//! `doctor` : diagnostics **non destructifs** du workspace et de l'installation.
//!
//! Chaque contrôle écrit une ligne `✓` (tout va bien) ou `✗` (à corriger). Seuls les échecs
//! **indispensables** — le workspace, l'écriture de `bridge/` et de `bridge/history/` — font
//! sortir en code 1 : un binaire absent ou un `.mcp.json` manquant se voit, mais n'empêche pas
//! de servir.

use super::args::Options;
use super::target;
use crate::{agents, binaries, peers};
use serde_json::Value;
use std::fmt::Display;
use std::path::{Path, PathBuf};

pub fn run(_options: &Options, ws: &Path) -> Result<(), String> {
    let mut checks = Checks::default();
    workspace(&mut checks, ws);
    mcp_config(&mut checks, ws);
    agent_inventory(&mut checks, ws);
    binaires(&mut checks);
    registry(&mut checks);
    instances(&mut checks);
    advertised_agents(&mut checks);
    writable(&mut checks, ws);

    for line in &checks.lines {
        println!("{line}");
    }
    println!("\n{}", checks.summary());
    if checks.essential > 0 {
        return Err(format!("{} contrôle(s) indispensable(s) en échec.", checks.essential));
    }
    Ok(())
}

fn workspace(checks: &mut Checks, ws: &Path) {
    if ws.is_dir() {
        checks.ok("workspace", ws.display());
    } else {
        checks.fail("workspace", format!("{} n'est pas un dossier", ws.display()));
    }
}

fn mcp_config(checks: &mut Checks, ws: &Path) {
    let file = ws.join(".mcp.json");
    match std::fs::read_to_string(&file) {
        Ok(text) => match mcp_count(&text) {
            Some(count) => checks.ok(".mcp.json", format!("{count} serveur(s) MCP déclaré(s)")),
            None => checks.warn(".mcp.json", "présent mais illisible (JSON invalide)"),
        },
        Err(_) => checks.warn(".mcp.json", "absent (les agents n'auront aucun serveur MCP)"),
    }
}

fn agent_inventory(checks: &mut Checks, ws: &Path) {
    let list = agents::list(ws);
    let counts: Vec<String> = target::RUNNERS
        .iter()
        .map(|runner| {
            let count = list.iter().filter(|agent| agent.runner == *runner).count();
            format!("{count} {}", runner.as_str())
        })
        .collect();
    checks.ok("agents (disque)", format!("{} · {}", list.len(), counts.join(", ")));
}

fn binaires(checks: &mut Checks) {
    let found = [
        ("binaire claude", binaries::claude_binary()),
        ("binaire hermes", binaries::hermes_binary()),
        ("binaire antigravity", binaries::antigravity_binary()),
    ];
    for (label, path) in found {
        if path.is_file() {
            checks.ok(label, path.display());
        } else {
            checks.warn(
                label,
                format!("introuvable ({} — ni PATH, ni variable dédiée)", path.display()),
            );
        }
    }
}

fn registry(checks: &mut Checks) {
    let dir = std::env::temp_dir().join("litemcp-registry");
    if std::fs::read_dir(&dir).is_err() {
        checks.warn("registre LiteMCP", format!("illisible ({})", dir.display()));
        return;
    }
    let announced = peers::discover().len() + peers::instances().len();
    checks.ok("registre LiteMCP", format!("{announced} annonce(s) lisible(s)"));
}

fn instances(checks: &mut Checks) {
    let list = peers::instances();
    let alive = list.iter().filter(|peer| peer.alive).count();
    checks.ok("instances", format!("{alive} vivante(s) sur {} annoncée(s)", list.len()));
}

fn advertised_agents(checks: &mut Checks) {
    let list = peers::discover();
    let alive = list.iter().filter(|peer| peer.alive).count();
    let names: Vec<&str> = list.iter().take(5).map(|peer| peer.name.as_str()).collect();
    let suffix = if names.is_empty() { String::new() } else { format!(" · {}", names.join(", ")) };
    checks
        .ok("agents annoncés", format!("{alive} vivant(s) sur {} annoncé(s){suffix}", list.len()));
}

fn writable(checks: &mut Checks, ws: &Path) {
    let bridge = ws.join("bridge");
    let history = bridge.join("history");
    for (label, dir) in [("bridge", bridge), ("bridge/history", history)] {
        match probe_writable(&dir) {
            Ok(()) => checks.ok(label, "inscriptible"),
            Err(e) => checks.fail(label, format!("non inscriptible ({e})")),
        }
    }
}

/// Crée le dossier, y écrit un fichier sonde, puis l'efface (aucune trace laissée).
fn probe_writable(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let probe: PathBuf = dir.join(format!(".doctor-{}.tmp", std::process::id()));
    std::fs::write(&probe, b"ok")?;
    std::fs::remove_file(&probe)
}

/// Nombre de serveurs MCP d'un `.mcp.json` ; `None` si le JSON est illisible.
fn mcp_count(text: &str) -> Option<usize> {
    let doc: Value = serde_json::from_str(text).ok()?;
    Some(doc.get("mcpServers").and_then(Value::as_object).map_or(0, |servers| servers.len()))
}

/// Lignes de contrôle et compteurs du rapport.
#[derive(Default)]
struct Checks {
    lines: Vec<String>,
    failures: usize,
    essential: usize,
}

impl Checks {
    fn ok(&mut self, label: &str, detail: impl Display) {
        self.lines.push(format!("✓ {label:<20} {detail}"));
    }

    fn warn(&mut self, label: &str, detail: impl Display) {
        self.failures += 1;
        self.lines.push(format!("✗ {label:<20} {detail}"));
    }

    fn fail(&mut self, label: &str, detail: impl Display) {
        self.warn(label, detail);
        self.essential += 1;
    }

    fn summary(&self) -> String {
        let total = self.lines.len();
        let ok = total - self.failures;
        let essential = if self.essential > 0 {
            format!(", dont {} indispensable(s)", self.essential)
        } else {
            String::new()
        };
        format!("{total} contrôle(s) : {ok} ✓, {} ✗{essential}.", self.failures)
    }
}

#[cfg(test)]
mod tests {
    use super::{Checks, mcp_count, probe_writable};

    #[test]
    fn counts_mcp_servers_and_rejects_broken_json() {
        assert_eq!(mcp_count(r#"{"mcpServers":{"a":{},"b":{}}}"#), Some(2));
        assert_eq!(mcp_count("{}"), Some(0));
        assert_eq!(mcp_count("pas du json"), None);
    }

    #[test]
    fn only_essential_failures_do_not_count_as_ok() {
        let mut checks = Checks::default();
        checks.ok("a", "1");
        checks.warn("b", "2");
        assert_eq!(checks.summary(), "2 contrôle(s) : 1 ✓, 1 ✗.");
        checks.fail("c", "3");
        assert_eq!(checks.summary(), "3 contrôle(s) : 1 ✓, 2 ✗, dont 1 indispensable(s).");
        assert_eq!(checks.essential, 1);
    }

    #[test]
    fn the_probe_leaves_nothing_behind_and_reports_impossible_writes() {
        let dir = std::env::temp_dir().join(format!("swarm-doctor-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(probe_writable(&dir.join("bridge").join("history")).is_ok());
        assert!(dir.join("bridge").join("history").is_dir());
        assert_eq!(std::fs::read_dir(dir.join("bridge").join("history")).unwrap().count(), 0);
        // Un dossier qui est en fait un fichier : l'écriture doit échouer, sans panique.
        std::fs::write(dir.join("fichier"), b"x").unwrap();
        assert!(probe_writable(&dir.join("fichier")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
