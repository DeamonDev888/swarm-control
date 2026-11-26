//! Registre des agents sur disque — mêmes fichiers que l'OverMind TS.
//!
//! **Claude** (dans `<workspace>/.claude/`) :
//! - `agents/<nom>.md`            → prompt système (persona)
//! - `settings_<nom>.json`        → `{ env, enabledMcpjsonServers, agent, runner }`
//! - `.mcp.<nom>.json`            → serveurs MCP de l'agent (extrait de `.mcp.json`)
//!
//! **Hermes** (dans le home natif, `…/hermes/profiles/<nom>/`) : profil géré
//! par le CLI `hermes` (`profile create`, `config set`) + `SOUL.md`,
//! `config.yaml`, `.env`. La présence d'un profil se vérifie sur disque —
//! pas de lancement de Python pour un simple `list`.

use crate::config;
use crate::process;
use crate::registry;
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub fn validate_name(name: &str) -> Result<(), String> {
    let ok = !name.is_empty()
        && name.len() <= 128
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    if ok {
        Ok(())
    } else {
        Err(format!(
            "Nom d'agent invalide '{name}' : seuls [a-zA-Z0-9_-] sont autorisés (1 à 128 caractères)."
        ))
    }
}

/// Règle des profils Hermes (`PROFILE_ID_RE` côté Hermes) : minuscules,
/// chiffres, `_` et `-`, 64 caractères max, premier caractère alphanumérique.
pub fn validate_hermes_name(name: &str) -> Result<(), String> {
    let b = name.as_bytes();
    let ok = !b.is_empty()
        && b.len() <= 64
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_' || *c == b'-');
    if ok {
        Ok(())
    } else {
        Err(format!(
            "Nom de profil Hermes invalide '{name}' : minuscules/chiffres/_/- uniquement, 64 max, sans _ ou - au début."
        ))
    }
}

// ─── Chemins ────────────────────────────────────────────────────────────────

fn claude_dir(ws: &Path) -> PathBuf {
    ws.join(".claude")
}
pub fn claude_prompt_path(ws: &Path, name: &str) -> PathBuf {
    claude_dir(ws).join("agents").join(format!("{name}.md"))
}
pub fn claude_settings_path(ws: &Path, name: &str) -> PathBuf {
    claude_dir(ws).join(format!("settings_{name}.json"))
}
pub fn claude_mcp_path(ws: &Path, name: &str) -> PathBuf {
    claude_dir(ws).join(format!(".mcp.{name}.json"))
}
pub fn global_mcp_path(ws: &Path) -> PathBuf {
    let primary = ws.join(".mcp.json");
    if primary.exists() {
        return primary;
    }
    let local = ws.join(".mcp.local.json");
    if local.exists() { local } else { primary }
}
pub fn hermes_profile_dir(name: &str) -> PathBuf {
    config::native_hermes_home().join("profiles").join(name)
}

// ─── Découverte ─────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Runner {
    Claude,
    Hermes,
}

impl Runner {
    pub fn as_str(self) -> &'static str {
        match self {
            Runner::Claude => "claude",
            Runner::Hermes => "hermes",
        }
    }
    pub fn parse(s: &str) -> Option<Runner> {
        match s {
            "claude" => Some(Runner::Claude),
            "hermes" => Some(Runner::Hermes),
            _ => None,
        }
    }
}

pub fn hermes_profile_exists(name: &str) -> bool {
    hermes_profile_dir(name).join("config.yaml").is_file()
}

/// Runner effectif d'un agent : profil Hermes d'abord, sinon settings Claude.
/// Un `settings_<nom>.json` dont `runner` vaut `hermes` mais sans profil
/// Hermes correspondant est traité comme Claude (c'est le seul qui puisse
/// réellement le lancer).
pub fn peek_runner(ws: &Path, name: &str) -> Option<Runner> {
    if hermes_profile_exists(name) {
        return Some(Runner::Hermes);
    }
    if claude_settings_path(ws, name).is_file() || claude_prompt_path(ws, name).is_file() {
        return Some(Runner::Claude);
    }
    None
}

/// L'agent existe-t-il pour **ce** runner ? (Un même nom peut exister côté
/// Claude et côté Hermes : chacun a ses fichiers et sa session.)
pub fn exists_for(ws: &Path, name: &str, runner: Runner) -> bool {
    match runner {
        Runner::Hermes => hermes_profile_exists(name),
        Runner::Claude => claude_settings_path(ws, name).is_file(),
    }
}

