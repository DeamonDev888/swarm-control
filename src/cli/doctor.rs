//! `doctor` : diagnostics **non destructifs** du workspace et de l'installation.
//!
//! Avec `--agent X`, ajoute les prérequis du modèle de cet agent : variables requises,
//! serveurs MCP attendus, binaire du runner (voir `agents::templates`).
//!
//! Chaque contrôle écrit une ligne `✓` (tout va bien) ou `✗` (à corriger). Seuls les échecs
//! **indispensables** — le workspace, l'écriture de `bridge/` et de `bridge/history/` — font
//! sortir en code 1 : un binaire absent ou un `.mcp.json` manquant se voit, mais n'empêche pas
//! de servir.

use super::args::Options;
use super::target;
use crate::agents::{Runner, compile, templates};
use crate::{agents, binaries, peers};
use serde_json::Value;
use std::fmt::Display;
use std::io;
use std::path::{Path, PathBuf};

pub fn run(options: &Options, ws: &Path) -> Result<(), String> {
    let mut checks = Checks::default();
    workspace(&mut checks, ws);
    mcp_config(&mut checks, ws);
    agent_inventory(&mut checks, ws);
    binaires(&mut checks);
    registry(&mut checks);
    instances(&mut checks);
    advertised_agents(&mut checks);
    writable(&mut checks, ws);
    compilation(&mut checks, ws);
    // `--agent X` : prérequis du modèle de cet agent (variables, serveurs MCP, binaire).
    if let Some(agent) = options.agent.as_deref() {
        template_checks(&mut checks, options, ws, agent);
    }

    for line in &checks.lines {
        println!("{line}");
    }
    println!("\n{}", checks.summary());
    if checks.essential > 0 {
        return Err(format!("{} contrôle(s) indispensable(s) en échec.", checks.essential));
    }
    Ok(())
}

