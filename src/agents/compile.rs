//! Compilation native d'un agent persistant (PLAN §4.8, décision 2 du §3).
//!
//! Un agent persistant = un **petit crate compilé** qui dépend de `swarm-control` et
//! appelle [`crate::serve_compiled`] avec son nom, sa classe et une définition par défaut.
//! L'identité (nom, classe) est **gravée dans le binaire**, jamais déduite du nom de
//! fichier ; la définition du workspace (persona, modèle, serveurs MCP) et les identifiants
//! restent sur disque, relus à chaque tour : aucun secret n'entre dans le binaire.
//!
//! - [`generate`] écrit `<ws>/agents/<agent>/build/` : `Cargo.toml`, `src/main.rs`,
//!   `agent.json` (définition par défaut, sans secret) et `build.rs` ;
//! - [`build`] lance `cargo build --release` au premier plan, [`spawn_build`] en arrière-plan
//!   (dossier de compilation partagé `<ws>/build/target`, journal dans `<ws>/logs/`) ;
//! - [`bin_version`], [`rebuild_needed`] et [`remove`] complètent le cycle de vie.
//!
//! Sans `cargo` ([`cargo_path`] rend `None`), la compilation échoue avec un message clair :
//! il n'y a **pas de repli par copie** (décision 2).

use super::executable;
use super::name::validate_name;
use super::runner::Runner;
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Lien vers l'installation de Rust, pour le message d'erreur sans `cargo`.
pub const RUSTUP_URL: &str = "https://rustup.rs";

/// Message type : `cargo` est introuvable sur cette machine.
pub fn no_cargo_message() -> String {
    format!(
        "cargo introuvable (PATH, puis variable CARGO) : installe Rust depuis {RUSTUP_URL} \
             puis relance `swarm-control build <agent>`."
    )
}

/// Dossier de compilation de l'agent : `<ws>/agents/<agent>/build/`.
pub fn build_dir(ws: &Path, agent: &str) -> PathBuf {
    ws.join("agents").join(agent).join("build")
}

/// Journal de compilation : `<ws>/logs/build-<agent>.log`.
pub fn log_path(ws: &Path, agent: &str) -> PathBuf {
    ws.join("logs").join(format!("build-{agent}.log"))
}

/// Dossier de compilation partagé : `<ws>/build/target` (les agents suivants ne
/// recompilent que leur `main` et l'édition de liens).
pub fn target_dir(ws: &Path) -> PathBuf {
    ws.join("build").join("target")
}

// --- Génération du crate ---------------------------------------------------

/// Génère (ou régénère, de façon idempotente) le crate de l'agent sous
/// `<ws>/agents/<agent>/build/`, et rend ce dossier.
///
/// `args` est l'objet d'arguments de `create_agent` (mêmes clés : `prompt`, `model`,
/// `mcpServers`, `env`, `template`, `runner`, `executable`, `unit`…). Toute valeur d'`env`
/// et tout `env` / `headers` de serveur MCP sont **retirés** du fichier écrit ; un `args`
/// qui contient un secret est **refusé** (même détection que les lectures masquées des
/// configurations : nom de clé TOKEN / SECRET / KEY / PASSWORD / Authorization, ou valeur
/// ressemblant à un jeton `sk-…`, `ghp_…`, `Bearer …`).
pub fn generate(
    ws: &Path,
    agent: &str,
    class: &str,
    version: &str,
    args: &Value,
) -> Result<PathBuf, String> {
    generate_with_path(ws, agent, class, version, args, None)
}

/// Corps de [`generate`] : la dépendance `swarm-control` est épinglée à `version`, ou
/// pointe sur un dossier local (`path_dependency`) pour les tests et la CI.
pub fn generate_with_path(
    ws: &Path,
    agent: &str,
    class: &str,
    version: &str,
    args: &Value,
    path_dependency: Option<&Path>,
) -> Result<PathBuf, String> {
    validate_name(agent)?;
    let definition = definition_of(agent, class, args)?;
    let dir = build_dir(ws, agent);
    let src = dir.join("src");
    std::fs::create_dir_all(&src)
        .map_err(|e| format!("Dossier '{}' impossible à créer : {e}", src.display()))?;
    write(&dir.join("Cargo.toml"), &cargo_toml(agent, version, path_dependency))?;
    write(&src.join("main.rs"), &main_rs(agent, definition["class"].as_str().unwrap_or("custom")))?;
    write(
        &dir.join("agent.json"),
        &serde_json::to_string_pretty(&definition).map_err(|e| e.to_string())?,
    )?;
    write(&dir.join("build.rs"), BUILD_RS)?;
    Ok(dir)
}

