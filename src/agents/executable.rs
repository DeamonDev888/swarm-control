//! Exécutable dédié à un agent, et unit systemd pour le lancer sans terminal.
//!
//! - binaire : `<ws>/bin/<agent><EXE_SUFFIX>`, **compilé** pour cet agent par
//!   `agents::compile` (crate généré sous `<ws>/agents/<agent>/build/`, identité gravée
//!   dans le binaire — PLAN §4.8, décision 2 du §3). Plus de copie de `current_exe()` ;
//! - unit : `<ws>/bridge/units/swarm-<agent>.service`, écrite mais **jamais**
//!   installée ni activée (pas de `/etc`, pas de root).
//!
//! Politique de partage : chaque agent reçoit **son** binaire, nommé d'après lui — un
//! fichier `bin/<agent>` n'est donc référencé que par l'agent qui porte son nom, et
//! `remove` n'a pas de référence croisée à vérifier. Les agents partagent seulement le
//! dossier de compilation (`<ws>/build/target`) et la version de swarm-control.

use super::compile;
use std::path::{Path, PathBuf};

/// Chemin de l'exécutable de l'agent : `<ws>/bin/<agent><EXE_SUFFIX>`.
pub fn path(ws: &Path, agent: &str) -> PathBuf {
    ws.join("bin").join(format!("{agent}{}", std::env::consts::EXE_SUFFIX))
}

/// Chemin de l'unit systemd de l'agent : `<ws>/bridge/units/swarm-<agent>.service`.
pub fn service_path(ws: &Path, agent: &str) -> PathBuf {
    ws.join("bridge").join("units").join(format!("swarm-{agent}.service"))
}

/// Supprime l'exécutable compilé de l'agent **et** son dossier de compilation
/// (`<ws>/agents/<agent>/build/`) s'ils existent. Rend le chemin du binaire s'il a été
/// retiré (le contrat historique de `agents::delete`), `None` sinon.
pub fn remove(ws: &Path, agent: &str) -> Option<PathBuf> {
    let binary = path(ws, agent);
    compile::remove(ws, agent).contains(&binary).then_some(binary)
}

/// Contenu de l'unit systemd : `--headless`, workspace en argument, redémarrage
/// sur échec. `%` est doublé (spécificateur systemd) et les chemins sont cités.
pub fn unit_text(agent: &str, exe: &Path, ws: &Path) -> String {
    let description = format!("Agent swarm-control '{agent}'").replace('%', "%%");
    let exe = quoted(exe);
    let ws = quoted(ws);
    format!(
        "[Unit]\nDescription={description}\n\n\
         [Service]\nType=simple\nExecStart={exe} --headless --workspace {ws}\n\
         Restart=on-failure\nRestartSec=5\n\n\
         [Install]\nWantedBy=default.target\n"
    )
}

/// Écrit `<ws>/bridge/units/swarm-<agent>.service` (sans l'activer). Le fichier
/// peut être préparé ici (Windows) puis copié sur l'hôte Linux : d'où l'écriture
/// inconditionnelle, l'activation restant à la charge de l'administrateur.
pub fn write_unit(ws: &Path, agent: &str, exe: &Path) -> Result<PathBuf, String> {
    let dir = ws.join("bridge").join("units");
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Dossier '{}' impossible à créer : {e}", dir.display()))?;
    let path = service_path(ws, agent);
    std::fs::write(&path, unit_text(agent, exe, ws))
        .map_err(|e| format!("Écriture de '{}' impossible : {e}", path.display()))?;
    Ok(path)
}

/// Commande à lancer en root pour installer et activer l'unit (chemins génériques).
pub fn install_hint(agent: &str) -> String {
    format!(
        "sudo cp <ws>/bridge/units/swarm-{agent}.service /etc/systemd/system/ && \
         sudo systemctl daemon-reload && sudo systemctl enable --now swarm-{agent}.service"
    )
}

/// Chemin entre guillemets doubles, `%` doublé pour systemd.
fn quoted(path: &Path) -> String {
    format!("\"{}\"", path.display().to_string().replace('%', "%%"))
}

#[cfg(test)]
mod tests {
    use super::{install_hint, path, remove, service_path, unit_text, write_unit};
    use crate::agents::compile::build_dir;
    use std::path::{Path, PathBuf};

    fn temp_ws(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("swarm-exe-{tag}-{}", std::process::id()))
    }

    #[test]
    fn path_follows_the_platform_suffix() {
        let ws = Path::new("ws");
        let expected = ws.join("bin").join(format!("alice{}", std::env::consts::EXE_SUFFIX));
        assert_eq!(path(ws, "alice"), expected);
        #[cfg(windows)]
        assert_eq!(path(ws, "alice").file_name().unwrap(), "alice.exe");
        #[cfg(unix)]
        assert_eq!(path(ws, "alice").file_name().unwrap(), "alice");
    }

    #[test]
    fn unit_text_names_the_agent_and_escapes_percent() {
        let ws = Path::new("/srv/ws 100%");
        let exe = path(ws, "alice");
        let text = unit_text("alice", &exe, ws);
        assert!(text.contains("Description=Agent swarm-control 'alice'"), "{text}");
        assert!(text.contains("Type=simple"), "{text}");
        assert!(text.contains("--headless"), "{text}");
        assert!(text.contains("--workspace"), "{text}");
        assert!(text.contains("Restart=on-failure"), "{text}");
        assert!(text.contains("RestartSec=5"), "{text}");
        assert!(text.contains("WantedBy=default.target"), "{text}");
        // Le `%` du workspace est doublé, et les deux chemins sont cités.
        assert!(text.contains("--workspace \"/srv/ws 100%%\""), "{text}");
        assert!(!text.contains("\"/srv/ws 100%\""), "{text}");
        let quoted_exe = format!("\"{}\"", exe.display().to_string().replace('%', "%%"));
        assert!(text.contains(&quoted_exe), "{text}");
    }

    #[test]
    fn remove_takes_the_binary_and_the_build_dir_then_nothing() {
        let ws = temp_ws("remove");
        let _ = std::fs::remove_dir_all(&ws);
        let agent = format!("gone-{}", std::process::id());
        let dest = path(&ws, &agent);
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::fs::write(&dest, b"binaire").unwrap();
        let build = build_dir(&ws, &agent).join("src");
        std::fs::create_dir_all(&build).unwrap();
        std::fs::write(build.join("main.rs"), b"fn main() {}").unwrap();
        assert_eq!(remove(&ws, &agent), Some(dest.clone()));
        assert!(!dest.exists());
        assert!(!build_dir(&ws, &agent).exists(), "le dossier de compilation part avec le binaire");
        assert_eq!(remove(&ws, &agent), None);
        // Le profil de l'agent lui-même reste : seul `build/` part.
        std::fs::create_dir_all(ws.join("agents").join(&agent).join("profil")).unwrap();
        assert_eq!(remove(&ws, &agent), None);
        assert!(ws.join("agents").join(&agent).join("profil").is_dir());
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn write_unit_lands_under_bridge_units() {
        let ws = temp_ws("unit");
        let _ = std::fs::remove_dir_all(&ws);
        let exe = path(&ws, "alice");
        let unit = write_unit(&ws, "alice", &exe).unwrap();
        assert_eq!(unit, service_path(&ws, "alice"));
        assert!(unit.ends_with(Path::new("bridge/units/swarm-alice.service")));
        assert!(std::fs::read_to_string(&unit).unwrap().contains("--headless"));
        assert!(install_hint("alice").contains("systemctl enable --now swarm-alice.service"));
        let _ = std::fs::remove_dir_all(&ws);
    }
}
