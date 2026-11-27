//! Tests unitaires des agents Antigravity (CRUD, secrets, lookup, inventaire).

use crate::agents::{self, CreateArgs, Runner, Updates};
use serde_json::{Map, json};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

static SEQ: AtomicU32 = AtomicU32::new(0);

struct TempWs(PathBuf);

impl TempWs {
    fn new(tag: &str) -> Self {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("swarm-agy-ut-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TempWs(dir)
    }
}

impl Drop for TempWs {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn create_read_update_delete_roundtrip() {
    let ws = TempWs::new("crud");
    let created = agents::create(
        &ws.0,
        &CreateArgs {
            name: "nova",
            prompt: "Tu es Nova.",
            runner: Runner::Antigravity,
            model: Some("gemini-4-neon-medium"),
            copy_env_from: None,
            clone_from: None,
        },
    )
    .unwrap();
    assert!(created.prompt_path.is_file());
    assert!(created.config_path.is_file());
    assert!(agents::exists_for(&ws.0, "nova", Runner::Antigravity));
    assert_eq!(agents::peek_runner(&ws.0, "nova"), Some(Runner::Antigravity));

    let listed = agents::list(&ws.0);
    let info = listed.iter().find(|a| a.name == "nova").expect("nova listé");
    assert_eq!(info.runner, Runner::Antigravity);
    assert_eq!(info.model, "gemini-4-neon-medium");

    // Mise à jour modèle + env secret + prompt.
    let mut env = Map::new();
    env.insert("GEMINI_API_KEY".into(), json!("secret-api-key-99999"));
    let changes = agents::update(
        &ws.0,
        "nova",
        Some(Runner::Antigravity),
        &Updates {
            model: Some("gemini-4-neon-high".into()),
            env: Some(env),
            prompt: Some("Tu es Nova v2.".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(changes.iter().any(|c| c.contains("gemini-4-neon-high")));

    // Secrets masqués à la lecture.
    let files = agents::get_configs(&ws.0, "nova", Some(Runner::Antigravity)).unwrap();
    let settings = &files.iter().find(|(k, _)| k == "settings.json").unwrap().1;
    assert!(settings.contains("gemini-4-neon-high"));
    assert!(!settings.contains("secret-api-key-99999"));

    // Réécriture fichier invalide refusée, fichier non supporté refusé.
    let bad_json = agents::update(
        &ws.0,
        "nova",
        Some(Runner::Antigravity),
        &Updates { file: Some(("settings.json".into(), "{invalide".into())), ..Default::default() },
    );
    assert!(bad_json.is_err());
    let bad_file = agents::update(
        &ws.0,
        "nova",
        Some(Runner::Antigravity),
        &Updates { file: Some((".mcp.json".into(), "{}".into())), ..Default::default() },
    );
    assert!(bad_file.is_err());

    // Suppression propre.
    let del = super::delete::delete(&ws.0, "nova");
    assert_eq!(del.files.len(), 2);
    assert!(del.errors.is_empty());
    assert!(!agents::exists_for(&ws.0, "nova", Runner::Antigravity));
}

#[test]
fn copy_env_from_copies_env_and_rejects_missing_source() {
    let ws = TempWs::new("copyenv");
    std::fs::write(
        ws.0.join("src_settings.json"),
        r#"{"env":{"GEMINI_API_KEY":"k1","CUSTOM":"v1"}}"#,
    )
    .unwrap();
    agents::create(
        &ws.0,
        &CreateArgs {
            name: "cloned",
            prompt: "p",
            runner: Runner::Antigravity,
            model: None,
            copy_env_from: Some("src_settings.json"),
            clone_from: None,
        },
    )
    .unwrap();
    let raw = std::fs::read_to_string(ws.0.join(".antigravity/settings_cloned.json")).unwrap();
    assert!(raw.contains("\"CUSTOM\": \"v1\""));

    let err = agents::create(
        &ws.0,
        &CreateArgs {
            name: "fail",
            prompt: "p",
            runner: Runner::Antigravity,
            model: None,
            copy_env_from: Some("absent.json"),
            clone_from: None,
        },
    );
    assert!(err.is_err());
}

#[test]
fn resolve_detects_ambiguity_between_claude_and_antigravity() {
    let ws = TempWs::new("ambig");
    std::fs::create_dir_all(ws.0.join(".claude")).unwrap();
    std::fs::write(ws.0.join(".claude/settings_dual.json"), r#"{"env":{}}"#).unwrap();
    agents::create(
        &ws.0,
        &CreateArgs {
            name: "dual",
            prompt: "p",
            runner: Runner::Antigravity,
            model: None,
            copy_env_from: None,
            clone_from: None,
        },
    )
    .unwrap();

    let err = agents::resolve(&ws.0, "dual", None).unwrap_err();
    assert!(err.contains("claude") && err.contains("antigravity"), "{err}");
    assert_eq!(agents::resolve(&ws.0, "dual", Some(Runner::Antigravity)), Ok(Runner::Antigravity));
}
