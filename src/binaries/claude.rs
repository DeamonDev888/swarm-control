//! Binaire Claude Code.

use super::path_lookup::Lookup;
use std::path::PathBuf;

/// `claude` : `CLAUDE_BIN`, puis l'exécutable npm global (Windows), puis le PATH.
pub fn claude_binary() -> PathBuf {
    claude_binary_in(&Lookup::from_env())
}

/// [`claude_binary`] sur un environnement injecté (les tests ne posent pas de variable).
///
/// Ordre : `CLAUDE_BIN` non vide → `%APPDATA%\npm\node_modules\@anthropic-ai\claude-code\bin\claude.exe`
/// (Windows, seulement si le fichier existe) → `claude.exe` dans le PATH (Windows) ou
/// `claude` (ailleurs) → le nom nu `claude`, résolu par le shell au lancement.
pub fn claude_binary_in(env: &Lookup<'_>) -> PathBuf {
    if let Some(bin) = env.non_empty_var("CLAUDE_BIN") {
        return PathBuf::from(bin);
    }
    if env.windows {
        if let Some(npm) = npm_global_exe(env).filter(|p| p.is_file()) {
            return npm;
        }
        if let Some(p) = env.on_path(&["claude.exe"]) {
            return p;
        }
    } else if let Some(p) = env.on_path(&["claude"]) {
        return p;
    }
    PathBuf::from("claude")
}

/// L'exécutable posé par `npm install -g @anthropic-ai/claude-code` sous Windows.
fn npm_global_exe(env: &Lookup<'_>) -> Option<PathBuf> {
    let appdata = env
        .appdata
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| env.home.join("AppData").join("Roaming"));
    Some(
        appdata
            .join("npm")
            .join("node_modules")
            .join("@anthropic-ai")
            .join("claude-code")
            .join("bin")
            .join("claude.exe"),
    )
}

#[cfg(test)]
mod tests {
    use super::{claude_binary_in, npm_global_exe};
    use crate::binaries::path_lookup::Lookup;
    use crate::binaries::path_lookup::tests::{fake_exe, path_of, temp_dir};
    use std::ffi::OsString;
    use std::path::PathBuf;

