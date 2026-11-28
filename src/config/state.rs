//! État global de configuration, initialisé une fois au démarrage.
//!
//! Deux familles de variables (PLAN §4.8) :
//! - **démarrage** : les `.env` des dossiers d'état (`~/.swarm-control/.env`). Ils fixent le
//!   workspace et `EXTERNAL_ENV_PATH` : immuables, lus une seule fois ;
//! - **chaudes** : le `.env` du workspace et le fichier `EXTERNAL_ENV_PATH`. Ils sont relus
//!   quand leur date de modification change ([`reload_env`]) : au début de chaque tour d'un
//!   agent résident, par la commande `reload`, par l'outil MCP du même nom.
//!
//! Dans les deux cas l'environnement système garde la priorité : une variable posée par l'env
//! du process n'entre jamais dans la table (voir `dotenv::load_into` et `env::var`).

use super::{dotenv, env, paths, workspace};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, RwLock};
use std::time::SystemTime;

pub struct Config {
    /// Racine : `.claude/`, `.mcp.json`, `bridge/`.
    pub workspace: PathBuf,
    /// Variables de démarrage (`.env` des dossiers d'état) : la base de chaque relecture.
    boot: HashMap<String, String>,
    /// Table effective (démarrage + fichiers chauds), absente de l'env système.
    live: RwLock<HashMap<String, String>>,
    /// Fichiers chauds au dernier chargement.
    hot: Mutex<Hot>,
}

/// Un fichier tel que vu au dernier chargement : date de modification et taille (`modified`
/// à `None` : absent). La taille complète la date : deux écritures dans le même tick de
/// l'horloge système portent la même date.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct Stamp {
    modified: Option<SystemTime>,
    len: u64,
}

/// Le fichier `EXTERNAL_ENV_PATH` : chemin résolu au dernier chargement, et son état.
#[derive(Clone, PartialEq, Eq, Debug)]
struct External {
    path: PathBuf,
    stamp: Stamp,
}

/// Les deux fichiers chauds au dernier chargement : `.env` du workspace, `EXTERNAL_ENV_PATH`.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
struct Hot {
    workspace: Stamp,
    /// `None` : aucun `EXTERNAL_ENV_PATH` n'était posé à ce chargement.
    external: Option<External>,
}

impl Hot {
    /// Les mêmes fichiers, tels qu'ils sont maintenant : deux `stat`, sans rien lire. Le
    /// chemin externe est celui du dernier chargement (un changement de chemin passe par le
    /// `.env` du workspace ou la base, donc par un changement de stamp).
    fn watch(&self, ws: &Path) -> Hot {
        Hot {
            workspace: stamp(&ws.join(".env")),
            external: self.external.as_ref().map(|external| External {
                path: external.path.clone(),
                stamp: stamp(&external.path),
            }),
        }
    }
}

impl Config {
    /// Lit tout : les dossiers d'état, puis le workspace (résolu avec eux), puis ses fichiers
    /// chauds. Le workspace est créé s'il manque.
    fn load(workspace_override: Option<PathBuf>) -> Config {
        let home = paths::home_dir();
        let mut boot = HashMap::new();
        for dir in paths::state_dirs(&home) {
            dotenv::load_into(&dir.join(".env"), &mut boot);
        }
        let workspace = workspace::resolve(workspace_override, &boot, &home);
        let _ = std::fs::create_dir_all(&workspace);
        let config = Config {
            workspace,
            boot,
            live: RwLock::new(HashMap::new()),
            hot: Mutex::new(Hot::default()),
        };
        config.install(load_hot(&config.workspace, &config.boot));
        config
    }

