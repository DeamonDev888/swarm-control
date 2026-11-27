//! Identité par nom d'exécutable : `sniper` (ou `sniper.exe`) sert l'agent `sniper`.
//!
//! Le binaire `swarm-control` copié sous le nom d'un agent devient l'exécutable de cet
//! agent ; sous son propre nom (`swarm-control`, `swarm_control…`), il reste l'instance
//! d'administration.

use std::ffi::OsStr;
use std::path::Path;

/// L'agent désigné par le nom de l'exécutable, ou `None` pour `swarm-control` lui-même.
pub fn agent_name(argv0: &OsStr) -> Option<String> {
    let stem = Path::new(argv0).file_stem()?.to_str()?;
    let lower = stem.to_ascii_lowercase();
    if stem.is_empty() || lower.starts_with("swarm-control") || lower.starts_with("swarm_control") {
        return None;
    }
    Some(stem.to_string())
}

#[cfg(test)]
mod tests {
    use super::agent_name;
    use std::ffi::OsStr;

    fn name(path: &str) -> Option<String> {
        agent_name(OsStr::new(path))
    }

    #[test]
    fn the_file_name_gives_the_agent() {
        assert_eq!(name("/opt/agents/sniper").as_deref(), Some("sniper"));
        assert_eq!(name("agents/sniper.exe").as_deref(), Some("sniper"));
        assert_eq!(name("trade_2").as_deref(), Some("trade_2"));
        // `\` ne sépare les dossiers que sous Windows.
        #[cfg(windows)]
        assert_eq!(name(r"C:\agents\sniper.exe").as_deref(), Some("sniper"));
    }

    #[test]
    fn swarm_control_itself_is_not_an_agent() {
        assert_eq!(name("swarm-control"), None);
        assert_eq!(name("/usr/local/bin/Swarm-Control.exe"), None);
        #[cfg(windows)]
        assert_eq!(name(r"C:\bin\Swarm-Control.exe"), None);
        assert_eq!(name("target/debug/deps/swarm_control-1a2b3c"), None);
        assert_eq!(name(""), None);
    }
}