pub struct AgentInfo {
    pub name: String,
    pub runner: Runner,
    pub model: String,
    pub mcp_servers: Vec<String>,
    pub prompt_bytes: u64,
    pub missing_config: bool,
}

pub fn list(ws: &Path) -> Vec<AgentInfo> {
    let mut out = Vec::new();
    // Claude : agents/*.md (hors *_skill.md)
    if let Ok(entries) = std::fs::read_dir(claude_dir(ws).join("agents")) {
        for entry in entries.flatten() {
            let file = entry.file_name().to_string_lossy().into_owned();
            let Some(name) = file.strip_suffix(".md") else { continue };
            if name.ends_with("_skill") || validate_name(name).is_err() {
                continue;
            }
            let settings = crate::store::read_json(&claude_settings_path(ws, name));
            let model = settings
                .as_ref()
                .map(config::interpolate_json)
                .and_then(|s| {
                    s.pointer("/env/ANTHROPIC_MODEL")
                        .or_else(|| s.get("model"))
                        .and_then(Value::as_str)
                        .map(String::from)
                })
                .unwrap_or_else(|| "settings-default".into());
            let mcp_servers = settings
                .as_ref()
                .and_then(|s| s.get("enabledMcpjsonServers"))
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();
            out.push(AgentInfo {
                name: name.to_string(),
                runner: Runner::Claude,
                model,
                mcp_servers,
                prompt_bytes: entry.metadata().map(|m| m.len()).unwrap_or(0),
                missing_config: settings.is_none(),
            });
        }
    }
    // Hermes : profiles/*/config.yaml
    if let Ok(entries) = std::fs::read_dir(config::native_hermes_home().join("profiles")) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let dir = entry.path();
            if !dir.join("config.yaml").is_file() || validate_name(&name).is_err() {
                continue;
            }
            if out.iter().any(|a| a.name == name) {
                continue;
            }
            let yaml = std::fs::read_to_string(dir.join("config.yaml")).unwrap_or_default();
            let soul = std::fs::metadata(dir.join("SOUL.md"));
            out.push(AgentInfo {
                runner: Runner::Hermes,
                model: yaml_model(&yaml).unwrap_or_else(|| "unknown".into()),
                mcp_servers: yaml_mcp_servers(&yaml),
                prompt_bytes: soul.as_ref().map(|m| m.len()).unwrap_or(0),
                missing_config: soul.is_err(),
                name,
            });
        }
    }
    out.sort_by(|a, b| (a.runner.as_str(), &a.name).cmp(&(b.runner.as_str(), &b.name)));
    out
}

/// `model.default` du `config.yaml` Hermes (lecture ligne à ligne, sans parseur YAML).
fn yaml_model(yaml: &str) -> Option<String> {
    let mut in_model = false;
    for line in yaml.lines() {
        if !line.starts_with(' ') && !line.starts_with('\t') {
            in_model = line.trim_end() == "model:";
            continue;
        }
        if in_model {
            if let Some(v) = line.trim().strip_prefix("default:") {
                return Some(v.trim().trim_matches(['"', '\'']).to_string());
            }
        }
    }
    None
}

/// Noms des serveurs de la section `mcp_servers:` du `config.yaml`.
fn yaml_mcp_servers(yaml: &str) -> Vec<String> {
    let mut in_block = false;
    let mut out = Vec::new();
    for line in yaml.lines() {
        if !line.starts_with(' ') && !line.starts_with('\t') && !line.trim().is_empty() {
            in_block = line.trim_end() == "mcp_servers:";
            continue;
        }
        if in_block {
            let indent = line.len() - line.trim_start().len();
            let t = line.trim();
            if indent == 2 && t.ends_with(':') && !t.starts_with('-') {
                out.push(t.trim_end_matches(':').to_string());
            }
        }
    }
    out
}

// ─── Création ───────────────────────────────────────────────────────────────

pub struct CreateArgs<'a> {
    pub name: &'a str,
    pub prompt: &'a str,
    pub runner: Runner,
    pub model: Option<&'a str>,
    pub copy_env_from: Option<&'a str>,
    /// Hermes : profil source dont copier config.yaml, .env, SOUL.md et skills.
    pub clone_from: Option<&'a str>,
}

pub struct Created {
    pub prompt_path: PathBuf,
    pub config_path: PathBuf,
    pub warnings: Vec<String>,
}

pub fn create(ws: &Path, args: &CreateArgs) -> Result<Created, String> {
    validate_name(args.name)?;
    match args.runner {
        Runner::Claude => create_claude(ws, args),
        Runner::Hermes => create_hermes(ws, args),
    }
}