/// Écrit un fichier en UTF-8, seulement si son contenu change (régénération idempotente :
/// `cargo` ne recompile pas ce qui n'a pas bougé).
fn write(path: &Path, content: &str) -> Result<(), String> {
    if std::fs::read_to_string(path).is_ok_and(|existing| existing == content) {
        return Ok(());
    }
    std::fs::write(path, content)
        .map_err(|e| format!("Écriture de '{}' impossible : {e}", path.display()))
}

/// Définition par défaut de l'agent, telle qu'embarquée dans le binaire (`agent.json`) :
/// de quoi le **recréer** si son dossier manque — jamais un secret, jamais les valeurs
/// d'`env` ni celles des serveurs MCP (seuls leurs noms restent).
fn definition_of(agent: &str, class: &str, args: &Value) -> Result<Value, String> {
    refuse_secrets(args)?;
    let class = super::logo::CLASSES.iter().find(|c| **c == class).copied().unwrap_or("custom");
    let mut doc = Map::new();
    doc.insert("name".into(), Value::String(agent.into()));
    doc.insert("class".into(), Value::String(class.into()));
    let runner = args
        .get("runner")
        .and_then(Value::as_str)
        .and_then(Runner::parse)
        .unwrap_or(Runner::Claude);
    doc.insert("runner".into(), Value::String(runner.as_str().into()));
    // Persona : prompt explicite ; avec un modèle, la persona interpolée de celui-ci a été
    // passée par l'appelant (create_agent) dans `prompt`.
    let prompt = args.get("prompt").and_then(Value::as_str).ok_or_else(|| {
        "prompt requis : la définition par défaut embarque la persona.".to_string()
    })?;
    doc.insert("prompt".into(), Value::String(prompt.into()));
    for key in ["model", "template"] {
        if let Some(v) = args.get(key).filter(|v| v.is_string()) {
            doc.insert(key.into(), v.clone());
        }
    }
    // Serveurs MCP : les noms seulement (les commandes, URL, env et headers restent au
    // workspace, relus à chaque tour).
    if let Some(servers) = args.get("mcpServers").and_then(Value::as_object) {
        let names: Vec<Value> = servers.keys().map(|k| Value::String(k.clone())).collect();
        if !names.is_empty() {
            doc.insert("mcpServers".into(), Value::Array(names));
        }
    }
    Ok(Value::Object(doc))
}

// --- Secrets ---------------------------------------------------------------

/// Nom de clé qui désigne un secret (même famille que `agents::secrets` et
/// `debug::mask`, par **mot** : TOKEN, SECRET, KEY, PASSWORD, AUTHORIZATION — `MONKEY`
/// n'en est pas).
fn is_secret_name(name: &str) -> bool {
    name.to_uppercase()
        .split(['_', '-', ' ', '.'])
        .any(|word| matches!(word, "TOKEN" | "SECRET" | "KEY" | "PASSWORD" | "AUTHORIZATION"))
}

/// Valeur qui ressemble à un jeton : `sk-…`, `ghp_…`, `Bearer …` (même famille que
/// `memory::secrets` ; ici seuls les préfixes sans ambiguïté comptent).
fn looks_like_token(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    ["sk-", "ghp_", "bearer "].iter().any(|p| {
        lower.find(p).is_some_and(|at| {
            let before = lower[..at].chars().next_back();
            before.is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        })
    })
}

/// Refuse un `args` qui contient un secret : clé au nom de secret, ou valeur qui
/// ressemble à un jeton. Le message ne montre jamais la valeur.
pub fn refuse_secrets(args: &Value) -> Result<(), String> {
    if let Some((path, why)) = find_secret(args, "") {
        return Err(format!(
            "arguments de compilation refusés : {why} dans « {path} » — aucun secret n'entre \
             dans un binaire. Les identifiants restent dans le workspace (settings de l'agent, \
             .env du profil, .env du workspace) et sont relus à chaque tour."
        ));
    }
    Ok(())
}