    /// Copie de la table `.env` effective : variables des fichiers chargées, absentes de l'env
    /// système. À poser sur la commande d'un runner (`cmd.env(k, v)`).
    pub fn dotenv(&self) -> HashMap<String, String> {
        self.live.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Une variable de la table, si elle y est.
    fn lookup(&self, name: &str) -> Option<String> {
        self.live.read().unwrap_or_else(|e| e.into_inner()).get(name).cloned()
    }

    /// Relit les fichiers chauds si l'un d'eux a changé (date ou taille) ; rend vrai si la
    /// table a changé. Rien n'a changé, ou même table après relecture : faux.
    fn reload(&self) -> bool {
        let mut hot = self.hot.lock().unwrap_or_else(|e| e.into_inner());
        if hot.watch(&self.workspace) == *hot {
            return false;
        }
        let (seen, vars) = load_hot(&self.workspace, &self.boot);
        *hot = seen;
        let mut live = self.live.write().unwrap_or_else(|e| e.into_inner());
        if *live == vars {
            return false;
        }
        *live = vars;
        true
    }

    /// Installe un chargement (fichiers vus + table) : le démarrage, ou un test.
    fn install(&self, loaded: (Hot, HashMap<String, String>)) {
        *self.hot.lock().unwrap_or_else(|e| e.into_inner()) = loaded.0;
        *self.live.write().unwrap_or_else(|e| e.into_inner()) = loaded.1;
    }
}

/// Les fichiers chauds et la table qui en résulte : la base de démarrage, puis le `.env` du
/// workspace, puis `EXTERNAL_ENV_PATH` (résolu dans la base et le `.env` du workspace : le
/// premier fichier lu gagne, comme à démarrage).
fn load_hot(ws: &Path, boot: &HashMap<String, String>) -> (Hot, HashMap<String, String>) {
    let env_file = ws.join(".env");
    let mut vars = boot.clone();
    dotenv::load_into(&env_file, &mut vars);
    let mut hot = Hot { workspace: stamp(&env_file), external: None };
    if let Some(path) = env::app_lookup_in(&vars, "EXTERNAL_ENV_PATH").map(PathBuf::from) {
        dotenv::load_into(&path, &mut vars);
        let stamp = stamp(&path);
        hot.external = Some(External { path, stamp });
    }
    (hot, vars)
}

/// Date de modification et taille d'un fichier (absent : `None` et zéro).
fn stamp(path: &Path) -> Stamp {
    std::fs::metadata(path)
        .map(|meta| Stamp { modified: meta.modified().ok(), len: meta.len() })
        .unwrap_or_default()
}

static CONFIG: OnceLock<Config> = OnceLock::new();

pub fn init(workspace_override: Option<PathBuf>) -> &'static Config {
    CONFIG.get_or_init(|| Config::load(workspace_override))
}

pub fn get() -> &'static Config {
    CONFIG.get().expect("config::init doit être appelé au démarrage")
}

/// Relit à chaud le `.env` du workspace et `EXTERNAL_ENV_PATH` (PLAN §4.8), si l'un des deux a
/// changé de date de modification (ou de taille) depuis le dernier chargement ; reconstruit la
/// table depuis la base de démarrage. Rend vrai si la table a changé ; faux sans changement,
/// ou si la configuration n'est pas initialisée. Les `.env` de démarrage ne sont jamais relus ;
/// l'env système garde la priorité.
pub fn reload_env() -> bool {
    CONFIG.get().is_some_and(Config::reload)
}

/// Workspace d'un appel : le paramètre `config` de l'outil, sinon le global.
pub fn workspace_for(override_dir: Option<&str>) -> PathBuf {
    match override_dir.map(str::trim).filter(|s| !s.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => get().workspace.clone(),
    }
}