fn create_claude(ws: &Path, args: &CreateArgs) -> Result<Created, String> {
    let model = args
        .model
        .map(String::from)
        .or_else(|| config::env_var("ANTHROPIC_MODEL"))
        .unwrap_or_else(|| "claude-sonnet-4-6".into());

    let mut env: Map<String, Value> = Map::new();
    env.insert("ANTHROPIC_MODEL".into(), json!(model));
    if let Some(token) = anthropic_token() {
        env.insert("ANTHROPIC_AUTH_TOKEN".into(), json!(token));
    }
    env.insert(
        "ANTHROPIC_BASE_URL".into(),
        json!(config::env_var("ANTHROPIC_BASE_URL").unwrap_or_else(|| "https://api.anthropic.com".into())),
    );
    env.insert(
        "AGENT_TIMEOUT_MS".into(),
        json!(config::env_var("AGENT_TIMEOUT_MS").unwrap_or_else(|| "3000000".into())),
    );
    env.insert("agent".into(), json!(args.name));

    let all_servers = mcp_server_names(ws);
    let mut servers = all_servers.clone();
    if let Some(rel) = args.copy_env_from {
        let source = resolve_inside(ws, rel)?;
        let content = std::fs::read_to_string(&source)
            .map_err(|e| format!("copyEnvFrom illisible ({}): {e}", source.display()))?;
        let parsed: Value = serde_json::from_str(content.trim_start_matches('\u{feff}'))
            .map_err(|e| format!("copyEnvFrom n'est pas du JSON valide: {e}"))?;
        if let Some(src_env) = parsed.get("env").and_then(Value::as_object) {
            for (k, v) in src_env {
                env.insert(k.clone(), v.clone());
            }
        }
        if let Some(list) = parsed.get("enabledMcpjsonServers").and_then(Value::as_array) {
            servers = list.iter().filter_map(|v| v.as_str().map(String::from)).collect();
        }
    }
    env.insert("ANTHROPIC_MODEL".into(), json!(model));

    // Sans aucun jeton (ni env, ni .env, ni copyEnvFrom) l'agent ne pourra pas
    // s'authentifier : on refuse plutôt que de créer un agent inutilisable.
    let has_token = ["ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_API_KEY"]
        .iter()
        .any(|k| env.get(*k).and_then(Value::as_str).is_some_and(|s| !s.is_empty()));
    if !has_token {
        return Err("MISSING_AUTH_TOKEN: ANTHROPIC_AUTH_TOKEN (ou ANTHROPIC_AUTH_TOKEN_<N>) absent de \
                    l'environnement et des .env — ou passe copyEnvFrom vers un settings existant."
            .into());
    }

    let prompt_path = claude_prompt_path(ws, args.name);
    std::fs::create_dir_all(prompt_path.parent().expect("agents/ a un parent")).map_err(io)?;
    std::fs::write(&prompt_path, args.prompt).map_err(io)?;

    let settings = json!({
        "env": env,
        "enableAllProjectMcpServers": false,
        "enabledMcpjsonServers": servers,
        "agent": args.name,
        "runner": "claude",
    });
    let settings_path = claude_settings_path(ws, args.name);
    crate::store::write_json_atomic(&settings_path, &settings).map_err(io)?;
    write_agent_mcp(ws, args.name, &servers)?;
    Ok(Created { prompt_path, config_path: settings_path, warnings: Vec::new() })
}

/// Jeton Anthropic : `ANTHROPIC_AUTH_TOKEN`, sinon le premier `ANTHROPIC_AUTH_TOKEN_<N>`.
fn anthropic_token() -> Option<String> {
    if let Some(t) = config::env_var("ANTHROPIC_AUTH_TOKEN").filter(|s| !s.is_empty()) {
        return Some(t);
    }
    let mut keys: Vec<String> = std::env::vars()
        .map(|(k, _)| k)
        .chain(config::get().dotenv.keys().cloned())
        .filter(|k| {
            k.strip_prefix("ANTHROPIC_AUTH_TOKEN_")
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        })
        .collect();
    keys.sort();
    keys.dedup();
    keys.iter().find_map(|k| config::env_var(k).filter(|s| !s.is_empty()))
}