/// Cherche un secret dans un document JSON ; rend `(chemin, raison)`.
fn find_secret(value: &Value, path: &str) -> Option<(String, &'static str)> {
    match value {
        Value::Object(map) => {
            for (key, val) in map {
                let here = if path.is_empty() { key.clone() } else { format!("{path}.{key}") };
                if is_secret_name(key)
                    && val.as_str().is_some_and(|s| !s.is_empty() && !s.starts_with('$'))
                {
                    return Some((here, "clé de secret"));
                }
                if let Some(found) = find_secret(val, &here) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(items) => {
            items.iter().enumerate().find_map(|(i, v)| find_secret(v, &format!("{path}[{i}]")))
        }
        Value::String(s) if looks_like_token(s) => {
            Some((path.to_string(), "valeur ressemblant à un jeton"))
        }
        _ => None,
    }
}

// --- Cargo.toml / main.rs / build.rs ---------------------------------------

/// `Cargo.toml` du crate généré : paquet `swarm-agent-<agent>`, édition 2024, dépendance
/// `swarm-control` épinglée à `version` (ou par chemin avec `path_dependency`), sans les
/// features par défaut si le générateur n'a pas la TUI ; binaire nommé d'après l'agent ;
/// `[workspace]` vide pour isoler le crate du dépôt ambiant.
pub fn cargo_toml(agent: &str, version: &str, path_dependency: Option<&Path>) -> String {
    let dependency = match path_dependency {
        Some(path) => format!(
            "{{ path = {}, version = \"={version}\"{} }}",
            toml_string(&path.display().to_string()),
            default_features()
        ),
        None => format!("{{ version = \"={version}\"{} }}", default_features()),
    };
    let windows_build_dep = if cfg!(windows) {
        "\n[target.'cfg(windows)'.build-dependencies]\nwinresource = \"0.1\"\n"
    } else {
        ""
    };
    format!(
        "[package]\nname = \"swarm-agent-{agent}\"\nversion = \"{version}\"\nedition = \"2024\"\n\
         publish = false\n\n[dependencies]\nswarm-control = {dependency}\n\
         {windows_build_dep}\n[[bin]]\nname = \"{agent}\"\npath = \"src/main.rs\"\n\n[workspace]\n"
    )
}

/// `default-features = false` quand le générateur est compilé sans la TUI : l'agent hérite
/// des capacités de l'instance qui le génère.
fn default_features() -> &'static str {
    if cfg!(feature = "tui") { "" } else { ", default-features = false" }
}

/// Chaîne TOML : antislashs et guillemets échappés.
fn toml_string(raw: &str) -> String {
    format!("\"{}\"", raw.replace('\\', "\\\\").replace('"', "\\\""))
}

/// `src/main.rs` du crate généré, exactement sur le modèle du PLAN §4.8 : nom et classe
/// gravés, définition par défaut embarquée par `include_str!`.
pub fn main_rs(agent: &str, class: &str) -> String {
    format!(
        "//! Binaire compilé de l'agent `{agent}` (classe `{class}`), généré par swarm-control.\n\n\
         const AGENT: &str = \"{agent}\";\n\
         const CLASS: &str = \"{class}\";\n\
         const DEFINITION: &str = include_str!(\"../agent.json\"); // définition PAR DÉFAUT, sans secret\n\n\
         fn main() -> std::process::ExitCode {{\n    \
             swarm_control::serve_compiled(AGENT, CLASS, DEFINITION)\n}}\n"
    )
}

/// `build.rs` du crate généré : sous Windows, icône de la classe (`resources/icon.ico`,
/// s'il existe) et version du paquet dans les ressources de l'exécutable.
pub const BUILD_RS: &str = r#"// Généré par swarm-control : ressources Windows de l'exécutable de l'agent.
fn main() {
    println!("cargo:rerun-if-changed=resources/icon.ico");
    #[cfg(windows)]
    {
        if std::path::Path::new("resources/icon.ico").is_file() {
            let mut res = winresource::WindowsResource::new();
            res.set_icon("resources/icon.ico");
            res.set("FileDescription", &format!("Agent swarm-control {}", env!("CARGO_PKG_NAME")));
            res.set("ProductName", "swarm-control");
            res.set("FileVersion", env!("CARGO_PKG_VERSION"));
            res.set("ProductVersion", env!("CARGO_PKG_VERSION"));
            res.set("LegalCopyright", "MIT OR Apache-2.0");
            res.compile().expect("ressources Windows");
        }
    }
}
"#;

// --- cargo -----------------------------------------------------------------

/// Chemin de `cargo` : dans `PATH` d'abord (avec l'extension de l'exécutable sous
/// Windows), sinon la variable `CARGO` (posée par `cargo` lui-même). `None` s'il est
/// introuvable.
pub fn cargo_path() -> Option<PathBuf> {
    cargo_in(std::env::var_os("PATH").as_deref())
        .or_else(|| std::env::var_os("CARGO").map(PathBuf::from).filter(|p| p.is_file()))
}

/// `cargo` dans un `PATH` donné (même recherche que [`cargo_path`], testable sans toucher
/// à l'environnement du process). Sous Windows, `PATH` sans extension trouve aussi
/// `cargo.exe` ; un dossier n'est jamais pris pour un exécutable.
fn cargo_in(path: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    let name = format!("cargo{}", std::env::consts::EXE_SUFFIX);
    path.and_then(|path| {
        std::env::split_paths(path).map(|dir| dir.join(&name)).find(|candidate| candidate.is_file())
    })
}

/// Version de `cargo` (`cargo --version`, première ligne), pour `doctor`.
///
/// Sans appelant pour l'instant : la ligne « compilation d'agents » de `doctor` (§4.8)
/// est à venir.
#[allow(dead_code)]
pub fn cargo_version(cargo: &Path) -> Option<String> {
    let out = Command::new(cargo).arg("--version").output().ok()?;
    let text = String::from_utf8(out.stdout).ok()?;
    let line = text.lines().next()?.trim();
    (!line.is_empty()).then(|| line.to_string())
}

// --- Compilation -----------------------------------------------------------

/// Options de compilation.
#[derive(Debug, Default, Clone)]
pub struct BuildOptions {
    /// `cargo build --offline` : le registre local doit déjà avoir les dépendances.
    pub offline: bool,
}

/// Résultat d'une compilation réussie.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildOutcome {
    /// Binaire final : `executable::path(ws, agent)`.
    pub binary: PathBuf,
    /// Journal de compilation.
    pub log: PathBuf,
    /// Durée, en ms.
    pub elapsed_ms: u64,
}

