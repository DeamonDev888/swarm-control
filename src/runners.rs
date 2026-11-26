//! Exécution d'un agent : Claude Code ou Hermes, avec reprise de session.
//!
//! Le cycle est le même pour les deux runners :
//! 1. résoudre l'agent (existence, runner) ;
//! 2. réserver le run dans le registre (un seul run vivant par agent) ;
//! 3. reprendre la dernière session de l'agent (`autoResume`) ;
//! 4. lancer le CLI, suivre sa sortie, enregistrer la session rendue ;
//! 5. si la session reprise est invalide : l'oublier et relancer à neuf, une fois.

use crate::agents::{self, Runner};
use crate::config;
use crate::process::{self, End, ProcOutput};
use crate::registry::{self, LiveRun, Outcome, Status};
use crate::sessions;
use litemcp::cancel::CancelToken;
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

pub struct RunRequest {
    pub runner: Runner,
    pub prompt: String,
    pub agent: Option<String>,
    pub session_id: Option<String>,
    pub auto_resume: bool,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub cwd: Option<String>,
    pub workspace: PathBuf,
}

#[derive(Debug, Clone, Default)]
pub struct RunResult {
    pub result: String,
    pub session_id: Option<String>,
    pub error: Option<String>,
    /// Sortie brute (fin de flux) — diagnostic en cas d'échec.
    pub raw: String,
    pub model: Option<String>,
}

impl RunResult {
    fn err(error: impl Into<String>) -> Self {
        RunResult { error: Some(error.into()), ..Default::default() }
    }
}

/// Réserve le run dans le registre. Les erreurs d'agent (nom, existence,
/// occupé) sont rendues ici, avant tout lancement.
pub fn prepare(req: &RunRequest) -> Result<Arc<LiveRun>, RunResult> {
    if req.prompt.trim().is_empty() {
        return Err(RunResult::err("INVALID_PROMPT: prompt vide."));
    }
    if let Some(name) = &req.agent {
        agents::validate_name(name).map_err(|e| RunResult::err(format!("INVALID_AGENT_NAME: {e}")))?;
        if !agents::exists_for(&req.workspace, name, req.runner) {
            let hint = match agents::peek_runner(&req.workspace, name) {
                Some(other) => format!(
                    " L'agent existe côté {} : relance avec runner:\"{}\".",
                    other.as_str(),
                    other.as_str()
                ),
                None => " (create_agent d'abord)".to_string(),
            };
            return Err(RunResult::err(format!(
                "INVALID_AGENT: agent '{name}' introuvable pour le runner {}.{hint}",
                req.runner.as_str()
            )));
        }
    }
    registry::begin(req.runner.as_str(), req.agent.as_deref(), &req.workspace)
        .map_err(RunResult::err)
}

/// Exécute le run réservé et clôt son entrée de registre.
pub fn execute(req: &RunRequest, live: &Arc<LiveRun>, cancel: Option<&CancelToken>) -> RunResult {
    let runner = req.runner.as_str();
    let mut resume = req.session_id.clone();
    if resume.is_none() && req.auto_resume {
        if let Some(agent) = &req.agent {
            resume = sessions::get(&req.workspace, runner, agent);
        }
    }

    let mut result = attempt(req, live, resume.as_deref(), cancel);
    if let (Some(used), Some(err)) = (&resume, &result.error) {
        if looks_like_dead_session(err, &result.raw) && !live.kill_requested() {
            if let Some(agent) = &req.agent {
                sessions::delete(&req.workspace, runner, agent);
            }
            let _ = used;
            result = attempt(req, live, None, cancel);
        }
    }

    // Persistance : la session rendue devient la reprise du prochain appel.
    if let (Some(agent), Some(sid)) = (&req.agent, &result.session_id) {
        if result.error.is_none() || !sid.is_empty() {
            if let Err(e) = sessions::save(&req.workspace, runner, agent, sid) {
                eprintln!("[overmind] sauvegarde de session impossible: {e}");
            }
            live.set_session(sid);
        }
    }

    let status = if live.kill_requested() {
        Status::Killed
    } else if result.error.is_some() {
        Status::Failed
    } else {
        Status::Done
    };
    live.finish(
        status,
        None,
        Outcome { result: result.result.clone(), error: result.error.clone() },
    );
    result
}

