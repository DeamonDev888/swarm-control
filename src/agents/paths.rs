//! Emplacements des fichiers d'un agent.
//!
//! Depuis les profils dédiés (PLAN §4.4), les définitions d'un agent vivent
//! dans son dossier propre `<ws>/agents/<nom>/` — même agencement qu'avant
//! à l'intérieur (`.claude/…`, `.antigravity/…`). Les constructeurs
//! renvoient :
//!
//! - le profil dédié dès qu'il existe (agents créés avec les profils) ;
//! - sinon l'emplacement historique partagé tant qu'il existe encore
//!   (compatibilité des agents créés avant) ;
//! - sinon le profil dédié (cas d'une création).
//!
//! Hermes reste sur ses profils natifs, déjà propres à chaque agent.

use crate::config;
use std::path::{Path, PathBuf};

/// Racine des profils dédiés : `<ws>/agents`.
pub fn profiles_root(ws: &Path) -> PathBuf {
    ws.join("agents")
}

/// Profil dédié d'un agent : `<ws>/agents/<nom>`.
pub fn profile_dir(ws: &Path, name: &str) -> PathBuf {
    profiles_root(ws).join(name)
}

/// Emplacement retenu pour un fichier d'agent (voir la doc du module).
fn dedicated_or_legacy(dedicated: PathBuf, legacy: PathBuf) -> PathBuf {
    if dedicated.exists() || !legacy.exists() { dedicated } else { legacy }
}

// --- Claude -------------------------------------------------------------

pub fn claude_dir(ws: &Path) -> PathBuf {
    ws.join(".claude")
}

pub fn claude_agents_dir(ws: &Path) -> PathBuf {
    claude_dir(ws).join("agents")
}

/// `.claude` du profil dédié d'un agent : `<ws>/agents/<nom>/.claude`.
pub fn claude_profile_dir(ws: &Path, name: &str) -> PathBuf {
    profile_dir(ws, name).join(".claude")
}

/// Prompt d'un agent Claude (profil dédié, sinon historique).
pub fn claude_prompt_path(ws: &Path, name: &str) -> PathBuf {
    dedicated_or_legacy(
        claude_profile_prompt_path(ws, name),
        claude_agents_dir(ws).join(format!("{name}.md")),
    )
}

/// Prompt dédié d'un agent Claude, sans repli historique (scan d'inventaire).
pub fn claude_profile_prompt_path(ws: &Path, name: &str) -> PathBuf {
    claude_profile_dir(ws, name).join("agents").join(format!("{name}.md"))
}

/// Skill d'un agent Claude (profil dédié, sinon historique).
pub fn claude_skill_path(ws: &Path, name: &str) -> PathBuf {
    dedicated_or_legacy(
        claude_profile_dir(ws, name).join("agents").join(format!("{name}_skill.md")),
        claude_agents_dir(ws).join(format!("{name}_skill.md")),
    )
}

/// Settings d'un agent Claude (profil dédié, sinon historique).
pub fn claude_settings_path(ws: &Path, name: &str) -> PathBuf {
    dedicated_or_legacy(
        claude_profile_dir(ws, name).join(format!("settings_{name}.json")),
        claude_dir(ws).join(format!("settings_{name}.json")),
    )
}

/// Fichier MCP propre à un agent Claude (profil dédié, sinon historique).
pub fn claude_mcp_path(ws: &Path, name: &str) -> PathBuf {
    dedicated_or_legacy(
        claude_profile_dir(ws, name).join(format!(".mcp.{name}.json")),
        claude_dir(ws).join(format!(".mcp.{name}.json")),
    )
}

/// `.mcp.json` du workspace (ou `.mcp.local.json` s'il est seul).
pub fn global_mcp_path(ws: &Path) -> PathBuf {
    let primary = ws.join(".mcp.json");
    if primary.exists() {
        return primary;
    }
    let local = ws.join(".mcp.local.json");
    if local.exists() { local } else { primary }
}

pub fn hermes_profile_dir(name: &str) -> PathBuf {
    config::native_hermes_home().join("profiles").join(name)
}

// --- Antigravity --------------------------------------------------------

pub fn antigravity_dir(ws: &Path) -> PathBuf {
    ws.join(".antigravity")
}

pub fn antigravity_agents_dir(ws: &Path) -> PathBuf {
    antigravity_dir(ws).join("agents")
}