/// Compile l'agent au premier plan : régénère le crate, lance
/// `cargo build --release --manifest-path <build>/Cargo.toml --target-dir <ws>/build/target`,
/// journalise dans `<ws>/logs/build-<agent>.log`, puis copie le binaire vers
/// `executable::path(ws, agent)`. Sans `cargo` : erreur claire avec le lien rustup.
pub fn build(ws: &Path, agent: &str, opts: &BuildOptions) -> Result<BuildOutcome, String> {
    let started = std::time::Instant::now();
    let cargo = cargo_path().ok_or_else(no_cargo_message)?;
    let manifest = build_dir(ws, agent).join("Cargo.toml");
    if !manifest.is_file() {
        return Err(format!(
            "crate de '{agent}' absent ({}) : recrée l'agent avec executable:true, ou régénère-le.",
            manifest.display()
        ));
    }
    let target = target_dir(ws);
    let log = log_path(ws, agent);
    if let Some(parent) = log.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Dossier '{}' impossible à créer : {e}", parent.display()))?;
    }
    let mut cmd = Command::new(&cargo);
    cmd.arg("build")
        .arg("--release")
        .arg("--manifest-path")
        .arg(&manifest)
        .arg("--target-dir")
        .arg(&target);
    if opts.offline {
        cmd.arg("--offline");
    }
    let output =
        cmd.output().map_err(|e| format!("Lancement de '{}' impossible : {e}", cargo.display()))?;
    let mut journal = String::new();
    journal.push_str(&format!("$ {} {}\n", cargo.display(), args_text(&cmd)));
    journal.push_str(&String::from_utf8_lossy(&output.stdout));
    journal.push_str(&String::from_utf8_lossy(&output.stderr));
    std::fs::write(&log, &journal)
        .map_err(|e| format!("Écriture du journal '{}' impossible : {e}", log.display()))?;
    print!("{journal}");
    if !output.status.success() {
        return Err(format!(
            "compilation de '{agent}' échouée (code {}) — journal : {}",
            output.status.code().map_or("?".to_string(), |c| c.to_string()),
            log.display()
        ));
    }
    let built = target.join("release").join(format!("{agent}{}", std::env::consts::EXE_SUFFIX));
    let binary = executable::path(ws, agent);
    if let Some(parent) = binary.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Dossier '{}' impossible à créer : {e}", parent.display()))?;
    }
    std::fs::copy(&built, &binary).map_err(|e| {
        format!("Copie de '{}' vers '{}' impossible : {e}", built.display(), binary.display())
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&binary)
            .map_err(|e| format!("Permissions de '{}' illisibles : {e}", binary.display()))?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&binary, perms).map_err(|e| {
            format!("Permissions de '{}' impossibles à régler : {e}", binary.display())
        })?;
    }
    sign_macos(&binary);
    Ok(BuildOutcome { binary, log, elapsed_ms: started.elapsed().as_millis() as u64 })
}

