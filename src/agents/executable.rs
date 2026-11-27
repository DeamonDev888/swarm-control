//! Exécutable dédié à un agent, et unit systemd pour le lancer sans terminal.
//!
//! - binaire : `<ws>/bin/<agent><EXE_SUFFIX>`, copie de l'exécutable courant ;
//! - unit : `<ws>/bridge/units/swarm-<agent>.service`, écrite mais **jamais**
//!   installée ni activée (pas de `/etc`, pas de root).
//!
//! Politique de partage : chaque agent reçoit **sa** copie, nommée d'après
//! lui — un fichier `bin/<agent>` n'est donc référencé que par l'agent qui
//! porte son nom, et `remove` n'a pas de référence croisée à vérifier.
//! Plusieurs agents « partagent un exécutable » au sens où leurs copies
//! proviennent du même binaire, jamais au sens fichier.

use std::path::{Path, PathBuf};

/// Chemin de l'exécutable de l'agent : `<ws>/bin/<agent><EXE_SUFFIX>`.
pub fn path(ws: &Path, agent: &str) -> PathBuf {
    ws.join("bin").join(format!("{agent}{}", std::env::consts::EXE_SUFFIX))
}

/// Chemin de l'unit systemd de l'agent : `<ws>/bridge/units/swarm-<agent>.service`.
pub fn service_path(ws: &Path, agent: &str) -> PathBuf {
    ws.join("bridge").join("units").join(format!("swarm-{agent}.service"))
}

/// Copie l'exécutable courant dans `<ws>/bin/` (écrase), et le rend exécutable
/// sous Unix. Rend le chemin installé.
pub fn install(ws: &Path, agent: &str) -> Result<PathBuf, String> {
    let dest = path(ws, agent);
    let dir = ws.join("bin");
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Dossier '{}' impossible à créer : {e}", dir.display()))?;
    let source =
        std::env::current_exe().map_err(|e| format!("Exécutable courant introuvable : {e}"))?;
    std::fs::copy(&source, &dest).map_err(|e| {
        format!("Copie de '{}' vers '{}' impossible : {e}", source.display(), dest.display())
    })?;
    #[cfg(unix)]
    set_executable(&dest)?;
    Ok(dest)
}

/// Supprime la copie dédiée de l'agent (une copie par agent : aucune autre
/// référence possible, voir le doc du module) s'il existe, et rend son chemin.
pub fn remove(ws: &Path, agent: &str) -> Option<PathBuf> {
    let path = path(ws, agent);
    std::fs::remove_file(&path).ok().map(|()| path)
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

/// Sous Unix, l'exécutable doit être `0o755` (la copie perd le bit x).
#[cfg(unix)]
fn set_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)
        .map_err(|e| format!("Permissions de '{}' illisibles : {e}", path.display()))?
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms)
        .map_err(|e| format!("Permissions de '{}' impossibles à régler : {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::{install, install_hint, path, remove, service_path, unit_text, write_unit};
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
    fn install_copies_an_executable_file() {
        let ws = temp_ws("install");
        let _ = std::fs::remove_dir_all(&ws);
        let agent = format!("agent-{}", std::process::id());
        let dest = install(&ws, &agent).unwrap();
        assert_eq!(dest, path(&ws, &agent));
        assert!(dest.is_file());
        let source = std::env::current_exe().unwrap();
        assert_eq!(
            std::fs::metadata(&dest).unwrap().len(),
            std::fs::metadata(&source).unwrap().len()
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&dest).unwrap().permissions().mode() & 0o777, 0o755);
        }
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn remove_returns_the_path_then_none() {
        let ws = temp_ws("remove");
        let _ = std::fs::remove_dir_all(&ws);
        let agent = format!("gone-{}", std::process::id());
        let dest = install(&ws, &agent).unwrap();
        assert_eq!(remove(&ws, &agent), Some(dest.clone()));
        assert!(!dest.exists());
        assert_eq!(remove(&ws, &agent), None);
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