/// `.antigravity` du profil dédié : `<ws>/agents/<nom>/.antigravity`.
pub fn antigravity_profile_dir(ws: &Path, name: &str) -> PathBuf {
    profile_dir(ws, name).join(".antigravity")
}

/// Prompt d'un agent Antigravity (profil dédié, sinon historique).
pub fn antigravity_prompt_path(ws: &Path, name: &str) -> PathBuf {
    dedicated_or_legacy(
        antigravity_profile_prompt_path(ws, name),
        antigravity_agents_dir(ws).join(format!("{name}.md")),
    )
}

/// Prompt dédié d'un agent Antigravity, sans repli historique (scan).
pub fn antigravity_profile_prompt_path(ws: &Path, name: &str) -> PathBuf {
    antigravity_profile_dir(ws, name).join("agents").join(format!("{name}.md"))
}

/// Settings d'un agent Antigravity (profil dédié, sinon historique).
pub fn antigravity_settings_path(ws: &Path, name: &str) -> PathBuf {
    dedicated_or_legacy(
        antigravity_profile_dir(ws, name).join(format!("settings_{name}.json")),
        antigravity_dir(ws).join(format!("settings_{name}.json")),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        antigravity_profile_prompt_path, antigravity_prompt_path, antigravity_settings_path,
        claude_profile_prompt_path, claude_prompt_path, claude_settings_path, profile_dir,
        profiles_root,
    };
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU32, Ordering};

    static SEQ: AtomicU32 = AtomicU32::new(0);

    /// Workspace temporaire unique, supprimé à la fin du test.
    struct TempWs(PathBuf);

    impl TempWs {
        fn new(tag: &str) -> Self {
            let n = SEQ.fetch_add(1, Ordering::Relaxed);
            let dir =
                std::env::temp_dir().join(format!("swarm-paths-{tag}-{}-{n}", std::process::id()));
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
    fn les_chemins_derivent_du_nom_d_agent() {
        // Ni dédié ni historique sur disque : une création irait au profil dédié.
        let ws = Path::new("ws");
        let name = "alice";
        assert_eq!(profiles_root(ws), ws.join("agents"));
        assert_eq!(profile_dir(ws, name), ws.join("agents").join(name));
        assert_eq!(
            claude_prompt_path(ws, name),
            profile_dir(ws, name).join(".claude").join("agents").join("alice.md")
        );
        assert_eq!(
            claude_settings_path(ws, name),
            profile_dir(ws, name).join(".claude").join("settings_alice.json")
        );
        assert_eq!(
            antigravity_prompt_path(ws, name),
            profile_dir(ws, name).join(".antigravity").join("agents").join("alice.md")
        );
        assert_eq!(
            antigravity_settings_path(ws, name),
            profile_dir(ws, name).join(".antigravity").join("settings_alice.json")
        );
    }

    #[test]
    fn un_agent_historique_reste_sur_ses_fichiers() {
        let ws = TempWs::new("legacy");
        let legacy = ws.0.join(".claude").join("settings_bob.json");
        std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        std::fs::write(&legacy, "{}").unwrap();
        assert_eq!(claude_settings_path(&ws.0, "bob"), legacy);
        // Le prompt, lui, n'existe nulle part : une création viserait le profil.
        assert_eq!(
            claude_prompt_path(&ws.0, "bob"),
            profile_dir(&ws.0, "bob").join(".claude").join("agents").join("bob.md")
        );
    }

    #[test]
    fn le_profil_dedie_prend_le_pas_des_quil_existe() {
        let ws = TempWs::new("dedicated");
        let dedicated = claude_settings_path(&ws.0, "carl");
        std::fs::create_dir_all(dedicated.parent().unwrap()).unwrap();
        std::fs::write(&dedicated, "{}").unwrap();
        // L'historique existe aussi : le profil dédié gagne quand même.
        let legacy = ws.0.join(".claude").join("settings_carl.json");
        std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        std::fs::write(&legacy, "{}").unwrap();
        assert_eq!(claude_settings_path(&ws.0, "carl"), dedicated);
        assert_eq!(
            claude_profile_prompt_path(&ws.0, "carl"),
            profile_dir(&ws.0, "carl").join(".claude").join("agents").join("carl.md")
        );
        assert_eq!(
            antigravity_profile_prompt_path(&ws.0, "carl"),
            profile_dir(&ws.0, "carl").join(".antigravity").join("agents").join("carl.md")
        );
    }
}