/// Lance la compilation en arrière-plan sur un fil nommé ; rend le chemin du journal.
/// Les erreurs sont écrites dans le journal (et sur stderr), jamais levées.
pub fn spawn_build(ws: &Path, agent: &str, opts: BuildOptions) -> Result<PathBuf, String> {
    let log = log_path(ws, agent);
    if let Some(parent) = log.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Dossier '{}' impossible à créer : {e}", parent.display()))?;
    }
    let ws = ws.to_path_buf();
    let agent = agent.to_string();
    let builder = std::thread::Builder::new().name(format!("build-{agent}"));
    let spawned = builder.spawn({
        let ws = ws.clone();
        let agent = agent.clone();
        let log = log.clone();
        move || {
            if let Err(e) = build(&ws, &agent, &opts) {
                eprintln!("swarm-control: {e}");
                let _ = std::fs::write(&log, format!("swarm-control: {e}\n"));
            }
        }
    });
    spawned
        .map_err(|e| format!("Fil de compilation de '{agent}' impossible à lancer : {e}"))
        .map(|_| log)
}

/// macOS : signature ad hoc si `codesign` est disponible (le binaire reste valide sans).
#[cfg(target_os = "macos")]
fn sign_macos(binary: &Path) {
    let _ = Command::new("codesign").arg("-s").arg("-").arg(binary).output();
}

#[cfg(not(target_os = "macos"))]
fn sign_macos(_binary: &Path) {}

/// Ligne de commande lisible pour le journal (les arguments ne portent jamais de secret :
/// chemins et drapeaux seulement).
fn args_text(cmd: &Command) -> String {
    cmd.get_args().map(|a| a.to_string_lossy().to_string()).collect::<Vec<_>>().join(" ")
}

/// Version rendue par `<bin> --version` (première ligne de stdout), ou `None` si le
/// binaire ne se lance pas ou ne répond pas.
pub fn bin_version(path: &Path) -> Option<String> {
    let out = Command::new(path).arg("--version").output().ok()?;
    let text = String::from_utf8(out.stdout).ok()?;
    let line = text.lines().next()?.trim();
    (!line.is_empty()).then(|| line.to_string())
}

/// Faut-il recompiler l'agent ? Vrai si le binaire manque, s'il ne répond pas à
/// `--version`, ou si sa version diffère de celle de l'instance (version de
/// swarm-control épinglée dans le crate).
///
/// Sans appelant pour l'instant : `status` publiera `rebuildNeeded` (§4.8).
#[allow(dead_code)]
pub fn rebuild_needed(ws: &Path, agent: &str) -> bool {
    let binary = executable::path(ws, agent);
    bin_version(&binary).is_none_or(|line| !version_matches(&line, env!("CARGO_PKG_VERSION")))
}

/// Supprime le binaire compilé (`<ws>/bin/<agent><exe>`) et le dossier de compilation
/// (`<ws>/agents/<agent>/build/`). Rend les chemins effectivement supprimés.
pub fn remove(ws: &Path, agent: &str) -> Vec<PathBuf> {
    let mut removed = Vec::new();
    let binary = executable::path(ws, agent);
    if std::fs::remove_file(&binary).is_ok() {
        removed.push(binary);
    }
    let dir = build_dir(ws, agent);
    if dir.is_dir() && std::fs::remove_dir_all(&dir).is_ok() {
        removed.push(dir);
    }
    removed
}

/// La ligne `--version` du binaire annonce-t-elle la version courante de swarm-control ?
/// Comparaison par mot entier : `0.1.2` ne satisfait pas `0.1.20`.
fn version_matches(bin_version: &str, current: &str) -> bool {
    bin_version.split_whitespace().any(|word| word == current)
}

