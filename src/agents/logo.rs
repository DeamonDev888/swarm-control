//! Correspondance classes d'agents ↔ logos (PLAN/TODO §4.1).
//!
//! Les 11 classes d'agent reprennent les slugs des logos de MCP Control
//! (`mcp-command-center`, onglet Swarm, `/swarm-logo/<classe>.svg`) — les mêmes fichiers
//! que `assets/classes/hornet-pixel-<classe>.svg` ici. Le slug se déduit du champ
//! **`class`** publié par `status` / `list_agents` / `/status` du pont, jamais du nom de
//! l'agent : la regex de MCP Control sur le nom ne sert plus que de repli (un agent
//! `sniperbot_analyst` de classe `discord` garde le logo Discord).

// Vocabulaire posé pour les consommateurs de la vague MCP Control (dépôt à part) et pour
// les tests ci-dessous : pas encore appelé dans la lib, gardé comme contrat §4.1.
#![allow(dead_code)]

/// Les 11 classes d'agent, alignées sur les logos de `assets/classes/`.
pub const CLASSES: [&str; 11] = [
    "discord",
    "whatsapp",
    "x",
    "google",
    "custom",
    "coder",
    "browser",
    "explorer",
    "coordinator",
    "monitor",
    "verifier",
];

/// Les slugs de logo reconnus par MCP Control (`logo` = visuel générique de l'onglet Swarm).
pub const CONTROL_SLUGS: [&str; 12] = [
    "logo",
    "coordinator",
    "coder",
    "explorer",
    "verifier",
    "monitor",
    "browser",
    "discord",
    "google",
    "whatsapp",
    "x",
    "custom",
];

/// Slug du logo MCP Control d'une classe : la classe elle-même (mêmes slugs des deux
/// côtés), `custom` pour une classe inconnue ou non déclarée.
pub fn slug(class: &str) -> &'static str {
    CLASSES.iter().find(|c| **c == class).copied().unwrap_or("custom")
}

/// Nom du fichier logo pixel d'une classe dans `assets/classes/`.
pub fn logo_file(class: &str) -> String {
    format!("hornet-pixel-{class}.svg")
}

#[cfg(test)]
mod tests {
    use super::{CLASSES, CONTROL_SLUGS, logo_file, slug};
    use std::path::Path;

    /// À chaque classe correspond un slug MCP Control et son logo pixel sur disque.
    #[test]
    fn chaque_classe_a_un_slug_mcp_control_et_son_logo_sur_disque() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets").join("classes");
        for class in CLASSES {
            assert!(
                CONTROL_SLUGS.contains(&slug(class)),
                "{class} : slug absent de la liste MCP Control"
            );
            let logo = dir.join(logo_file(class));
            assert!(logo.is_file(), "{class} : logo absent ({})", logo.display());
        }
    }

    /// Une classe inconnue replie sur `custom` ; les classes connues restent identity.
    #[test]
    fn le_slug_est_la_classe_et_le_repli_est_custom() {
        assert_eq!(slug("discord"), "discord");
        assert_eq!(slug("coder"), "coder");
        assert_eq!(slug("inconnu-du-workspace"), "custom");
    }
}