fn create_hermes(ws: &Path, args: &CreateArgs) -> Result<Created, String> {
    validate_hermes_name(args.name)?;
    if let Some(src) = args.clone_from {
        validate_hermes_name(src)?;
        if !hermes_profile_exists(src) {
            return Err(format!("cloneFrom: le profil Hermes '{src}' n'existe pas."));
        }
    }
    let model = args.model.unwrap_or("MiniMax-M3");
    let provider = detect_provider(model);
    let description = format!("Overmind agent: {}", args.name);

    // Le profil est créé par le CLI Hermes lui-même (qui connaît sa structure).
    let mut create: Vec<&str> = vec!["profile", "create", args.name, "--no-alias", "--description", &description];
    if let Some(src) = args.clone_from {
        create.extend(["--clone-from", src]);
    }
    if let Err(e) = hermes_cli(&create) {
        if !e.contains("already exists") {
            return Err(format!("hermes profile create a échoué: {e}"));
        }
    }
    // Ordre important : `model.default` puis `model.provider` (provider en dernier).
    hermes_cli(&["-p", args.name, "config", "set", "model.default", model])?;
    hermes_cli(&["-p", args.name, "config", "set", "model.provider", provider])?;

    let dir = hermes_profile_dir(args.name);
    if !dir.is_dir() {
        return Err(format!(
            "Profil Hermes introuvable après création ({}). HERMES_HOME pointe-t-il au bon endroit ?",
            dir.display()
        ));
    }
    let soul = dir.join("SOUL.md");
    std::fs::write(&soul, args.prompt).map_err(io)?;
    let servers = mcp_server_names(ws);
    if !servers.is_empty() {
        // Les serveurs du .mcp.json sont écrits dans config.yaml (section mcp_servers).
        let _ = set_hermes_mcp(ws, &dir, &servers);
    }
    let mut warnings = Vec::new();
    if !has_credentials(&dir, provider) {
        warnings.push(format!(
            "Aucune credential détectée pour le provider '{provider}' dans ce profil (les credentials \
             Hermes sont PAR PROFIL) : les runs échoueront avec « No usable credentials found ». \
             Ajoute-en une : `hermes -p {name} auth add {provider}`, ou recrée l'agent avec \
             cloneFrom:<profil qui a déjà ses clés>.",
            name = args.name
        ));
    }
    Ok(Created { prompt_path: soul, config_path: dir.join("config.yaml"), warnings })
}

/// Heuristique : le profil a-t-il une credential pour `provider` ?
/// (pool de `auth.json`, entrée `providers`, ou un `.env` non vide).
fn has_credentials(profile_dir: &Path, provider: &str) -> bool {
    if let Some(auth) = crate::store::read_json(&profile_dir.join("auth.json")) {
        let in_pool = auth
            .pointer(&format!("/credential_pool/{provider}"))
            .and_then(Value::as_array)
            .is_some_and(|l| !l.is_empty());
        if in_pool || auth.pointer(&format!("/providers/{provider}")).is_some() {
            return true;
        }
    }
    std::fs::read_to_string(profile_dir.join(".env"))
        .map(|c| c.lines().any(|l| l.contains('=') && !l.trim_start().starts_with('#')))
        .unwrap_or(false)
}

fn detect_provider(model: &str) -> &'static str {
    let m = model.to_lowercase();
    if m.contains("glm") || m.contains("zai") {
        "zai"
    } else if m.contains("minimax") {
        "minimax-cn"
    } else if m.contains("claude") || m.contains("sonnet") || m.contains("opus") || m.contains("haiku") {
        "anthropic"
    } else if m.contains("gpt") || m.starts_with("o1") || m.starts_with("o3") {
        "openai-api"
    } else if m.contains("deepseek") {
        "deepseek"
    } else if m.contains("gemini") {
        "gemini"
    } else if m.contains("qwen") {
        "qwen-oauth"
    } else if m.contains("grok") || m.contains("xai") {
        "xai"
    } else {
        "openai-api"
    }
}

/// Exécute `hermes <args>` avec le home natif. `Err` = sortie d'erreur.
fn hermes_cli(args: &[&str]) -> Result<String, String> {
    let mut cmd = process::command(&crate::runners::hermes_binary());
    cmd.args(args)
        .env("HERMES_HOME", config::native_hermes_home())
        .env("PYTHONIOENCODING", "utf-8")
        .stdin(std::process::Stdio::null());
    let output = run_with_timeout(cmd, Duration::from_secs(120))?;
    if output.0 {
        Ok(output.1)
    } else {
        Err(output.1.trim().to_string())
    }
}

