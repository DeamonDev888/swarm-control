//! Renommage du binaire : **plus aucune identité par nom de fichier** depuis la 0.2.0.
//!
//! Un `swarm-control` copié ou renommé sous le nom d'un agent (`discord`, `discord.exe`) ne
//! désigne plus cet agent : [`renamed`] le repère et `cli::run` le refuse avec le message de
//! migration (code 2). Un agent persistant est un binaire **compilé** pour lui
//! (`swarm-control build <agent>`, `create_agent {executable:true}`), dont le nom et la classe
//! sont gravés dans le code (PLAN §4.8, décision 2 du §3). `--agent NAME` reste disponible pour
//! le développement et les tests.
//!
//! [`agent_name`] ne survit que comme alias de compatibilité pour l'ancien `cli/mod.rs`
//! (0.1.x) : il rend le même nom, mais ce nom ne désigne plus un agent à servir.

use std::ffi::OsStr;
use std::path::Path;

/// Message de migration affiché quand l'exécutable n'est pas `swarm-control`.
pub const MIGRATION: &str = "depuis la 0.2.0, un binaire renommé ne sert plus d'agent : compilez-le \
                             avec `swarm-control build <agent>` (ou create_agent {executable:true}) ; \
                             `--agent NAME` reste disponible pour le développement";

/// Le nom de fichier de l'exécutable, sans dossier ni extension, s'il ne désigne pas
/// `swarm-control` lui-même (`swarm-control`, `swarm_control…`, dans quelque casse que ce
/// soit). `Some(stem)` = binaire renommé, à refuser ; `None` = instance d'administration.
pub fn renamed(argv0: &OsStr) -> Option<String> {
    let stem = Path::new(argv0).file_stem()?.to_str()?;
    let lower = stem.to_ascii_lowercase();
    if stem.is_empty() || lower.starts_with("swarm-control") || lower.starts_with("swarm_control") {
        return None;
    }
    Some(stem.to_string())
}

/// Message d'erreur pour un binaire renommé (`renamed_stem` vient de [`renamed`]).
pub fn migration_error(renamed_stem: &str) -> String {
    format!("exécutable « {renamed_stem} » : {MIGRATION}")
}

#[cfg(test)]
mod tests {
    use super::{MIGRATION, migration_error, renamed};
    use std::ffi::OsStr;

    fn name(path: &str) -> Option<String> {
        renamed(OsStr::new(path))
    }

    #[test]
    fn a_renamed_binary_is_detected_not_served() {
        // Le nom est seulement repéré, pour refuser le lancement : plus jamais pour
        // choisir l'agent servi.
        assert_eq!(name("/opt/agents/discord").as_deref(), Some("discord"));
        assert_eq!(name("agents/discord.exe").as_deref(), Some("discord"));
        assert_eq!(name("trade_2").as_deref(), Some("trade_2"));
        // `\\` ne sépare les dossiers que sous Windows.
        #[cfg(windows)]
        assert_eq!(name(r"C:\agents\discord.exe").as_deref(), Some("discord"));
    }

    #[test]
    fn swarm_control_itself_is_not_renamed() {
        assert_eq!(name("swarm-control"), None);
        assert_eq!(name("/usr/local/bin/Swarm-Control.exe"), None);
        #[cfg(windows)]
        assert_eq!(name(r"C:\bin\Swarm-Control.exe"), None);
        assert_eq!(name("target/debug/deps/swarm_control-1a2b3c"), None);
        assert_eq!(name(""), None);
    }

    #[test]
    fn the_migration_message_names_both_ways_forward() {
        let error = migration_error("discord");
        assert!(error.starts_with("exécutable « discord » : "), "{error}");
        assert!(error.contains("0.2.0"), "{error}");
        assert!(error.contains("swarm-control build <agent>"), "{error}");
        assert!(error.contains("executable:true"), "{error}");
        assert!(MIGRATION.contains("--agent NAME` reste disponible"), "{MIGRATION}");
    }
}