#[cfg(test)]
mod tests {
    use super::{
        BUILD_RS, build_dir, cargo_in, definition_of, generate, generate_with_path, is_secret_name,
        log_path, looks_like_token, rebuild_needed, remove, target_dir, write,
    };
    use serde_json::json;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU32, Ordering};

    static SEQ: AtomicU32 = AtomicU32::new(0);

    /// Dossier temporaire unique, supprimé à la fin du test.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let n = SEQ.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir()
                .join(format!("swarm-compile-{tag}-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn args() -> serde_json::Value {
        json!({ "name": "discord", "prompt": "Tu es Discord.", "runner": "claude", "model": "sonnet" })
    }

    #[test]
    fn les_fichiers_generes_sont_exacts_et_sans_valeur_d_env() {
        let ws = TempDir::new("fichiers");
        let mut a = args();
        a["env"] =
            json!({ "ANTHROPIC_BASE_URL": "https://example.test", "ANTHROPIC_MODEL": "sonnet" });
        a["mcpServers"] = json!({ "db": { "command": "node", "env": { "DB_HOST": "db.local" } } });
        let dir = generate(&ws.0, "discord", "discord", "0.1.2", &a).unwrap();
        assert_eq!(dir, build_dir(&ws.0, "discord"));
        // Cargo.toml : paquet, édition, version épinglée, binaire nommé, workspace vide.
        let manifest = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap();
        for part in [
            "name = \"swarm-agent-discord\"",
            "edition = \"2024\"",
            "version = \"=0.1.2\"",
            "[[bin]]\nname = \"discord\"",
            "[workspace]",
        ] {
            assert!(manifest.contains(part), "{part} absent de :\n{manifest}");
        }
        #[cfg(feature = "tui")]
        assert!(!manifest.contains("default-features"), "{manifest}");
        #[cfg(not(feature = "tui"))]
        assert!(manifest.contains("default-features = false"), "{manifest}");
        // main.rs : constantes gravées, include_str!, appel exact du PLAN.
        let main = std::fs::read_to_string(dir.join("src").join("main.rs")).unwrap();
        assert!(main.contains("const AGENT: &str = \"discord\";"), "{main}");
        assert!(main.contains("const CLASS: &str = \"discord\";"), "{main}");
        assert!(
            main.contains("const DEFINITION: &str = include_str!(\"../agent.json\"); // définition PAR DÉFAUT, sans secret"),
            "{main}"
        );
        assert!(
            main.contains("fn main() -> std::process::ExitCode {\n    swarm_control::serve_compiled(AGENT, CLASS, DEFINITION)\n}"),
            "{main}"
        );
        // agent.json : définition sans secret, sans valeur d'env ni de serveur MCP.
        let definition = std::fs::read_to_string(dir.join("agent.json")).unwrap();
        assert!(!definition.contains("ANTHROPIC_BASE_URL"), "{definition}");
        assert!(!definition.contains("db.local"), "{definition}");
        assert!(!definition.contains("example.test"), "{definition}");
        let doc: serde_json::Value = serde_json::from_str(&definition).unwrap();
        assert_eq!(doc["name"], "discord");
        assert_eq!(doc["class"], "discord");
        assert_eq!(doc["runner"], "claude");
        assert_eq!(doc["model"], "sonnet");
        assert_eq!(doc["mcpServers"], json!(["db"]), "noms seulement");
        assert!(std::fs::read_to_string(dir.join("build.rs")).unwrap() == BUILD_RS);
    }

    #[test]
    fn un_jeton_ou_une_cle_de_secret_est_refuse() {
        let ws = TempDir::new("jeton");
        // Clé au nom de secret, où que ce soit (env, serveur MCP, headers).
        for bad in [
            json!({ "env": { "ANTHROPIC_AUTH_TOKEN": "valeur" } }),
            json!({ "mcpServers": { "db": { "headers": { "Authorization": "x" } } } }),
            json!({ "env": { "API_KEY": "abc" } }),
        ] {
            let mut a = args();
            a["env"] = bad.get("env").cloned().unwrap_or(json!({}));
            if let Some(s) = bad.get("mcpServers") {
                a["mcpServers"] = s.clone();
            }
            let err = generate(&ws.0, "discord", "discord", "0.1.2", &a).unwrap_err();
            assert!(err.contains("refusés") && err.contains("secret"), "{err}");
            assert!(
                !err.contains("valeur") || err.contains("clé de secret"),
                "le message ne montre pas la valeur : {err}"
            );
        }
        // Valeur ressemblant à un jeton, même sous une clé anodine.
        let mut a = args();
        a["prompt"] = json!("ma clé : sk-abc123def456ghi789jkl");
        let err = generate(&ws.0, "discord", "discord", "0.1.2", &a).unwrap_err();
        assert!(err.contains("jeton"), "{err}");
        assert!(!err.contains("sk-abc123"), "le jeton n'est jamais répété : {err}");
        let mut a = args();
        a["env"] = json!({ "NOTE": "Bearer abcdef1234567890" });
        assert!(generate(&ws.0, "discord", "discord", "0.1.2", &a).is_err());
        // Rien n'a été écrit.
        assert!(!build_dir(&ws.0, "discord").exists());
    }

    #[test]
    fn un_nom_invalide_ou_un_prompt_absent_est_refuse() {
        let ws = TempDir::new("nom");
        let err = generate(&ws.0, "../évasion", "discord", "0.1.2", &args()).unwrap_err();
        assert!(err.contains("Nom d'agent invalide"), "{err}");
        let mut a = args();
        a.as_object_mut().unwrap().remove("prompt");
        let err = generate(&ws.0, "alice", "discord", "0.1.2", &a).unwrap_err();
        assert!(err.contains("prompt requis"), "{err}");
        // Une classe inconnue replie sur `custom`.
        let dir = generate(&ws.0, "alice", "inconnue", "0.1.2", &args()).unwrap();
        let main = std::fs::read_to_string(dir.join("src").join("main.rs")).unwrap();
        assert!(main.contains("const CLASS: &str = \"custom\";"), "{main}");
    }

    #[test]
    fn la_regeneration_est_idempotente() {
        let ws = TempDir::new("idempotent");
        let dir = generate(&ws.0, "alice", "custom", "0.1.2", &args()).unwrap();
        let first = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap();
        // Une seconde génération ne réécrit rien (mtimes conservés).
        let before = std::fs::metadata(dir.join("Cargo.toml")).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        generate(&ws.0, "alice", "custom", "0.1.2", &args()).unwrap();
        let after = std::fs::metadata(dir.join("Cargo.toml")).unwrap().modified().unwrap();
        assert_eq!(before, after, "un fichier inchangé n'est pas réécrit");
        assert_eq!(first, std::fs::read_to_string(dir.join("Cargo.toml")).unwrap());
        // Avec une dépendance par chemin (tests/CI) : `path = "…"`, version épinglée aussi.
        let dir = generate_with_path(
            &ws.0,
            "alice",
            "custom",
            "0.1.2",
            &args(),
            Some(Path::new("/depôt")),
        )
        .unwrap();
        let manifest = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap();
        assert!(manifest.contains("path = ") && manifest.contains("=0.1.2"), "{manifest}");
        assert!(manifest.contains("/depôt") || manifest.contains("\\\\depôt"), "{manifest}");
    }

    #[test]
    fn cargo_dans_le_path_ou_absent() {
        // Sans cargo nulle part : None.
        assert_eq!(cargo_in(None), None);
        let empty = TempDir::new("cargo-vide");
        let path = empty.0.clone().into_os_string();
        assert_eq!(cargo_in(Some(&path)), None);
        // Un faux cargo(.exe) dans un dossier du PATH : trouvé, avec l'extension de la plateforme.
        let dir = TempDir::new("cargo-present");
        let name = format!("cargo{}", std::env::consts::EXE_SUFFIX);
        std::fs::write(dir.0.join(&name), b"").unwrap();
        assert_eq!(cargo_in(Some(&dir.0.clone().into_os_string())), Some(dir.0.join(&name)));
        // Deux dossiers : le premier qui le contient gagne.
        let two = std::env::join_paths([&empty.0, &dir.0]).unwrap();
        assert_eq!(cargo_in(Some(&two)), Some(dir.0.join(&name)));
    }

    #[test]
    fn rebuild_needed_est_vrai_sans_binaire_et_pour_un_faux_binaire() {
        let ws = TempDir::new("rebuild");
        assert!(rebuild_needed(&ws.0, "alice"), "sans binaire : à compiler");
        // Un faux binaire qui ne se lance pas (fichier texte) : toujours à compiler.
        let bin = super::super::executable::path(&ws.0, "alice");
        std::fs::create_dir_all(bin.parent().unwrap()).unwrap();
        std::fs::write(&bin, "ceci n'est pas un executable").unwrap();
        assert!(rebuild_needed(&ws.0, "alice"), "binaire non exécutable : à compiler");
        assert_eq!(super::bin_version(&bin), None);
        // Un faux binaire absent : aucune version.
        assert_eq!(super::bin_version(&ws.0.join("absent")), None);
    }

    #[test]
    fn rebuild_needed_compare_la_version_du_binaire() {
        // `bin_version` lit la première ligne de `--version` ; la comparaison est portée par
        // `version_matches`, testée ici indépendamment du binaire.
        use super::version_matches;
        let current = env!("CARGO_PKG_VERSION");
        assert!(version_matches(
            &format!("swarm-control {current} (agent alice, classe custom)"),
            current
        ));
        assert!(!version_matches("swarm-control 0.0.1 (agent alice, classe custom)", current));
        assert!(!version_matches("", current));
    }

    #[test]
    fn remove_efface_le_binaire_et_le_dossier_de_build() {
        let ws = TempDir::new("remove");
        let bin = super::super::executable::path(&ws.0, "alice");
        std::fs::create_dir_all(bin.parent().unwrap()).unwrap();
        std::fs::write(&bin, b"x").unwrap();
        std::fs::create_dir_all(build_dir(&ws.0, "alice").join("src")).unwrap();
        std::fs::write(build_dir(&ws.0, "alice").join("src").join("main.rs"), b"x").unwrap();
        let mut removed = remove(&ws.0, "alice");
        removed.sort();
        assert_eq!(removed.len(), 2, "{removed:?}");
        assert!(!bin.exists());
        assert!(!build_dir(&ws.0, "alice").exists());
        assert!(remove(&ws.0, "alice").is_empty(), "déjà tout retiré");
        // Le profil de l'agent lui-même reste.
        std::fs::create_dir_all(ws.0.join("agents").join("bob")).unwrap();
        remove(&ws.0, "bob");
        assert!(ws.0.join("agents").join("bob").is_dir());
    }

    #[test]
    fn les_chemins_derivent_du_workspace() {
        let ws = Path::new("/ws");
        assert_eq!(build_dir(ws, "a"), Path::new("/ws/agents/a/build"));
        assert_eq!(log_path(ws, "a"), Path::new("/ws/logs/build-a.log"));
        assert_eq!(target_dir(ws), Path::new("/ws/build/target"));
    }

    #[test]
    fn la_detection_des_secrets_reconnait_les_noms_et_les_jetons() {
        for name in [
            "TOKEN",
            "ANTHROPIC_AUTH_TOKEN",
            "API_SECRET",
            "SECRET_KEY",
            "PASSWORD",
            "Authorization",
            "GITHUB_KEY",
        ] {
            assert!(is_secret_name(name), "{name}");
        }
        for name in ["ANTHROPIC_BASE_URL", "agent", "model", "DB_HOST", "MONKEY"] {
            assert!(!is_secret_name(name), "{name}");
        }
        for value in ["sk-abc123", "github ghp_16C7e42F", "Authorization: Bearer abc"] {
            assert!(looks_like_token(value), "{value}");
        }
        for value in ["task-12345678", "risk-based", "Tu es Discord.", "swarm-control 0.1.2"] {
            assert!(!looks_like_token(value), "{value}");
        }
        // Les références `$VAR` sous une clé de secret passent (pas une valeur).
        assert!(
            definition_of("a", "custom", &json!({ "prompt": "p", "env": { "TOKEN": "$REF" } }))
                .is_ok()
        );
    }

    #[test]
    fn write_ne_reecrit_pas_un_contenu_identique() {
        let dir = TempDir::new("write");
        let file = dir.0.join("f");
        write(&file, "a").unwrap();
        let before = std::fs::metadata(&file).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        write(&file, "a").unwrap();
        assert_eq!(std::fs::metadata(&file).unwrap().modified().unwrap(), before);
        write(&file, "b").unwrap();
        assert_ne!(std::fs::metadata(&file).unwrap().modified().unwrap(), before);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "b");
    }

    #[test]
    fn sans_cargo_le_message_donner_le_lien_rustup() {
        let message = super::no_cargo_message();
        assert!(message.contains("https://rustup.rs"), "{message}");
        assert!(message.contains("cargo"), "{message}");
    }
}