/// (succès, stdout+stderr). Délai dur pour ne jamais bloquer le serveur.
fn run_with_timeout(mut cmd: std::process::Command, timeout: Duration) -> Result<(bool, String), String> {
    use std::io::Read;
    cmd.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("lancement impossible: {e}"))?;
    let mut so = child.stdout.take();
    let mut se = child.stderr.take();
    let t_out = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(r) = so.as_mut() {
            let mut b = Vec::new();
            let _ = r.read_to_end(&mut b);
            s = String::from_utf8_lossy(&b).into_owned();
        }
        s
    });
    let t_err = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(r) = se.as_mut() {
            let mut b = Vec::new();
            let _ = r.read_to_end(&mut b);
            s = String::from_utf8_lossy(&b).into_owned();
        }
        s
    });
    let start = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if start.elapsed() < timeout => std::thread::sleep(Duration::from_millis(50)),
            Ok(None) => {
                process::kill_tree(child.id());
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Err(e) => return Err(e.to_string()),
        }
    };
    let out = t_out.join().unwrap_or_default();
    let err = t_err.join().unwrap_or_default();
    match status {
        Some(s) => Ok((s.success(), format!("{out}{err}"))),
        None => Err(format!("délai dépassé ({}s)", timeout.as_secs())),
    }
}

// ─── Mise à jour ────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct Updates {
    pub model: Option<String>,
    pub mcp_servers: Option<Vec<String>>,
    pub env: Option<Map<String, Value>>,
    pub prompt: Option<String>,
    /// Réécriture complète d'un fichier : (`prompt.md`|`settings.json`|`.mcp.json`|`skill.md`, contenu).
    pub file: Option<(String, String)>,
}

/// Runner visé : celui demandé (s'il existe pour cet agent), sinon l'unique
/// runner possible ; ambigu (Claude **et** Hermes) → erreur explicite.
pub fn resolve(ws: &Path, name: &str, wanted: Option<Runner>) -> Result<Runner, String> {
    validate_name(name)?;
    match wanted {
        Some(r) if exists_for(ws, name, r) => Ok(r),
        Some(r) => Err(format!("Agent '{name}' introuvable pour le runner {}.", r.as_str())),
        None => {
            let claude = exists_for(ws, name, Runner::Claude) || claude_prompt_path(ws, name).is_file();
            let hermes = exists_for(ws, name, Runner::Hermes);
            match (claude, hermes) {
                (true, true) => Err(format!(
                    "Agent '{name}' existe côté claude ET hermes : précise runner."
                )),
                (true, false) => Ok(Runner::Claude),
                (false, true) => Ok(Runner::Hermes),
                (false, false) => Err(format!("Agent '{name}' introuvable.")),
            }
        }
    }
}

pub fn update(ws: &Path, name: &str, wanted: Option<Runner>, up: &Updates) -> Result<Vec<String>, String> {
    let runner = resolve(ws, name, wanted)?;
    let mut changes = Vec::new();
    match runner {
        Runner::Hermes => update_hermes(ws, name, up, &mut changes)?,
        Runner::Claude => update_claude(ws, name, up, &mut changes)?,
    }
    if changes.is_empty() {
        changes.push("Aucune modification demandée.".into());
    }
    Ok(changes)
}

fn update_hermes(ws: &Path, name: &str, up: &Updates, changes: &mut Vec<String>) -> Result<(), String> {
    let dir = hermes_profile_dir(name);
    if let Some((file, content)) = &up.file {
        let target = match file.as_str() {
            "prompt.md" => dir.join("SOUL.md"),
            "settings.json" => dir.join("config.yaml"),
            ".mcp.json" => dir.join("mcp.json"),
            "skill.md" => dir.join("skills").join("skill.md"),
            other => return Err(format!("Fichier non supporté: {other}")),
        };
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(io)?;
        }
        std::fs::write(&target, content).map_err(io)?;
        changes.push(format!("{file} réécrit → {}", target.display()));
        return Ok(());
    }
    if let Some(model) = &up.model {
        hermes_cli(&["-p", name, "config", "set", "model.default", model])?;
        hermes_cli(&["-p", name, "config", "set", "model.provider", detect_provider(model)])?;
        changes.push(format!("Modèle → {model}"));
    }
    if let Some(prompt) = &up.prompt {
        std::fs::write(dir.join("SOUL.md"), prompt).map_err(io)?;
        changes.push("SOUL.md mis à jour".into());
    }
    if let Some(env) = &up.env {
        let path = dir.join(".env");
        let mut content = std::fs::read_to_string(&path).unwrap_or_default();
        for (k, v) in env {
            if !k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') || k.is_empty() {
                return Err(format!("Nom de variable invalide: {k}"));
            }
            let value = v.as_str().unwrap_or_default();
            if value.contains(['\n', '\r']) {
                return Err(format!("Valeur multiligne refusée pour {k}"));
            }
            let prefix = format!("{k}=");
            let mut found = false;
            let mut lines: Vec<String> = content
                .lines()
                .map(|l| {
                    if l.starts_with(&prefix) {
                        found = true;
                        format!("{k}={value}")
                    } else {
                        l.to_string()
                    }
                })
                .collect();
            if !found {
                lines.push(format!("{k}={value}"));
            }
            content = lines.join("\n") + "\n";
            changes.push(format!("Env '{k}' → {}", if value.is_empty() { "(vide)" } else { "***" }));
        }
        std::fs::write(&path, content).map_err(io)?;
    }
    if let Some(servers) = &up.mcp_servers {
        set_hermes_mcp(ws, &dir, servers)?;
        changes.push(format!("MCPs : [{}]", servers.join(", ")));
    }
    Ok(())
}

