//! Binaire Antigravity CLI (`agy`).

use super::path_lookup::Lookup;
use std::path::PathBuf;

/// `agy` : `AGY_BIN`, `ANTIGRAVITY_BIN`, dossier d'installation officiel, puis le PATH.
pub fn antigravity_binary() -> PathBuf {
    antigravity_binary_in(&Lookup::from_env())
}

/// [`antigravity_binary`] sur un environnement injecté (les tests ne posent pas de variable).
///
/// Ordre : `AGY_BIN` non vide → `ANTIGRAVITY_BIN` non vide → l'exécutable de l'installeur
/// officiel (`%LOCALAPPDATA%\agy\bin\agy.exe` sous Windows, `~/.local/bin/agy` ailleurs,
/// seulement s'il existe) → dans le PATH `agy.exe`, `agy.cmd`, `agy` (Windows) ou `agy`
/// (ailleurs) → le nom nu `agy`, résolu par le shell au lancement.
pub fn antigravity_binary_in(env: &Lookup<'_>) -> PathBuf {
    for var in ["AGY_BIN", "ANTIGRAVITY_BIN"] {
        if let Some(bin) = env.non_empty_var(var) {
            return PathBuf::from(bin);
        }
    }
    if let Some(installed) = default_install_exe(env).filter(|p| p.is_file()) {
        return installed;
    }
    let names: &[&str] = if env.windows { &["agy.exe", "agy.cmd", "agy"] } else { &["agy"] };
    env.on_path(names).unwrap_or_else(|| PathBuf::from("agy"))
}

/// Emplacement posé par l'installeur officiel Antigravity.
fn default_install_exe(env: &Lookup<'_>) -> Option<PathBuf> {
    if env.windows {
        let local = env
            .localappdata
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(|| env.home.join("AppData").join("Local"));
        Some(local.join("agy").join("bin").join("agy.exe"))
    } else {
        Some(env.home.join(".local").join("bin").join("agy"))
    }
}

