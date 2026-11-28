//! Compilation native d'un agent (PLAN §4.8) : crate généré, icône de la classe, erreur
//! claire de `build` sans crate.
//!
//! La vraie compilation (`cargo build --release`) est un test `#[ignore]` : lent, il a
//! besoin du réseau et d'une chaîne Rust complète.

use crate::harness::{Harness, fake_cli, start};
use crate::rpc::ok;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::Command;

/// `executable:true` écrit le crate de l'agent avant la moindre compilation : `Cargo.toml`,
/// `src/main.rs`, `agent.json`, `build.rs` et l'icône Windows de sa classe.
#[test]
fn executable_true_generates_the_crate_and_the_class_icon() {
    let h = start(&[("SWARM_SKIP_AGENT_BUILD", "1")]);
    let r = ok(
        &h,
        "create_agent",
        json!({"name": "compil", "runner": "claude", "prompt": "Tu es compil.", "executable": true}),
    );
    let build = &r["build"];
    assert_eq!(build["status"], "skipped", "{}", build);
    let dir = PathBuf::from(build["buildDir"].as_str().expect("buildDir"));
    for file in ["Cargo.toml", "src/main.rs", "agent.json", "build.rs"] {
        assert!(dir.join(file).is_file(), "{} absent ({})", file, dir.display());
    }
    // L'icône de la classe (16/32/48 px dans un .ico) : posée pour `build.rs` du crate.
    let icon = dir.join("resources").join("icon.ico");
    let bytes = std::fs::read(&icon).unwrap_or_default();
    assert_eq!(&bytes[..4], &[0, 0, 1, 0], "en-tête .ico absent ({})", icon.display());
    // Le nom et la classe sont gravés dans le source généré, jamais déduits du fichier.
    let main = std::fs::read_to_string(dir.join("src/main.rs")).unwrap();
    assert!(main.contains("const AGENT: &str = \"compil\";"), "{main}");
    assert!(main.contains("const CLASS: &str = "), "{main}");
}

/// `swarm-control build <agent>` sans crate généré : erreur claire, code 1, rien de lancé.
#[test]
fn build_without_a_generated_crate_is_an_explicit_error() {
    let h = start(&[]);
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_swarm-control"));
    let output = cli(&h, &exe, &["build", "jamais"]).expect("lancement de la CLI");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("le crate n'a jamais été généré"), "{stderr}");
}

/// La commande prête : workspace du harnais, faux CLI, `TEMP` isolé (annonces LiteMCP).
fn cli(h: &Harness, exe: &Path, args: &[&str]) -> std::io::Result<std::process::Output> {
    let root = h.ws.parent().expect("ws sous la racine du harnais").to_path_buf();
    let mut cmd = Command::new(exe);
    cmd.arg("--workspace").arg(&h.ws).args(args);
    for (key, value) in [
        ("USERPROFILE", root.clone()),
        ("HOME", root.clone()),
        ("TEMP", root.clone()),
        ("TMP", root.clone()),
        ("TMPDIR", root.clone()),
        ("HERMES_HOME", h.hermes_home.clone()),
        ("CLAUDE_BIN", fake_cli()),
        ("HERMES_BIN", fake_cli()),
        ("AGY_BIN", fake_cli()),
    ] {
        cmd.env(key, value);
    }
    cmd.output()
}