fn update_claude(ws: &Path, name: &str, up: &Updates, changes: &mut Vec<String>) -> Result<(), String> {
    if let Some((file, content)) = &up.file {
        let target = match file.as_str() {
            "prompt.md" => claude_prompt_path(ws, name),
            "settings.json" => claude_settings_path(ws, name),
            ".mcp.json" => claude_mcp_path(ws, name),
            "skill.md" => claude_dir(ws).join("agents").join(format!("{name}_skill.md")),
            other => return Err(format!("Fichier non supporté: {other}")),
        };
        if file == "settings.json" || file == ".mcp.json" {
            serde_json::from_str::<Value>(content).map_err(|e| format!("{file} n'est pas du JSON valide: {e}"))?;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(io)?;
        }
        std::fs::write(&target, content).map_err(io)?;
        changes.push(format!("{file} réécrit → {}", target.display()));
        return Ok(());
    }
    if let Some(prompt) = &up.prompt {
        let path = claude_prompt_path(ws, name);
        std::fs::create_dir_all(path.parent().expect("agents/ a un parent")).map_err(io)?;
        std::fs::write(&path, prompt).map_err(io)?;
        changes.push("Prompt mis à jour".into());
    }
    if up.model.is_some() || up.env.is_some() || up.mcp_servers.is_some() {
        let path = claude_settings_path(ws, name);
        let mut settings = crate::store::read_json(&path)
            .ok_or_else(|| format!("settings_{name}.json introuvable ou invalide."))?;
        if !settings.is_object() {
            return Err(format!("settings_{name}.json n'est pas un objet JSON."));
        }
        let env = settings
            .as_object_mut()
            .expect("objet vérifié")
            .entry("env")
            .or_insert_with(|| json!({}));
        if !env.is_object() {
            *env = json!({});
        }
        let env = env.as_object_mut().expect("objet vérifié");
        if let Some(model) = &up.model {
            let old = env.get("ANTHROPIC_MODEL").and_then(Value::as_str).unwrap_or("(aucun)").to_string();
            env.insert("ANTHROPIC_MODEL".into(), json!(model));
            changes.push(format!("Modèle : {old} → {model}"));
        }
        if let Some(extra) = &up.env {
            for (k, v) in extra {
                env.insert(k.clone(), v.clone());
                let shown = if v.as_str().is_some_and(str::is_empty) { "(vide)" } else { "***" };
                changes.push(format!("Env '{k}' → {shown}"));
            }
        }
        if let Some(servers) = &up.mcp_servers {
            let known = mcp_server_names(ws);
            let unknown: Vec<&String> = servers.iter().filter(|s| !known.contains(s)).collect();
            if !unknown.is_empty() && !known.is_empty() {
                changes.push(format!(
                    "ATTENTION: serveurs absents de .mcp.json: {} (valides: {})",
                    unknown.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "),
                    known.join(", ")
                ));
            }
            settings["enabledMcpjsonServers"] = json!(servers);
            write_agent_mcp(ws, name, servers)?;
            changes.push(format!("Serveurs MCP → [{}]", servers.join(", ")));
        }
        crate::store::write_json_atomic(&path, &settings).map_err(io)?;
    }
    Ok(())
}

// ─── Suppression ────────────────────────────────────────────────────────────

pub struct Deleted {
    pub files: Vec<String>,
    pub errors: Vec<String>,
}