    fn lookup_with<'a>(
        home: &'a std::path::Path,
        vars: &'a dyn Fn(&str) -> Option<String>,
        appdata: Option<OsString>,
        path: Option<OsString>,
        windows: bool,
    ) -> Lookup<'a> {
        Lookup { var: vars, path, appdata, localappdata: None, home: home.to_path_buf(), windows }
    }

    #[test]
    fn claude_bin_wins_over_everything_and_is_taken_verbatim() {
        let root = temp_dir("claude-bin");
        fake_exe(&root, "claude.exe");
        let appdata = root.join("roaming");
        fake_exe(
            &appdata
                .join("npm")
                .join("node_modules")
                .join("@anthropic-ai")
                .join("claude-code")
                .join("bin"),
            "claude.exe",
        );
        let vars =
            |name: &str| (name == "CLAUDE_BIN").then(|| "/opt/ma-version/claude".to_string());
        let env = lookup_with(
            &root,
            &vars,
            Some(appdata.into_os_string()),
            Some(path_of(&[&root])),
            true,
        );
        // Ni normalisé, ni testé sur disque : la variable fait foi.
        assert_eq!(claude_binary_in(&env), PathBuf::from("/opt/ma-version/claude"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_empty_claude_bin_falls_through_to_the_search() {
        let root = temp_dir("claude-vide");
        let exe = fake_exe(&root, "claude.exe");
        let vars = |name: &str| (name == "CLAUDE_BIN").then(String::new);
        let env = lookup_with(&root, &vars, None, Some(path_of(&[&root])), true);
        assert_eq!(claude_binary_in(&env), exe);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn on_windows_the_npm_global_executable_wins_over_the_path() {
        let root = temp_dir("claude-npm");
        fake_exe(&root, "claude.exe");
        let appdata = root.join("roaming");
        let npm = fake_exe(
            &appdata
                .join("npm")
                .join("node_modules")
                .join("@anthropic-ai")
                .join("claude-code")
                .join("bin"),
            "claude.exe",
        );
        let env = lookup_with(
            &root,
            &|_| None,
            Some(appdata.into_os_string()),
            Some(path_of(&[&root])),
            true,
        );
        assert_eq!(claude_binary_in(&env), npm);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn on_windows_a_missing_npm_executable_falls_back_to_the_path_with_exe_only() {
        let root = temp_dir("claude-path");
        let exe = fake_exe(&root, "claude.exe");
        let _cmd = fake_exe(&root, "claude.cmd");
        // `%APPDATA%` pointe sur un dossier sans l'exécutable npm.
        let appdata = root.join("roaming-vide");
        std::fs::create_dir_all(&appdata).unwrap();
        let env = lookup_with(
            &root,
            &|_| None,
            Some(appdata.into_os_string()),
            Some(path_of(&[&root])),
            true,
        );
        // Sous Windows, `claude` ne cherche que `.exe` : un wrapper `.cmd` seul est ignoré.
        assert_eq!(claude_binary_in(&env), exe);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn on_windows_a_cmd_wrapper_alone_is_not_picked_up() {
        let root = temp_dir("claude-cmd");
        let _cmd = fake_exe(&root, "claude.cmd");
        let env = lookup_with(&root, &|_| None, None, Some(path_of(&[&root])), true);
        assert_eq!(claude_binary_in(&env), PathBuf::from("claude"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn elsewhere_the_bare_name_is_looked_up_and_the_npm_folder_is_ignored() {
        let root = temp_dir("claude-unix");
        let bare = fake_exe(&root, "claude");
        let _exe = fake_exe(&root, "claude.exe");
        let appdata = root.join("roaming");
        let npm = appdata
            .join("npm")
            .join("node_modules")
            .join("@anthropic-ai")
            .join("claude-code")
            .join("bin");
        fake_exe(&npm, "claude.exe");
        // Même avec un `%APPDATA%` fourni : hors Windows, le dossier npm n'est pas consulté.
        let env = lookup_with(
            &root,
            &|_| None,
            Some(appdata.into_os_string()),
            Some(path_of(&[&root])),
            false,
        );
        assert_eq!(claude_binary_in(&env), bare);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn nothing_anywhere_gives_the_bare_name_for_the_shell_to_resolve() {
        let root = temp_dir("claude-nom");
        for windows in [true, false] {
            let env = lookup_with(&root, &|_| None, None, Some(path_of(&[&root])), windows);
            assert_eq!(claude_binary_in(&env), PathBuf::from("claude"));
            // Même sans PATH du tout.
            let env = lookup_with(&root, &|_| None, None, None, windows);
            assert_eq!(claude_binary_in(&env), PathBuf::from("claude"));
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn npm_global_exe_follows_appdata_then_falls_back_to_home() {
        let root = temp_dir("claude-npm-chemin");
        let vars = |_: &str| None;
        let with_appdata =
            lookup_with(&root, &vars, Some(PathBuf::from("D:/App").into_os_string()), None, true);
        assert_eq!(
            npm_global_exe(&with_appdata),
            Some(PathBuf::from("D:/App/npm/node_modules/@anthropic-ai/claude-code/bin/claude.exe"))
        );
        // Sans `%APPDATA%` : `<home>\AppData\Roaming\npm\…`.
        let without = lookup_with(&root, &vars, None, None, true);
        assert_eq!(
            npm_global_exe(&without),
            Some(
                root.join("AppData")
                    .join("Roaming")
                    .join("npm")
                    .join("node_modules")
                    .join("@anthropic-ai")
                    .join("claude-code")
                    .join("bin")
                    .join("claude.exe")
            )
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_real_resolver_agrees_with_the_injected_one_on_the_current_environment() {
        // Sans poser de variable : la porte publique et la forme injectée lisent le même
        // environnement, elles doivent rendre le même chemin.
        let a = super::claude_binary();
        let b = claude_binary_in(&Lookup::from_env());
        assert_eq!(a, b);
    }
}
