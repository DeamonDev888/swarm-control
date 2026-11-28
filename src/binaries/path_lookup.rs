//! Recherche d'un exécutable dans le `PATH`.

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

/// Premier fichier nommé `names[i]` trouvé dans un dossier du `PATH` déjà lu : les
/// résolveurs passent par [`Lookup`] (`from_env`), les tests injectent le `PATH` sans
/// toucher à l'environnement.
///
/// Parcours dans l'ordre du `PATH` (premier dossier gagne) et, dans un dossier, dans l'ordre
/// de `names` (la première extension gagne). Un dossier sans aucun des noms est sauté ; un
/// élément du `PATH` vide (chemin relatif courant) est résolu par `split_paths` comme `.`.
pub fn on_path_in(path: &OsStr, names: &[&str]) -> Option<PathBuf> {
    for dir in std::env::split_paths(path) {
        for name in names {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Ce qu'un résolveur (`claude`, `hermes`, `agy`) lit de son environnement : injecté par les
/// tests, lu dans le process par [`Lookup::from_env`].
pub struct Lookup<'a> {
    /// Valeur d'une variable (`CLAUDE_BIN`, `HERMES_BIN`…) : env système puis `.env`.
    pub var: &'a dyn Fn(&str) -> Option<String>,
    /// Le `PATH`, sous la forme native de la plateforme.
    pub path: Option<OsString>,
    /// `%APPDATA%` (Windows) : où npm pose ses exécutables globaux.
    pub appdata: Option<OsString>,
    /// `%LOCALAPPDATA%` (Windows) : où l'installeur Antigravity pose `agy.exe`.
    pub localappdata: Option<OsString>,
    /// Dossier home de l'utilisateur (repli quand `%APPDATA%` / `%LOCALAPPDATA%` manquent).
    pub home: PathBuf,
    /// Sémantique Windows (extensions, emplacements d'installation) — pas `cfg!(windows)`
    /// seul, pour que les deux branches soient testables partout.
    pub windows: bool,
}

impl Lookup<'_> {
    /// Le `Lookup` du process courant : variables par [`crate::config::var`], dossiers par
    /// l'environnement système, sémantique de la plateforme de compilation.
    pub fn from_env() -> Lookup<'static> {
        Lookup {
            var: &crate::config::var,
            path: std::env::var_os("PATH"),
            appdata: std::env::var_os("APPDATA"),
            localappdata: std::env::var_os("LOCALAPPDATA"),
            home: crate::config::home_dir(),
            windows: cfg!(windows),
        }
    }

    /// Comme [`on_path`], sur le `PATH` de ce `Lookup`.
    pub fn on_path(&self, names: &[&str]) -> Option<PathBuf> {
        self.path.as_deref().and_then(|p| on_path_in(p, names))
    }

    /// Valeur de `name`, vide → `None` (une variable posée à vide compte pour absente).
    pub fn non_empty_var(&self, name: &str) -> Option<String> {
        (self.var)(name).filter(|s| !s.is_empty())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::{Lookup, on_path_in};
    use std::ffi::{OsStr, OsString};
    use std::path::{Path, PathBuf};

    pub(crate) fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("swarm-binaries-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Crée un fichier vide (et ses dossiers parents) : un faux exécutable suffit, seul
    /// `is_file()` est testé.
    pub(crate) fn fake_exe(dir: &Path, name: &str) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let file = dir.join(name);
        std::fs::write(&file, b"").unwrap();
        file
    }

    /// Construit un `PATH` à partir de dossiers, avec le séparateur de liste de la plateforme
    /// (`;` sous Windows, `:` ailleurs — pas le séparateur de composants).
    pub(crate) fn path_of(dirs: &[&Path]) -> OsString {
        std::env::join_paths(dirs.iter().copied()).expect("dossier du PATH sans séparateur")
    }

    /// Un `Lookup` sans rien dans l'environnement, `PATH` et home donnés.
    pub(crate) fn lookup(home: &Path, path: Option<OsString>, windows: bool) -> Lookup<'static> {
        Lookup {
            var: &|_| None,
            path,
            appdata: None,
            localappdata: None,
            home: home.to_path_buf(),
            windows,
        }
    }

    #[test]
    fn finds_the_first_name_in_the_first_directory_of_the_path() {
        let root = temp_dir("ordre");
        let (a, b) = (root.join("a"), root.join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let exe_b = fake_exe(&b, "claude.exe");
        let cmd_a = fake_exe(&a, "claude.cmd");
        // `a` précède `b` dans le PATH : c'est `a` qui gagne, avec l'extension demandée.
        let found = on_path_in(&path_of(&[&a, &b]), &["claude.exe", "claude.cmd"]);
        assert_eq!(found, Some(cmd_a));
        // Inversement, `b` d'abord : `claude.exe` y est trouvé.
        assert_eq!(on_path_in(&path_of(&[&b, &a]), &["claude.exe", "claude.cmd"]), Some(exe_b));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn within_a_directory_the_first_matching_name_wins() {
        let dir = temp_dir("extensions");
        let exe = fake_exe(&dir, "agy.exe");
        let cmd = fake_exe(&dir, "agy.cmd");
        // Ordre déclaré `.exe` puis `.cmd` : `.exe` gagne même si `.cmd` existe aussi.
        assert_eq!(on_path_in(&path_of(&[&dir]), &["agy.exe", "agy.cmd", "agy"]), Some(exe));
        // Ordre inverse : `.cmd` gagne.
        assert_eq!(on_path_in(&path_of(&[&dir]), &["agy.cmd", "agy.exe"]), Some(cmd.clone()));
        // Seul `.cmd` demandé : rendu tel quel, pas de complément d'extension.
        assert_eq!(on_path_in(&path_of(&[&dir]), &["agy.cmd"]), Some(cmd));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn directories_without_a_candidate_are_skipped() {
        let root = temp_dir("sautes");
        let (empty, missing, good) = (root.join("vide"), root.join("absent"), root.join("bon"));
        std::fs::create_dir_all(&empty).unwrap();
        std::fs::create_dir_all(&good).unwrap();
        let exe = fake_exe(&good, "hermes.exe");
        // Dossier vide puis dossier inexistant : ni l'un ni l'autre n'arrêtent la recherche.
        assert_eq!(on_path_in(&path_of(&[&empty, &missing, &good]), &["hermes.exe"]), Some(exe));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_directory_named_like_the_binary_is_not_a_match() {
        let root = temp_dir("dossier");
        let dir = root.join("outils");
        std::fs::create_dir_all(dir.join("claude.exe")).unwrap();
        // Un **dossier** `claude.exe` n'est pas un exécutable : `is_file()` le refuse.
        assert_eq!(on_path_in(&path_of(&[&dir]), &["claude.exe"]), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn nothing_is_found_without_a_candidate_or_with_an_empty_path() {
        let root = temp_dir("rien");
        assert_eq!(on_path_in(&path_of(&[&root]), &["claude", "claude.exe"]), None);
        assert_eq!(on_path_in(OsStr::new(""), &["claude"]), None);
        assert_eq!(on_path_in(&path_of(&[&root]), &[]), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_empty_path_entry_resolves_to_the_current_directory() {
        // `split_paths` rend un élément vide comme `""` : `Path::new("").join("x")` = `x`,
        // donc relatif au répertoire courant. Contrat observé, documenté ici.
        let entries: Vec<PathBuf> = std::env::split_paths(OsStr::new("")).collect();
        assert_eq!(entries, [PathBuf::from("")]);
        assert_eq!(Path::new("").join("x"), PathBuf::from("x"));
    }

    #[test]
    fn a_lookup_without_path_finds_nothing_and_one_with_path_uses_it() {
        let root = temp_dir("lookup");
        let exe = fake_exe(&root, "claude.exe");
        let none = lookup(&root, None, true);
        assert_eq!(none.on_path(&["claude.exe"]), None);
        let some = lookup(&root, Some(path_of(&[&root])), true);
        assert_eq!(some.on_path(&["claude.exe"]), Some(exe));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_empty_variable_counts_as_absent() {
        let root = temp_dir("vide");
        let vars = |name: &str| {
            (name == "PLEIN")
                .then(|| "x".to_string())
                .or_else(|| (name == "VIDE").then(String::new))
        };
        let l = Lookup {
            var: &vars,
            path: None,
            appdata: None,
            localappdata: None,
            home: root.clone(),
            windows: false,
        };
        assert_eq!(l.non_empty_var("PLEIN").as_deref(), Some("x"));
        assert_eq!(l.non_empty_var("VIDE"), None, "une variable vide = absente");
        assert_eq!(l.non_empty_var("ABSENT"), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn des_entrees_vides_du_path_ne_stoppent_pas_la_recherche() {
        let root = temp_dir("entrees-vides");
        let exe = fake_exe(&root, "claude.exe");
        let vide = root.join("vide");
        std::fs::create_dir_all(&vide).unwrap();
        // Élément vide en tête et entre deux dossiers (résolu au dossier courant, sans
        // candidat) : la recherche continue jusqu'au dossier qui porte l'exécutable.
        let path = path_of(&[Path::new(""), &vide, Path::new(""), &root]);
        assert_eq!(on_path_in(&path, &["claude.exe"]), Some(exe));
        let _ = std::fs::remove_dir_all(&root);
    }
}