fn looks_like_dead_session(error: &str, raw: &str) -> bool {
    let hay = format!("{error}\n{raw}").to_lowercase();
    [
        "no conversation found",
        "invalid session",
        "session not found",
        "could not find session",
        "unknown session",
        "no session found",
    ]
    .iter()
    .any(|p| hay.contains(p))
}

fn attempt(req: &RunRequest, live: &Arc<LiveRun>, resume: Option<&str>, cancel: Option<&CancelToken>) -> RunResult {
    match req.runner {
        Runner::Claude => run_claude(req, live, resume, cancel),
        Runner::Hermes => run_hermes(req, live, resume, cancel),
    }
}

fn end_error(out: &ProcOutput, timeout: Duration) -> Option<String> {
    match out.end {
        End::Exited => None,
        End::TimedOut => Some(format!("TIMEOUT: arrêté après {}s.", timeout.as_secs())),
        End::Killed => Some("KILLED: arrêté par agent_control.".into()),
        End::Cancelled => Some("CANCELLED: requête annulée par le client.".into()),
    }
}

fn tail(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut start = s.len() - max;
    while !s.is_char_boundary(start) {
        start += 1;
    }
    format!("…{}", &s[start..])
}

// ─── Binaires ───────────────────────────────────────────────────────────────

fn on_path(names: &[&str]) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for name in names {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// `claude` : `CLAUDE_BIN`, puis l'exécutable npm global (Windows), puis le PATH.
pub fn claude_binary() -> PathBuf {
    if let Some(bin) = config::env_var("CLAUDE_BIN").filter(|s| !s.is_empty()) {
        return PathBuf::from(bin);
    }
    if cfg!(windows) {
        let appdata = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| config::home_dir().join("AppData").join("Roaming"));
        let npm = appdata
            .join("npm")
            .join("node_modules")
            .join("@anthropic-ai")
            .join("claude-code")
            .join("bin")
            .join("claude.exe");
        if npm.is_file() {
            return npm;
        }
        if let Some(p) = on_path(&["claude.exe"]) {
            return p;
        }
    } else if let Some(p) = on_path(&["claude"]) {
        return p;
    }
    PathBuf::from("claude")
}

/// `hermes` : `HERMES_BIN`, puis le PATH (en évitant les wrappers `.cmd`).
pub fn hermes_binary() -> PathBuf {
    if let Some(bin) = config::env_var("HERMES_BIN").filter(|s| !s.is_empty()) {
        return PathBuf::from(bin);
    }
    let names: &[&str] = if cfg!(windows) { &["hermes.exe"] } else { &["hermes"] };
    on_path(names).unwrap_or_else(|| PathBuf::from("hermes"))
}

// ─── Claude Code ────────────────────────────────────────────────────────────