pub fn delete(ws: &Path, name: &str, wanted: Option<Runner>) -> Result<Deleted, String> {
    let runner = resolve(ws, name, wanted)?;
    let mut out = Deleted { files: Vec::new(), errors: Vec::new() };
    if let Some(run) = registry::find(name, Some(runner.as_str())) {
        if run.snapshot().status == registry::Status::Running {
            return Err(format!("L'agent '{name}' a un run en cours : agent_control(kill) d'abord."));
        }
    }
    match runner {
        Runner::Hermes => {
            let res = hermes_cli(&["profile", "delete", name, "--yes"]);
            // Hermes peut sortir en code non nul (arrêt du gateway, alias…) alors
            // que le profil est bien supprimé : le disque fait foi.
            if hermes_profile_dir(name).exists() {
                let why = res.err().unwrap_or_else(|| "le dossier existe encore".into());
                out.errors.push(format!("hermes profile delete: {}", why.lines().last().unwrap_or("")));
            } else {
                out.files.push(format!("profil hermes: {name}"));
            }
        }
        Runner::Claude => {
            let skill = claude_dir(ws).join("agents").join(format!("{name}_skill.md"));
            for path in [
                claude_prompt_path(ws, name),
                claude_settings_path(ws, name),
                claude_mcp_path(ws, name),
                skill,
            ] {
                match std::fs::remove_file(&path) {
                    Ok(()) => out.files.push(path.display().to_string()),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => out.errors.push(format!("{}: {e}", path.display())),
                }
            }
        }
    }
    let runner_name = runner.as_str();
    crate::sessions::delete(ws, runner_name, name);
    Ok(out)
}

// ─── Lecture complète ───────────────────────────────────────────────────────

pub fn get_configs(ws: &Path, name: &str, wanted: Option<Runner>) -> Result<Vec<(String, String)>, String> {
    let runner = resolve(ws, name, wanted)?;
    let read = |p: PathBuf| std::fs::read_to_string(p).unwrap_or_else(|_| "MISSING".into());
    let mut out = Vec::new();
    match runner {
        Runner::Hermes => {
            let dir = hermes_profile_dir(name);
            out.push(("settings.json".into(), read(dir.join("config.yaml"))));
            out.push(("prompt.md".into(), read(dir.join("SOUL.md"))));
            let mcp = read(dir.join("mcp.json"));
            out.push((".mcp.json".into(), mcp));
            // Les secrets du .env sont masqués : on n'expose que les noms.
            out.push((".env".into(), mask_env(&read(dir.join(".env")))));
        }
        Runner::Claude => {
            out.push(("prompt.md".into(), read(claude_prompt_path(ws, name))));
            let settings = read(claude_settings_path(ws, name));
            out.push(("settings.json".into(), mask_settings(&settings)));
            out.push((".mcp.json".into(), read(claude_mcp_path(ws, name))));
            let skill = claude_dir(ws).join("agents").join(format!("{name}_skill.md"));
            if skill.is_file() {
                out.push(("skill.md".into(), read(skill)));
            }
        }
    }
    Ok(out)
}

