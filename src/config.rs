//! Configuration globale : workspace, variables d'environnement, chemins Hermes.
//!
//! Même résolution que l'OverMind TypeScript, pour relire les mêmes fichiers :
//!
//! 1. `--workspace` / `OVERMIND_WORKSPACE` (environnement du process) ;
//! 2. `OVERMIND_WORKSPACE` lu dans `~/.overmind/.env` ;
//! 3. premier dossier contenant `.mcp.json` / `.mcp.local.json` en remontant
//!    depuis le répertoire courant ;
//! 4. `~/.overmind`.
//!
//! Les fichiers `.env` ne modifient jamais l'environnement du process : leurs
//! valeurs sont gardées dans une table (`Config::dotenv`) qui complète l'env
//! du système **sans l'écraser** (sémantique dotenv classique), puis sont
//! transmises aux agents lancés.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

/// Délai par défaut d'un run d'agent (15 min, comme `TIMEOUT_MS` côté TS).
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15 * 60);

pub struct Config {
    /// Racine Overmind : `.claude/`, `.mcp.json`, `bridge/`.
    pub workspace: PathBuf,
    /// Variables lues dans les fichiers `.env` et absentes de l'env système.
    pub dotenv: HashMap<String, String>,
}

static CONFIG: OnceLock<Config> = OnceLock::new();

pub fn init(workspace_override: Option<PathBuf>) -> &'static Config {
    CONFIG.get_or_init(|| {
        let mut dotenv = HashMap::new();
        let home = home_dir();
        load_dotenv_into(&home.join(".overmind").join(".env"), &mut dotenv);

        let workspace = workspace_override
            .or_else(|| std::env::var_os("OVERMIND_WORKSPACE").map(PathBuf::from))
            .or_else(|| dotenv.get("OVERMIND_WORKSPACE").map(|raw| expand_tilde(raw, &home)))
            .or_else(find_mcp_root)
            .unwrap_or_else(|| home.join(".overmind"));
        let _ = std::fs::create_dir_all(&workspace);

        load_dotenv_into(&workspace.join(".env"), &mut dotenv);
        if let Some(external) = lookup_in(&dotenv, "OVERMIND_EXTERNAL_ENV_PATH") {
            load_dotenv_into(Path::new(&external), &mut dotenv);
        }
        Config { workspace, dotenv }
    })
}

pub fn get() -> &'static Config {
    CONFIG.get().expect("config::init doit être appelé au démarrage")
}

/// Workspace d'un appel : le paramètre `config` de l'outil s'il est fourni,
/// sinon le workspace global.
pub fn workspace_for(override_dir: Option<&str>) -> PathBuf {
    match override_dir.map(str::trim).filter(|s| !s.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => get().workspace.clone(),
    }
}

/// Valeur d'une variable : env système d'abord, puis fichiers `.env`.
pub fn env_var(name: &str) -> Option<String> {
    match std::env::var(name) {
        Ok(v) => Some(v),
        Err(_) => CONFIG.get().and_then(|c| c.dotenv.get(name).cloned()),
    }
}

fn lookup_in(dotenv: &HashMap<String, String>, name: &str) -> Option<String> {
    std::env::var(name).ok().or_else(|| dotenv.get(name).cloned())
}

/// Remplace `$VAR` et `${VAR}` par leur valeur (vide si absente).
pub fn interpolate(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' {
            let (name, consumed) = if bytes.get(i + 1) == Some(&b'{') {
                match input[i + 2..].find('}') {
                    Some(end) => (&input[i + 2..i + 2 + end], end + 3),
                    None => ("", 0),
                }
            } else {
                let len = input[i + 1..]
                    .bytes()
                    .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_')
                    .count();
                (&input[i + 1..i + 1 + len], len + 1)
            };
            if !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
                out.push_str(&env_var(name).unwrap_or_default());
                i += consumed;
                continue;
            }
        }
        let ch = input[i..].chars().next().expect("index sur une frontière de caractère");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Interpole récursivement toutes les chaînes d'une valeur JSON.
pub fn interpolate_json(value: &serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match value {
        Value::String(s) => Value::String(interpolate(s)),
        Value::Array(items) => Value::Array(items.iter().map(interpolate_json).collect()),
        Value::Object(map) => Value::Object(
            map.iter().map(|(k, v)| (k.clone(), interpolate_json(v))).collect(),
        ),
        other => other.clone(),
    }
}

/// Home Hermes **natif** : là où le CLI `hermes` lit ses profils
/// (`%LOCALAPPDATA%\hermes` sous Windows, `~/.hermes` ailleurs).
pub fn native_hermes_home() -> PathBuf {
    if let Some(h) = env_var("HERMES_HOME").filter(|s| !s.is_empty()) {
        return PathBuf::from(h);
    }
    if cfg!(windows) {
        let base = std::env::var_os("LOCALAPPDATA")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .unwrap_or_else(home_dir);
        base.join("hermes")
    } else {
        home_dir().join(".hermes")
    }
}

/// Délai d'un run : `OVERMIND_TIMEOUT_MS` si défini, sinon 15 min.
pub fn default_timeout() -> Duration {
    env_var("OVERMIND_TIMEOUT_MS")
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or(DEFAULT_TIMEOUT)
}

pub fn home_dir() -> PathBuf {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn expand_tilde(raw: &str, home: &Path) -> PathBuf {
    if let Some(rest) = raw.strip_prefix("~/").or_else(|| raw.strip_prefix("~\\")) {
        home.join(rest)
    } else if let Some(rest) = raw.strip_prefix('~') {
        home.join(rest)
    } else {
        PathBuf::from(raw)
    }
}

fn find_mcp_root() -> Option<PathBuf> {
    let mut current = std::env::current_dir().ok()?;
    loop {
        if current.join(".mcp.json").exists() || current.join(".mcp.local.json").exists() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

/// Charge un `.env` : n'ajoute que les clés absentes de l'env système et de
/// la table (le premier fichier lu gagne, comme `dotenv` sans `override`).
fn load_dotenv_into(path: &Path, into: &mut HashMap<String, String>) {
    let Ok(content) = std::fs::read_to_string(path) else {
        return;
    };
    for (key, value) in parse_dotenv(&content) {
        if std::env::var_os(&key).is_none() && !into.contains_key(&key) {
            into.insert(key, value);
        }
    }
}

pub fn parse_dotenv(content: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            continue;
        }
        let mut value = value.trim();
        let quoted = value.len() >= 2
            && ((value.starts_with('"') && value.ends_with('"'))
                || (value.starts_with('\'') && value.ends_with('\'')));
        if quoted {
            value = &value[1..value.len() - 1];
        } else if let Some(idx) = value.find(" #") {
            value = value[..idx].trim_end();
        }
        out.push((key.to_string(), value.to_string()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dotenv_parsing() {
        let parsed = parse_dotenv("# c\nA=1\nexport B=\"deux mots\"\nC='x' \nD=v # note\nbad line\n");
        assert_eq!(
            parsed,
            vec![
                ("A".into(), "1".into()),
                ("B".into(), "deux mots".into()),
                ("C".into(), "x".into()),
                ("D".into(), "v".into()),
            ]
        );
    }

    #[test]
    fn interpolation_keeps_unrelated_dollars() {
        // Sans config initialisée : seules les variables système comptent.
        assert_eq!(interpolate("prix 5$ et $"), "prix 5$ et $");
        assert_eq!(interpolate("${OVERMIND_TEST_SURELY_ABSENT}x"), "x");
        assert_eq!(interpolate("é$OVERMIND_TEST_SURELY_ABSENT-é"), "é-é");
    }
}
