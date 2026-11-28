//! Binaire Hermes.

use super::path_lookup::Lookup;
use std::path::PathBuf;

/// `hermes` : `HERMES_BIN`, puis le PATH (en évitant les wrappers `.cmd`).
pub fn hermes_binary() -> PathBuf {
    hermes_binary_in(&Lookup::from_env())
}

/// [`hermes_binary`] sur un environnement injecté (les tests ne posent pas de variable).
///
/// Ordre : `HERMES_BIN` non vide → `hermes.exe` dans le PATH (Windows) ou `hermes` (ailleurs)
/// → le nom nu `hermes`, résolu par le shell au lancement. Jamais `.cmd` : un wrapper npm
/// ferait perdre la sortie et le code de sortie du vrai programme.
pub fn hermes_binary_in(env: &Lookup<'_>) -> PathBuf {
    if let Some(bin) = env.non_empty_var("HERMES_BIN") {
        return PathBuf::from(bin);
    }
    let names: &[&str] = if env.windows { &["hermes.exe"] } else { &["hermes"] };
    env.on_path(names).unwrap_or_else(|| PathBuf::from("hermes"))
}

#[cfg(test)]
mod tests {
    use super::hermes_binary_in;
    use crate::binaries::path_lookup::Lookup;
    use crate::binaries::path_lookup::tests::{fake_exe, path_of, temp_dir};
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};

    fn env_with<'a>(
        home: &'a Path,
        vars: &'a dyn Fn(&str) -> Option<String>,
        path: Option<OsString>,
        windows: bool,
    ) -> Lookup<'a> {
        Lookup {
            var: vars,
            path,
            appdata: None,
            localappdata: None,
            home: home.to_path_buf(),
            windows,
        }
    }

    #[test]
    fn hermes_bin_wins_and_is_taken_verbatim() {
        let root = temp_dir("hermes-bin");
        fake_exe(&root, "hermes.exe");
        let vars =
            |name: &str| (name == "HERMES_BIN").then(|| "C:\\outils\\hermes.exe".to_string());
        let env = env_with(&root, &vars, Some(path_of(&[&root])), true);
        assert_eq!(hermes_binary_in(&env), PathBuf::from("C:\\outils\\hermes.exe"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_empty_hermes_bin_falls_through_to_the_path() {
        let root = temp_dir("hermes-vide");
        let exe = fake_exe(&root, "hermes.exe");
        let vars = |name: &str| (name == "HERMES_BIN").then(String::new);
        let env = env_with(&root, &vars, Some(path_of(&[&root])), true);
        assert_eq!(hermes_binary_in(&env), exe);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn on_windows_only_the_exe_is_looked_up_never_the_cmd_wrapper() {
        let root = temp_dir("hermes-cmd");
        let _cmd = fake_exe(&root, "hermes.cmd");
        let _bare = fake_exe(&root, "hermes");
        // Ni `.cmd` ni `hermes` nu ne correspondent sous Windows.
        let env = env_with(&root, &|_| None, Some(path_of(&[&root])), true);
        assert_eq!(hermes_binary_in(&env), PathBuf::from("hermes"));
        // Avec le `.exe` à côté, c'est lui.
        let exe = fake_exe(&root, "hermes.exe");
        let env = env_with(&root, &|_| None, Some(path_of(&[&root])), true);
        assert_eq!(hermes_binary_in(&env), exe);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn elsewhere_the_bare_name_is_looked_up() {
        let root = temp_dir("hermes-unix");
        let bare = fake_exe(&root, "hermes");
        fake_exe(&root, "hermes.exe");
        let env = env_with(&root, &|_| None, Some(path_of(&[&root])), false);
        assert_eq!(hermes_binary_in(&env), bare);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn nothing_anywhere_or_no_path_gives_the_bare_name() {
        let root = temp_dir("hermes-rien");
        for windows in [true, false] {
            assert_eq!(
                hermes_binary_in(&env_with(&root, &|_| None, Some(path_of(&[&root])), windows)),
                PathBuf::from("hermes")
            );
            assert_eq!(
                hermes_binary_in(&env_with(&root, &|_| None, None, windows)),
                PathBuf::from("hermes")
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_real_resolver_agrees_with_the_injected_one_on_the_current_environment() {
        assert_eq!(super::hermes_binary(), hermes_binary_in(&Lookup::from_env()));
    }
}