fn mask_env(content: &str) -> String {
    content
        .lines()
        .map(|l| match l.split_once('=') {
            Some((k, v)) if !k.trim_start().starts_with('#') && !v.trim().is_empty() => format!("{k}=***"),
            _ => l.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn mask_settings(raw: &str) -> String {
    let Ok(mut v) = serde_json::from_str::<Value>(raw) else { return raw.to_string() };
    if let Some(env) = v.get_mut("env").and_then(Value::as_object_mut) {
        for (k, val) in env.iter_mut() {
            let upper = k.to_uppercase();
            let secret = ["TOKEN", "KEY", "SECRET", "PASSWORD"].iter().any(|w| upper.contains(w));
            // Les références `$VAR` ne sont pas des secrets : on les garde lisibles.
            if secret && val.as_str().is_some_and(|s| !s.is_empty() && !s.starts_with('$')) {
                *val = json!("***");
            }
        }
    }
    serde_json::to_string_pretty(&v).unwrap_or_else(|_| raw.to_string())
}

// ─── MCP ────────────────────────────────────────────────────────────────────

pub fn mcp_server_names(ws: &Path) -> Vec<String> {
    crate::store::read_json(&global_mcp_path(ws))
        .and_then(|v| v.get("mcpServers").and_then(Value::as_object).map(|m| m.keys().cloned().collect()))
        .unwrap_or_default()
}

fn global_mcp_servers(ws: &Path) -> Map<String, Value> {
    crate::store::read_json(&global_mcp_path(ws))
        .and_then(|v| v.get("mcpServers").and_then(Value::as_object).cloned())
        .unwrap_or_default()
}

fn write_agent_mcp(ws: &Path, name: &str, servers: &[String]) -> Result<(), String> {
    let all = global_mcp_servers(ws);
    let picked: Map<String, Value> = servers
        .iter()
        .filter_map(|s| all.get(s).map(|cfg| (s.clone(), cfg.clone())))
        .collect();
    crate::store::write_json_atomic(&claude_mcp_path(ws, name), &json!({ "mcpServers": picked })).map_err(io)
}

/// Réécrit la section `mcp_servers:` du `config.yaml` Hermes.
fn set_hermes_mcp(ws: &Path, profile_dir: &Path, servers: &[String]) -> Result<(), String> {
    let all = global_mcp_servers(ws);
    let config_path = profile_dir.join("config.yaml");
    let current = std::fs::read_to_string(&config_path).unwrap_or_default();

    // Retire l'ancienne section (la ligne `mcp_servers:` et son bloc indenté).
    let mut kept = Vec::new();
    let mut skipping = false;
    for line in current.lines() {
        let top_level = !line.starts_with(' ') && !line.starts_with('\t') && !line.trim().is_empty();
        if top_level {
            skipping = line.trim_end() == "mcp_servers:";
            if skipping {
                continue;
            }
        } else if skipping {
            continue;
        }
        kept.push(line.to_string());
    }
    let mut body = kept.join("\n").trim_end().to_string();

    let mut section = Vec::new();
    for name in servers {
        let Some(cfg) = all.get(name) else { continue };
        if let Some(url) = cfg.get("url").and_then(Value::as_str) {
            section.push(format!("  {name}:"));
            section.push(format!("    url: {}", yaml_scalar(url)));
        } else if let Some(cmd) = cfg.get("command").and_then(Value::as_str) {
            section.push(format!("  {name}:"));
            section.push(format!("    command: {}", yaml_scalar(cmd)));
            if let Some(args) = cfg.get("args").and_then(Value::as_array).filter(|a| !a.is_empty()) {
                section.push("    args:".into());
                for a in args.iter().filter_map(Value::as_str) {
                    section.push(format!("    - {}", yaml_scalar(a)));
                }
            }
        }
    }
    if !section.is_empty() {
        body.push_str("\n\nmcp_servers:\n");
        body.push_str(&section.join("\n"));
    }
    body.push('\n');
    std::fs::write(&config_path, body).map_err(io)
}

/// Scalaire YAML sûr : guillemets doubles dès qu'un caractère spécial apparaît.
fn yaml_scalar(s: &str) -> String {
    let plain = !s.is_empty()
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"/._-:@+=%\\".contains(&b))
        && !s.starts_with(['-', ':', '@', '%']);
    if plain {
        s.to_string()
    } else {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

fn resolve_inside(ws: &Path, rel: &str) -> Result<PathBuf, String> {
    let p = Path::new(rel);
    if p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err(format!("copyEnvFrom sort du workspace: {rel}"));
    }
    Ok(if p.is_absolute() { p.to_path_buf() } else { ws.join(p) })
}

fn io(e: std::io::Error) -> String {
    e.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert!(validate_name("expert_python-2").is_ok());
        assert!(validate_name("../x").is_err());
        assert!(validate_name("").is_err());
        assert!(validate_name("a b").is_err());
    }

    #[test]
    fn yaml_helpers() {
        let y = "model:\n  default: glm-5.3-flash\n  provider: zai\nmcp_servers:\n  memory:\n    url: http://x\n  db:\n    command: node\n";
        assert_eq!(yaml_model(y).as_deref(), Some("glm-5.3-flash"));
        assert_eq!(yaml_mcp_servers(y), vec!["memory".to_string(), "db".to_string()]);
        assert_eq!(yaml_scalar("http://localhost:3099/mcp"), "http://localhost:3099/mcp");
        assert_eq!(yaml_scalar("a b"), "\"a b\"");
    }

    #[test]
    fn provider_detection() {
        assert_eq!(detect_provider("MiniMax-M3"), "minimax-cn");
        assert_eq!(detect_provider("glm-5.3-flash"), "zai");
        assert_eq!(detect_provider("claude-opus-4-7"), "anthropic");
        assert_eq!(detect_provider("deepseek-v4-flash"), "deepseek");
    }

    #[test]
    fn secrets_are_masked() {
        let masked = mask_settings(r#"{"env":{"ANTHROPIC_AUTH_TOKEN":"sk-real","ANTHROPIC_BASE_URL":"x","K":"$REF","SOME_KEY":"$REF"}}"#);
        assert!(!masked.contains("sk-real"));
        assert!(masked.contains("$REF"));
        assert_eq!(mask_env("A=secret\n# c\nB="), "A=***\n# c\nB=");
    }
}