pub(super) fn dotenv_get(name: &str) -> Option<String> {
    CONFIG.get().and_then(|config| config.lookup(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static SEQ: AtomicU32 = AtomicU32::new(0);

    /// Nom unique dans le process : la variable d'un test ne doit exister ni dans l'env
    /// système, ni dans un `.env` d'un autre test.
    fn variable() -> String {
        format!("SWARM_RELOAD_TEST_{}_{}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed))
    }

    /// Une config sur un workspace temporaire, comme `init` le ferait (sans toucher à la
    /// config globale du process).
    fn config(tag: &str) -> Config {
        let dir = std::env::temp_dir().join(format!(
            "swarm-reload-{tag}-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        Config::load(Some(dir))
    }

    /// Entre deux écritures : la date de modification doit différer (tick de l'horloge
    /// système), pour que le test ne dépende pas de la taille du fichier.
    fn next_tick() {
        std::thread::sleep(std::time::Duration::from_millis(30));
    }

    /// Le test pur du rechargement : un `.env` temporaire, écrit puis modifié, est relu ; la
    /// nouvelle valeur arrive dans la table ; l'env système gagne.
    #[test]
    fn un_env_modifie_est_relu_et_l_environnement_systeme_gagne() {
        let config = config("chaud");
        let env_file = config.workspace.join(".env");
        let name = variable();
        assert!(!config.reload(), "rien à charger : faux");
        assert_eq!(config.lookup(&name), None);

        // Le fichier apparaît → lu. PATH est posé par le `.env`, mais l'env système le porte
        // déjà : il n'entre pas dans la table, et `lookup_in` (la règle de `var`) rend la
        // valeur du système.
        std::fs::write(&env_file, format!("{name}=un\nPATH=/ne/doit/pas/gagner\n")).unwrap();
        assert!(config.reload(), "fichier nouveau : table changée");
        assert_eq!(config.lookup(&name).as_deref(), Some("un"));
        assert!(!config.dotenv().contains_key("PATH"), "l'env système garde la priorité");
        assert_eq!(env::lookup_in(&config.dotenv(), "PATH"), std::env::var("PATH").ok());
        // Sans nouvelle écriture : rien n'est rechargé.
        assert!(!config.reload(), "inchangé : faux");

        // Seule la date change : même taille, autre valeur, un tick d'horloge plus tard.
        next_tick();
        std::fs::write(&env_file, format!("{name}=de\n")).unwrap();
        assert!(config.reload(), "date modifiée : table changée");
        assert_eq!(config.lookup(&name).as_deref(), Some("de"));

        // Seule la taille change (autre valeur, un mot plus long), sans attendre un tick.
        std::fs::write(&env_file, format!("{name}=trois\n")).unwrap();
        assert!(config.reload(), "taille modifiée : table changée");
        assert_eq!(config.lookup(&name).as_deref(), Some("trois"));

        // Fichier supprimé : la variable sort de la table.
        std::fs::remove_file(&env_file).unwrap();
        assert!(config.reload(), "fichier supprimé : table changée");
        assert_eq!(config.lookup(&name), None);
        assert!(!config.reload(), "plus rien à recharger");
        let _ = std::fs::remove_dir_all(&config.workspace);
    }

    /// Même contenu réécrit : le fichier est relu, mais la table est identique → faux.
    #[test]
    fn un_env_reecrit_a_l_identique_ne_change_pas_la_table() {
        let config = config("identique");
        let env_file = config.workspace.join(".env");
        let name = variable();
        std::fs::write(&env_file, format!("{name}=un\n")).unwrap();
        assert!(config.reload(), "premier chargement");
        next_tick();
        std::fs::write(&env_file, format!("{name}=un\n")).unwrap();
        assert!(!config.reload(), "même table : faux");
        assert_eq!(config.lookup(&name).as_deref(), Some("un"));
        let _ = std::fs::remove_dir_all(&config.workspace);
    }

    /// Une variable de démarrage (dossiers d'état) n'est jamais relue ; elle sert de base et
    /// garde la priorité sur le `.env` du workspace, comme au démarrage.
    #[test]
    fn la_base_de_demarrage_est_immuable_et_prime_sur_le_env_du_workspace() {
        let mut config = config("base");
        let base = variable();
        config.boot.insert(base.clone(), "démarrage".into());
        config.install(load_hot(&config.workspace, &config.boot));
        let env_file = config.workspace.join(".env");
        std::fs::write(&env_file, format!("{base}=workspace\nAUTRE=oui\n")).unwrap();
        assert!(config.reload(), "le `.env` du workspace arrive");
        assert_eq!(
            config.lookup(&base).as_deref(),
            Some("démarrage"),
            "le premier fichier lu gagne"
        );
        assert_eq!(config.lookup("AUTRE").as_deref(), Some("oui"));
        // La base ne bouge pas : un `reload` la repartage telle quelle.
        std::fs::write(&env_file, "AUTRE=non\n").unwrap();
        assert!(config.reload());
        assert_eq!(config.lookup(&base).as_deref(), Some("démarrage"));
        assert_eq!(config.lookup("AUTRE").as_deref(), Some("non"));
        let _ = std::fs::remove_dir_all(&config.workspace);
    }

    /// `EXTERNAL_ENV_PATH` (posé par le `.env` du workspace) : lu après lui, donc il perd sur
    /// une variable commune ; rechargé quand le fichier externe change seul, puis quand le
    /// `.env` du workspace en change le chemin (l'ancien n'est plus suivi).
    #[test]
    fn le_fichier_externe_est_charge_apres_le_env_et_relu_a_chaud() {
        let config = config("externe");
        let env_file = config.workspace.join(".env");
        let first = config.workspace.join("secret.env");
        let second = config.workspace.join("autre.env");
        std::fs::write(&first, "EXTERNE=premier\nCOMMUN=externe\n").unwrap();
        std::fs::write(&second, "EXTERNE=second\n").unwrap();
        std::fs::write(
            &env_file,
            format!("COMMUN=workspace\nSWARM_EXTERNAL_ENV_PATH={}\n", first.display()),
        )
        .unwrap();
        assert!(config.reload(), "le `.env` du workspace et son fichier externe arrivent");
        assert_eq!(
            config.lookup("COMMUN").as_deref(),
            Some("workspace"),
            "le `.env` du workspace gagne"
        );
        assert_eq!(config.lookup("EXTERNE").as_deref(), Some("premier"));
        assert!(!config.reload(), "rien n'a bougé");

        // Le fichier externe change seul : il est relu.
        next_tick();
        std::fs::write(&first, "EXTERNE=recu\n").unwrap();
        assert!(config.reload(), "fichier externe modifié");
        assert_eq!(config.lookup("COMMUN").as_deref(), Some("workspace"));
        assert_eq!(config.lookup("EXTERNE").as_deref(), Some("recu"));

        // Le `.env` du workspace impose un autre chemin : c'est lui qu'on suit désormais.
        std::fs::write(&env_file, format!("SWARM_EXTERNAL_ENV_PATH={}\n", second.display()))
            .unwrap();
        assert!(config.reload(), "chemin externe changé");
        assert_eq!(config.lookup("EXTERNE").as_deref(), Some("second"));
        assert!(!config.reload());
        // L'ancien fichier ne compte plus : le modifier ne recharge rien.
        next_tick();
        std::fs::write(&first, "EXTERNE=ignore\n").unwrap();
        assert!(!config.reload(), "l'ancien fichier externe n'est plus suivi");
        // Le nouveau, oui.
        next_tick();
        std::fs::write(&second, "EXTERNE=suivi\n").unwrap();
        assert!(config.reload());
        assert_eq!(config.lookup("EXTERNE").as_deref(), Some("suivi"));
        let _ = std::fs::remove_dir_all(&config.workspace);
    }

    /// `EXTERNAL_ENV_PATH` venu d'un `.env` de démarrage : immuable, il prime sur celui du
    /// `.env` du workspace (le premier fichier lu gagne), et son fichier reste suivi.
    #[test]
    fn le_chemin_externe_de_demarrage_prime_sur_celui_du_workspace() {
        let mut config = config("externe-base");
        let first = config.workspace.join("secret.env");
        let second = config.workspace.join("autre.env");
        std::fs::write(&first, "EXTERNE=premier\n").unwrap();
        std::fs::write(&second, "EXTERNE=second\n").unwrap();
        config.boot.insert("SWARM_EXTERNAL_ENV_PATH".into(), first.display().to_string());
        config.install(load_hot(&config.workspace, &config.boot));
        assert_eq!(config.lookup("EXTERNE").as_deref(), Some("premier"));
        std::fs::write(
            config.workspace.join(".env"),
            format!("AUTRE=oui\nSWARM_EXTERNAL_ENV_PATH={}\n", second.display()),
        )
        .unwrap();
        assert!(config.reload(), "le `.env` du workspace arrive (AUTRE)");
        assert_eq!(config.lookup("AUTRE").as_deref(), Some("oui"));
        assert_eq!(
            config.lookup("EXTERNE").as_deref(),
            Some("premier"),
            "la base de démarrage garde son chemin"
        );
        // Le fichier de démarrage reste celui qu'on suit.
        next_tick();
        std::fs::write(&first, "EXTERNE=recu\n").unwrap();
        assert!(config.reload(), "fichier externe de démarrage modifié");
        assert_eq!(config.lookup("EXTERNE").as_deref(), Some("recu"));
        let _ = std::fs::remove_dir_all(&config.workspace);
    }

    /// Config globale du process : initialisée ou non selon les tests déjà passés ; l'appel
    /// rend un booléen dans les deux cas, sans panique.
    #[test]
    fn reload_env_rend_un_booleen_sans_paniquer() {
        let _ = super::reload_env();
    }
}