/// Fichiers temporaires d'un run, supprimés à la fin (même en cas d'erreur).
struct TempFiles(Vec<PathBuf>);
impl Drop for TempFiles {
    fn drop(&mut self) {
        for p in &self.0 {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// Alias de modèles : `<workspace>/model-aliases.json` (`{"surnom": "vrai-modèle"}`).
fn resolve_model(ws: &Path, model: &str) -> String {
    let aliases = crate::store::read_json(&ws.join("model-aliases.json"));
    aliases
        .as_ref()
        .and_then(Value::as_object)
        .and_then(|m| {
            m.iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(model))
                .and_then(|(_, v)| v.as_str().map(String::from))
        })
        .unwrap_or_else(|| model.to_string())
}

const FALLBACK_KEYS: [&str; 4] = [
    "AUTH_FALLBACK_1",
    "AUTH_FALLBACK_2",
    "AUTH_FALLBACK_3",
    "ANTHROPIC_AUTH_TOKEN_FALLBACK",
];

fn run_claude(req: &RunRequest, live: &Arc<LiveRun>, resume: Option<&str>, cancel: Option<&CancelToken>) -> RunResult {
    let ws = &req.workspace;
    let claude_dir = ws.join(".claude");
    let mut temps = TempFiles(Vec::new());
    let mut agent_env: Map<String, Value> = Map::new();
    let mut timeout = config::default_timeout();

    let mut settings_arg: Option<PathBuf> = None;
    let mut mcp_arg: Option<PathBuf> = None;
    let mut strict_mcp = false;
    let mut persona: Option<String> = None;

    if let Some(name) = &req.agent {
        let settings_path = agents::claude_settings_path(ws, name);
        let Some(raw) = crate::store::read_json(&settings_path) else {
            return RunResult::err(format!("INVALID_AGENT: settings_{name}.json absent ou invalide."));
        };
        let settings = config::interpolate_json(&raw);
        if let Some(env) = settings.get("env").and_then(Value::as_object) {
            agent_env = env.clone();
            if let Some(ms) = ["AGENT_TIMEOUT_MS", "API_TIMEOUT_MS"]
                .iter()
                .find_map(|k| env.get(*k).and_then(Value::as_str))
                .and_then(|s| s.trim().parse::<u64>().ok())
                .filter(|ms| *ms > 0)
            {
                timeout = Duration::from_millis(ms);
            }
        }
        // Le fichier de settings passé au CLI ne contient PAS `env` : les
        // variables (jetons compris) voyagent par l'environnement du process,
        // jamais par un fichier temporaire sur disque.
        let mut for_cli = settings.clone();
        if let Some(obj) = for_cli.as_object_mut() {
            obj.remove("env");
        }
        let tmp = claude_dir.join(format!("settings_{name}_run_{}.json", live.run_id));
        if crate::store::write_json_atomic(&tmp, &for_cli).is_ok() {
            temps.0.push(tmp.clone());
            settings_arg = Some(tmp);
        }

        // Serveurs MCP : fichier de l'agent, sinon extrait filtré du .mcp.json global.
        let own_mcp = agents::claude_mcp_path(ws, name);
        if own_mcp.is_file() {
            mcp_arg = Some(own_mcp);
            strict_mcp = true;
        } else if settings.get("enableAllProjectMcpServers") == Some(&Value::Bool(false)) {
            if let Some(enabled) = settings.get("enabledMcpjsonServers").and_then(Value::as_array) {
                let global = crate::store::read_json(&agents::global_mcp_path(ws));
                let all = global
                    .as_ref()
                    .and_then(|g| g.get("mcpServers"))
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                let picked: Map<String, Value> = enabled
                    .iter()
                    .filter_map(Value::as_str)
                    .filter_map(|s| all.get(s).map(|c| (s.to_string(), c.clone())))
                    .collect();
                let tmp = claude_dir.join(format!("mcp_{name}_run_{}.json", live.run_id));
                if crate::store::write_json_atomic(&tmp, &serde_json::json!({ "mcpServers": picked })).is_ok() {
                    temps.0.push(tmp.clone());
                    mcp_arg = Some(tmp);
                    strict_mcp = true;
                }
            }
        }
        if mcp_arg.is_none() {
            let g = agents::global_mcp_path(ws);
            if g.is_file() {
                mcp_arg = Some(g);
            }
        }
        persona = std::fs::read_to_string(agents::claude_prompt_path(ws, name))
            .ok()
            .filter(|p| !p.trim().is_empty());
    } else {
        let s = claude_dir.join("settings.json");
        if s.is_file() {
            settings_arg = Some(s);
        }
        let g = agents::global_mcp_path(ws);
        if g.is_file() {
            mcp_arg = Some(g);
        }
    }

    let model = req
        .model
        .clone()
        .or_else(|| agent_env.get("ANTHROPIC_MODEL").and_then(Value::as_str).map(String::from))
        .filter(|m| !m.is_empty())
        .map(|m| resolve_model(ws, &m));

    let mut args: Vec<String> = vec!["-p".into(), "--output-format".into(), "json".into()];
    let permissions = config::env_var("OVERMIND_CLAUDE_PERMISSIONS")
        .unwrap_or_else(|| "--dangerously-skip-permissions".into());
    args.extend(permissions.split_whitespace().map(String::from));
    if let Some(p) = &settings_arg {
        args.push("--settings".into());
        args.push(p.display().to_string());
    }
    if let Some(p) = &mcp_arg {
        args.push("--mcp-config".into());
        args.push(p.display().to_string());
        if strict_mcp {
            args.push("--strict-mcp-config".into());
        }
    }
    if let Some(sid) = resume {
        args.push("--resume".into());
        args.push(sid.into());
    }
    if let Some(m) = &model {
        args.push("--model".into());
        args.push(m.clone());
    }
    if let Some(name) = &req.agent {
        args.push("--name".into());
        args.push(name.clone());
    }
    // Limite de ligne de commande Windows (~32 Kio) : une persona plus longue
    // est ajoutée au début du prompt (stdin) à la place.
    let mut stdin_prompt = req.prompt.clone();
    if let Some(p) = persona {
        if p.len() <= 20_000 {
            args.push("--append-system-prompt".into());
            args.push(p);
        } else if resume.is_none() {
            stdin_prompt = format!("{p}\n\n---\n\n{}", req.prompt);
        }
    }

    // Jetons : primaire (déjà dans l'env) puis AUTH_FALLBACK_n sur 401/429/5xx.
    let fallbacks: Vec<String> = FALLBACK_KEYS
        .iter()
        .filter_map(|k| agent_env.get(*k).and_then(Value::as_str))
        .filter(|v| !v.is_empty())
        .map(String::from)
        .collect();

    let cwd = req.cwd.as_deref().map(PathBuf::from).unwrap_or_else(|| req.workspace.clone());
    let mut last = RunResult::default();
    let mut attempts: Vec<Option<&String>> = vec![None];
    attempts.extend(fallbacks.iter().map(Some));

    for (i, token) in attempts.iter().enumerate() {
        let mut cmd = process::command(&claude_binary());
        cmd.args(&args).current_dir(&cwd);
        for (k, v) in &config::get().dotenv {
            cmd.env(k, v);
        }
        for (k, v) in &agent_env {
            if let Some(s) = v.as_str() {
                cmd.env(k, s);
            }
        }
        if let Some(name) = &req.agent {
            cmd.env("OVERMIND_AGENT_NAME", name);
        }
        if let Some(t) = token {
            cmd.env("ANTHROPIC_AUTH_TOKEN", t);
        }

        let output = match process::run(cmd, Some(&stdin_prompt), live, timeout, cancel) {
            Ok(o) => o,
            Err(e) => {
                return RunResult::err(format!(
                    "SPAWN_ERROR: {e} (binaire: {}). Définis CLAUDE_BIN si Claude Code est ailleurs.",
                    claude_binary().display()
                ));
            }
        };
        last = parse_claude(&output, timeout, model.clone());
        let retryable = last.error.is_some() && output.end == End::Exited && is_retryable(&output);
        if retryable && i + 1 < attempts.len() && !live.kill_requested() {
            continue;
        }
        if retryable && attempts.len() > 1 {
            last.error = Some(format!(
                "RETRYABLE_ERROR_ALL_FALLBACKS_EXHAUSTED: {}",
                last.error.clone().unwrap_or_default()
            ));
        }
        break;
    }
    last
}

/// Enveloppe JSON de `claude -p --output-format json`.
fn parse_claude(out: &ProcOutput, timeout: Duration, model: Option<String>) -> RunResult {
    let raw = join_raw(out);
    if let Some(e) = end_error(out, timeout) {
        return RunResult { error: Some(e), raw, session_id: envelope(&out.stdout).as_ref().and_then(sid_of), model, ..Default::default() };
    }
    let env = envelope(&out.stdout);
    let session_id = env.as_ref().and_then(sid_of);
    match env {
        Some(v) => {
            let text = v
                .get("result")
                .or_else(|| v.get("reply"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let is_error = v.get("is_error").and_then(Value::as_bool).unwrap_or(false);
            if is_error || out.code.is_some_and(|c| c != 0) {
                let subtype = v.get("subtype").and_then(Value::as_str).unwrap_or("error");
                let status = v.get("api_error_status").and_then(Value::as_u64);
                let detail = if text.is_empty() { tail(&out.stderr, 800) } else { tail(&text, 800) };
                let code = match status {
                    Some(s) => format!("CLAUDE_ERROR[{subtype}, HTTP {s}]"),
                    None => format!("CLAUDE_ERROR[{subtype}]"),
                };
                return RunResult { error: Some(format!("{code}: {detail}")), result: text, raw, session_id, model };
            }
            RunResult { result: text, session_id, raw, model, error: None }
        }
        None => {
            if out.code == Some(0) && !out.stdout.trim().is_empty() {
                RunResult { result: out.stdout.trim().to_string(), raw, model, ..Default::default() }
            } else {
                let code = out.code.map_or("?".to_string(), |c| c.to_string());
                let hint = tail(out.stderr.trim(), 800);
                RunResult { error: Some(format!("EXIT_CODE_{code}: {hint}")), raw, model, ..Default::default() }
            }
        }
    }
}

/// Dernier objet JSON de la sortie (tolère du bruit avant/après).
fn envelope(stdout: &str) -> Option<Value> {
    let t = stdout.trim();
    if let Ok(v) = serde_json::from_str::<Value>(t) {
        return v.is_object().then_some(v);
    }
    let end = t.rfind('}')?;
    let mut start = t[..end].rfind('{')?;
    loop {
        if let Ok(v) = serde_json::from_str::<Value>(&t[start..=end]) {
            if v.is_object() {
                return Some(v);
            }
        }
        start = t[..start].rfind('{')?;
    }
}

fn sid_of(v: &Value) -> Option<String> {
    v.get("session_id").and_then(Value::as_str).filter(|s| !s.is_empty()).map(String::from)
}

/// Erreur d'authentification / quota / serveur : un autre jeton peut réussir.
fn is_retryable(out: &ProcOutput) -> bool {
    if let Some(status) = envelope(&out.stdout).and_then(|v| v.get("api_error_status").and_then(Value::as_u64)) {
        return matches!(status, 401 | 429 | 500 | 502 | 503);
    }
    let hay = out.stderr.to_lowercase();
    [
        "unauthorized",
        "invalid api key",
        "api key invalid",
        "authentication failed",
        "invalid authentication",
        "rate limit",
        "quota exhausted",
        "limit exhausted",
        "service unavailable",
        "internal server error",
    ]
    .iter()
    .any(|p| hay.contains(p))
}

fn join_raw(out: &ProcOutput) -> String {
    let mut s = tail(&out.stdout, 20_000);
    if !out.stderr.trim().is_empty() {
        s.push_str("\n\n--- STDERR ---\n");
        s.push_str(&tail(&out.stderr, 8_000));
    }
    s
}

// ─── Hermes ─────────────────────────────────────────────────────────────────

fn run_hermes(req: &RunRequest, live: &Arc<LiveRun>, resume: Option<&str>, cancel: Option<&CancelToken>) -> RunResult {
    let timeout = config::default_timeout();
    let mut args: Vec<String> = Vec::new();
    if let Some(name) = &req.agent {
        args.push("-p".into());
        args.push(name.clone());
    }
    // Prompt lu sur stdin (`--query-file -`) : aucun souci de guillemets,
    // d'échappement ni de longueur de ligne de commande.
    args.extend(
        ["chat", "--query-file", "-", "-Q", "--yolo", "--pass-session-id", "--source", "tool"]
            .iter()
            .map(|s| s.to_string()),
    );
    if let Some(m) = &req.model {
        args.push("--model".into());
        args.push(m.clone());
    }
    if let Some(p) = &req.provider {
        args.push("--provider".into());
        args.push(p.clone());
    }
    if let Some(sid) = resume {
        args.push("--resume".into());
        args.push(sid.into());
    }

    let cwd = req.cwd.as_deref().map(PathBuf::from).unwrap_or_else(|| req.workspace.clone());
    let mut cmd = process::command(&hermes_binary());
    cmd.args(&args).current_dir(&cwd);
    for (k, v) in &config::get().dotenv {
        cmd.env(k, v);
    }
    cmd.env("HERMES_HOME", config::native_hermes_home())
        .env("OVERMIND_AGENT_NAME", req.agent.as_deref().unwrap_or(""))
        .env("PYTHONIOENCODING", "utf-8")
        .env("PYTHONUNBUFFERED", "1")
        .env("TERM", "emacs")
        .env("PROMPT_TOOLKIT_NO_INTERACTIVE", "1");

    let output = match process::run(cmd, Some(&req.prompt), live, timeout, cancel) {
        Ok(o) => o,
        Err(e) => {
            return RunResult::err(format!(
                "SPAWN_ERROR: {e} (binaire: {}). Définis HERMES_BIN si Hermes est ailleurs.",
                hermes_binary().display()
            ));
        }
    };

    let raw = join_raw(&output);
    // L'identifiant de session sort sur stderr (ou, selon la version, stdout).
    let session_id = find_session_id(&output.stderr)
        .or_else(|| find_session_id(&output.stdout))
        .or_else(|| resume.map(String::from));
    if let Some(e) = end_error(&output, timeout) {
        return RunResult { error: Some(e), raw, session_id, ..Default::default() };
    }
    let text = strip_session_line(&output.stdout).trim().to_string();
    match output.code {
        Some(0) => RunResult { result: text, session_id, raw, ..Default::default() },
        code => {
            // Code non nul = échec, même avec du texte sur stdout : Hermes y met
            // alors le message d'erreur du fournisseur (crédits épuisés, 401…).
            let code = code.map_or("?".to_string(), |c| c.to_string());
            let detail = if text.is_empty() { tail(output.stderr.trim(), 800) } else { tail(&text, 800) };
            let mut error = format!("EXIT_CODE_{code}: {detail}");
            if let Some(hint) = hermes_hint(&format!("{text}\n{}", output.stderr), req.agent.as_deref()) {
                error.push_str(&format!("\n→ {hint}"));
            }
            RunResult { error: Some(error), result: text, raw, session_id, ..Default::default() }
        }
    }
}

/// Indice actionnable pour les échecs Hermes les plus courants.
fn hermes_hint(text: &str, agent: Option<&str>) -> Option<String> {
    let lower = text.to_lowercase();
    if lower.contains("no usable credentials found") {
        let who = agent.unwrap_or("<agent>");
        return Some(format!(
            "les credentials Hermes sont par profil : `hermes -p {who} auth add <provider>` (ou recrée l'agent avec cloneFrom)."
        ));
    }
    if lower.contains("billing or credits exhausted") || lower.contains("insufficient balance") {
        return Some("crédits du fournisseur épuisés : change de modèle/provider (update_agent_config model) ou recharge.".into());
    }
    None
}

/// `session_id: 20260628_114351_3fa469` → `20260628_114351_3fa469`.
fn find_session_id(text: &str) -> Option<String> {
    for line in text.lines().rev() {
        let lower = line.to_lowercase();
        if let Some(idx) = lower.find("session_id:") {
            let rest = line[idx + "session_id:".len()..].trim();
            let id: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
                .collect();
            if !id.is_empty() {
                return Some(id);
            }
        }
    }
    None
}

fn strip_session_line(stdout: &str) -> String {
    stdout
        .lines()
        .filter(|l| !l.trim().to_lowercase().starts_with("session_id:"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(stdout: &str, stderr: &str, code: i32) -> ProcOutput {
        ProcOutput { stdout: stdout.into(), stderr: stderr.into(), code: Some(code), end: End::Exited }
    }

    #[test]
    fn claude_success_envelope() {
        let o = out(r#"{"type":"result","is_error":false,"result":"ok","session_id":"abc"}"#, "", 0);
        let r = parse_claude(&o, Duration::from_secs(1), None);
        assert_eq!(r.result, "ok");
        assert_eq!(r.session_id.as_deref(), Some("abc"));
        assert!(r.error.is_none());
    }

    #[test]
    fn claude_error_envelope_and_retryable() {
        let o = out(r#"{"is_error":true,"subtype":"error","api_error_status":429,"result":"rate"}"#, "", 1);
        let r = parse_claude(&o, Duration::from_secs(1), None);
        assert!(r.error.unwrap().contains("HTTP 429"));
        assert!(is_retryable(&o));
        assert!(!is_retryable(&out("{}", "boom", 1)));
    }

    #[test]
    fn envelope_tolerates_noise() {
        let v = envelope("warning {not json\n{\"session_id\":\"s\"}\ntrailer").unwrap();
        assert_eq!(v["session_id"], "s");
    }

    #[test]
    fn hermes_session_id() {
        assert_eq!(find_session_id("x\nsession_id: 20260628_114351_3fa469\n").as_deref(), Some("20260628_114351_3fa469"));
        assert_eq!(strip_session_line("hello\nsession_id: abc\nworld"), "hello\nworld");
    }

    #[test]
    fn dead_session_detection() {
        assert!(looks_like_dead_session("EXIT_CODE_1", "No conversation found with session ID"));
        assert!(!looks_like_dead_session("TIMEOUT", ""));
    }
}