/// Prérequis du modèle d'un agent (`doctor --agent X`) : variables requises fournies,
/// serveurs MCP attendus déclarés dans le workspace, binaire du runner présent. Tout se
/// signale sans bloquer : ces absences ne rendent pas l'instance inutilisable.
fn template_checks(checks: &mut Checks, options: &Options, ws: &Path, agent: &str) {
    let runner = match agents::resolve(ws, agent, options.runner) {
        Ok(runner) => runner,
        Err(e) => {
            checks.warn("modèle", format!("{agent} : {e}"));
            return;
        }
    };
    let class = agents::class_of(ws, agent, runner).unwrap_or_else(|| "-".into());
    let Some(name) = templates::template_of(ws, agent, runner) else {
        checks.ok("modèle", format!("{agent} : aucun (créé sans modèle)"));
        return;
    };
    let loaded = match templates::load(ws, &name) {
        Ok(loaded) => loaded,
        Err(e) => {
            checks.warn("modèle", format!("{agent} : '{name}' illisible ({e})"));
            return;
        }
    };
    checks.ok("modèle", format!("{agent} : '{name}' (classe : {class})"));
    // Variables requises : toutes présentes, ou la liste de celles qui manquent.
    let missing = templates::missing_vars(&loaded.template);
    let count = loaded.template.required_vars.len();
    if missing.is_empty() {
        let detail = if count == 0 {
            format!("{agent} : aucune (modèle '{name}')")
        } else {
            format!("{agent} : {count} présente(s) (modèle '{name}')")
        };
        checks.ok("variables requises", detail);
    } else {
        checks.warn(
            "variables requises",
            format!("{agent} : manquantes {} (modèle '{name}')", missing.join(", ")),
        );
    }
    // Serveurs MCP attendus : déclarés dans le `.mcp.json` du workspace ?
    let absent = templates::missing_servers(ws, &loaded.template);
    let count = loaded.template.expected_mcp_servers.len();
    if absent.is_empty() {
        let detail = if count == 0 {
            format!("{agent} : aucun (modèle '{name}')")
        } else {
            format!("{agent} : {count} déclaré(s) (modèle '{name}')")
        };
        checks.ok("serveurs MCP attendus", detail);
    } else {
        checks.warn(
            "serveurs MCP attendus",
            format!("{agent} : absents du workspace : {} (modèle '{name}')", absent.join(", ")),
        );
    }
    // Binaire du runner : sans lui, aucun tour ne démarre.
    let binary = match runner {
        Runner::Claude => binaries::claude_binary(),
        Runner::Hermes => binaries::hermes_binary(),
        Runner::Antigravity => binaries::antigravity_binary(),
    };
    if binary.is_file() {
        checks.ok("binaire du runner", format!("{agent} : {}", binary.display()));
    } else {
        checks.warn("binaire du runner", format!("{agent} : introuvable ({})", binary.display()));
    }
    // Port de pont conseillé par le modèle : information, jamais un échec.
    if let Some(port) = loaded.template.defaults.bridge_port {
        checks.ok("pont conseillé", format!("{agent} : --bridge {port} (modèle '{name}')"));
    }
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

/// « compilation d'agents » (PLAN §4.8) : `cargo` dans le `PATH` (ou variable `CARGO`),
/// architecture de l'hôte, dossier de compilation partagé `<ws>/build/target` inscriptible.
/// Rien d'indispensable : sans Rust, les agents tournent quand même via l'instance — seul
/// l'exécutable dédié manque.
fn compilation(checks: &mut Checks, ws: &Path) {
    let target = compile::target_dir(ws);
    let writable = probe_writable(&target);
    let (ok, detail) = compilation_detail(
        compile::cargo_path().as_deref(),
        std::env::consts::ARCH,
        &target,
        writable.as_ref().map(|_| ()),
    );
    if ok {
        checks.ok("compilation d'agents", detail);
    } else {
        checks.warn("compilation d'agents", detail);
    }
}

/// Texte de la ligne « compilation d'agents », sans aucune I/O : `cargo` trouvé (ou `None` :
/// message avec le lien rustup), architecture de l'hôte, dossier partagé `target` et résultat
/// de sa sonde d'écriture. Rend `(tout_va_bien, détail)`.
fn compilation_detail(
    cargo: Option<&Path>,
    arch: &str,
    target: &Path,
    target_writable: Result<(), &io::Error>,
) -> (bool, String) {
    let (cargo_ok, cargo_text) = match cargo {
        Some(path) => (true, format!("cargo {}", path.display())),
        None => {
            (false, format!("cargo introuvable — installe Rust depuis {}", compile::RUSTUP_URL))
        }
    };
    let (target_ok, target_text) = match target_writable {
        Ok(()) => (true, format!("dossier partagé {} inscriptible", target.display())),
        Err(e) => (false, format!("dossier partagé {} non inscriptible ({e})", target.display())),
    };
    (cargo_ok && target_ok, format!("{cargo_text} · cible {arch} · {target_text}"))
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
    use super::{Checks, compilation, compilation_detail, mcp_count, probe_writable};
    use crate::agents::compile;
    use std::io;
    use std::path::Path;

    /// La ligne « compilation d'agents » est formatée sans I/O : chemin de `cargo` (ou lien
    /// rustup), cible de l'hôte, dossier partagé et résultat de sa sonde.
    #[test]
    fn la_ligne_de_compilation_formate_cargo_la_cible_et_le_dossier_partage() {
        let target = Path::new("/ws/build/target");
        let (ok, line) =
            compilation_detail(Some(Path::new("/outils/cargo")), "x86_64", target, Ok(()));
        assert!(ok, "{line}");
        assert_eq!(
            line,
            "cargo /outils/cargo · cible x86_64 · dossier partagé /ws/build/target inscriptible"
        );
        // Sans cargo : le lien rustup, et la ligne ne passe pas.
        let (ok, line) = compilation_detail(None, "aarch64", target, Ok(()));
        assert!(!ok, "{line}");
        assert!(line.contains("cargo introuvable") && line.contains("https://rustup.rs"), "{line}");
        assert!(line.contains("cible aarch64") && line.contains("inscriptible"), "{line}");
        // Dossier partagé non inscriptible : l'erreur est montrée, la ligne ne passe pas.
        let err = io::Error::other("accès refusé");
        let (ok, line) = compilation_detail(Some(Path::new("cargo")), "x86_64", target, Err(&err));
        assert!(!ok, "{line}");
        assert!(line.contains("non inscriptible (accès refusé)"), "{line}");
    }

    /// Le contrôle réel écrit une seule ligne, jamais indispensable, avec la cible de l'hôte
    /// et le dossier partagé du workspace (`cargo` trouvé ou non selon la machine).
    #[test]
    fn le_controle_de_compilation_n_est_jamais_indispensable() {
        let ws =
            std::env::temp_dir().join(format!("swarm-doctor-compilation-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&ws);
        std::fs::create_dir_all(&ws).unwrap();
        let mut checks = Checks::default();
        compilation(&mut checks, &ws);
        let _ = std::fs::remove_dir_all(&ws);
        assert_eq!(checks.lines.len(), 1, "{:?}", checks.lines);
        assert_eq!(checks.essential, 0, "jamais indispensable");
        let line = &checks.lines[0];
        assert!(
            line.starts_with("✓ compilation d'agents ")
                || line.starts_with("✗ compilation d'agents "),
            "{line}"
        );
        assert!(line.contains(&format!("cible {}", std::env::consts::ARCH)), "{line}");
        assert!(line.contains(&compile::target_dir(&ws).display().to_string()), "{line}");
    }

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