#[cfg(test)]
mod tests {
    use super::{antigravity_binary_in, default_install_exe};
    use crate::binaries::path_lookup::Lookup;
    use crate::binaries::path_lookup::tests::{fake_exe, path_of, temp_dir};
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};

    fn env_with<'a>(
        home: &'a Path,
        vars: &'a dyn Fn(&str) -> Option<String>,
        localappdata: Option<OsString>,
        path: Option<OsString>,
        windows: bool,
    ) -> Lookup<'a> {
        Lookup { var: vars, path, appdata: None, localappdata, home: home.to_path_buf(), windows }
    }

    #[test]
    fn agy_bin_wins_over_antigravity_bin_and_both_beat_the_disk() {
        let root = temp_dir("agy-bin");
        let installed = default_install_exe(&env_with(&root, &|_| None, None, None, true)).unwrap();
        fake_exe(installed.parent().unwrap(), "agy.exe");
        fake_exe(&root, "agy.exe");
        let both = |name: &str| match name {
            "AGY_BIN" => Some("A".to_string()),
            "ANTIGRAVITY_BIN" => Some("B".to_string()),
            _ => None,
        };
        assert_eq!(
            antigravity_binary_in(&env_with(&root, &both, None, Some(path_of(&[&root])), true)),
            PathBuf::from("A")
        );
        // `AGY_BIN` vide ou absent : `ANTIGRAVITY_BIN` prend le relais.
        let only_legacy = |name: &str| {
            (name == "AGY_BIN")
                .then(String::new)
                .or_else(|| (name == "ANTIGRAVITY_BIN").then(|| "B".to_string()))
        };
        assert_eq!(
            antigravity_binary_in(&env_with(
                &root,
                &only_legacy,
                None,
                Some(path_of(&[&root])),
                true
            )),
            PathBuf::from("B")
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn on_windows_the_official_installer_location_wins_over_the_path() {
        let root = temp_dir("agy-installer");
        let local = root.join("local");
        let installed = fake_exe(&local.join("agy").join("bin"), "agy.exe");
        fake_exe(&root, "agy.exe");
        let env =
            env_with(&root, &|_| None, Some(local.into_os_string()), Some(path_of(&[&root])), true);
        assert_eq!(antigravity_binary_in(&env), installed);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn on_windows_the_path_tries_exe_then_cmd_then_the_bare_name() {
        let root = temp_dir("agy-extensions");
        // Sans `%LOCALAPPDATA%` : le repli est `<home>\AppData\Local`, vide ici.
        let env = env_with(&root, &|_| None, None, Some(path_of(&[&root])), true);
        assert_eq!(antigravity_binary_in(&env), PathBuf::from("agy"), "rien dans le PATH");

        let bare = fake_exe(&root, "agy");
        assert_eq!(antigravity_binary_in(&env), bare, "le nom nu suffit");

        let cmd = fake_exe(&root, "agy.cmd");
        assert_eq!(antigravity_binary_in(&env), cmd, "`.cmd` prime le nom nu");

        let exe = fake_exe(&root, "agy.exe");
        assert_eq!(antigravity_binary_in(&env), exe, "`.exe` prime `.cmd`");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn elsewhere_only_the_bare_name_is_looked_up_in_the_path() {
        let root = temp_dir("agy-unix");
        let _exe = fake_exe(&root, "agy.exe");
        let _cmd = fake_exe(&root, "agy.cmd");
        // Rien d'installé sous `~/.local/bin` (home = root, pas de `.local`).
        let env = env_with(&root, &|_| None, None, Some(path_of(&[&root])), false);
        assert_eq!(antigravity_binary_in(&env), PathBuf::from("agy"));
        let bare = fake_exe(&root, "agy");
        assert_eq!(antigravity_binary_in(&env), bare);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn elsewhere_the_official_installer_location_is_dot_local_bin() {
        let root = temp_dir("agy-dotlocal");
        let installed = fake_exe(&root.join(".local").join("bin"), "agy");
        fake_exe(&root, "agy");
        let env = env_with(&root, &|_| None, None, Some(path_of(&[&root])), false);
        assert_eq!(antigravity_binary_in(&env), installed);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_installer_path_that_is_a_directory_is_ignored() {
        let root = temp_dir("agy-dossier");
        std::fs::create_dir_all(root.join(".local").join("bin").join("agy")).unwrap();
        let on_path = fake_exe(&root, "agy");
        let env = env_with(&root, &|_| None, None, Some(path_of(&[&root])), false);
        // `is_file()` refuse un dossier : on tombe sur le PATH.
        assert_eq!(antigravity_binary_in(&env), on_path);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn nothing_anywhere_or_no_path_gives_the_bare_name() {
        let root = temp_dir("agy-rien");
        for windows in [true, false] {
            assert_eq!(
                antigravity_binary_in(&env_with(
                    &root,
                    &|_| None,
                    None,
                    Some(path_of(&[&root])),
                    windows
                )),
                PathBuf::from("agy")
            );
            assert_eq!(
                antigravity_binary_in(&env_with(&root, &|_| None, None, None, windows)),
                PathBuf::from("agy")
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn default_install_exe_follows_localappdata_then_falls_back_to_home() {
        let root = temp_dir("agy-chemin");
        let with_local = env_with(
            &root,
            &|_| None,
            Some(PathBuf::from("D:/Local").into_os_string()),
            None,
            true,
        );
        assert_eq!(
            default_install_exe(&with_local),
            Some(PathBuf::from("D:/Local/agy/bin/agy.exe"))
        );
        let without = env_with(&root, &|_| None, None, None, true);
        assert_eq!(
            default_install_exe(&without),
            Some(root.join("AppData").join("Local").join("agy").join("bin").join("agy.exe"))
        );
        let unix = env_with(&root, &|_| None, None, None, false);
        assert_eq!(default_install_exe(&unix), Some(root.join(".local").join("bin").join("agy")));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_real_resolver_agrees_with_the_injected_one_on_the_current_environment() {
        assert_eq!(super::antigravity_binary(), antigravity_binary_in(&Lookup::from_env()));
    }
}
