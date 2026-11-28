//! Runner Claude Code : `claude -p --output-format json`, prompt sur stdin.

pub(super) mod aliases;
mod args;
pub(super) mod envelope;
mod mcp;
mod persona;
mod plan;
mod retry;
mod settings;
mod temp;

use super::{RunRequest, RunResult};
use crate::registry::LiveRun;
use crate::{binaries, config, process};
use litemcp::cancel::CancelToken;
use plan::Plan;
use process::End;
use std::process::Command;
use std::sync::Arc;

pub fn run(
    req: &RunRequest,
    live: &Arc<LiveRun>,
    resume: Option<&str>,
    cancel: Option<&CancelToken>,
) -> RunResult {
    let plan = match plan::build(req, live, resume) {
        Ok(plan) => plan,
        Err(result) => return result,
    };
    // Jeton primaire d'abord, puis chaque jeton de secours sur 401/429/5xx.
    let tokens: Vec<Option<&String>> =
        std::iter::once(None).chain(plan.fallbacks.iter().map(Some)).collect();
    let mut last = RunResult::default();

    for (i, token) in tokens.iter().enumerate() {
        // Une ligne par commande lancée, secrets masqués (l'environnement n'est pas imprimé).
        crate::debug!(
            "commande {}",
            crate::debug::command_line(&binaries::claude_binary(), &plan.args)
        );
        let output = match process::run(
            command(req, &plan, *token),
            Some(&plan.stdin_prompt),
            live,
            plan.timeout,
            cancel,
        ) {
            Ok(output) => output,
            Err(e) => return spawn_error(&e),
        };
        last = envelope::parse(&output, plan.timeout, plan.model.clone());
        let retryable =
            last.error.is_some() && output.end == End::Exited && retry::is_retryable(&output);
        let more = i + 1 < tokens.len();
        if retryable && more && !live.kill_requested() {
            crate::debug!(
                "retente {} ({}): jeton de secours n°{}",
                req.agent.as_deref().unwrap_or("?"),
                req.runner.as_str(),
                i + 1
            );
            continue;
        }
        if retryable && tokens.len() > 1 {
            last.error = Some(format!(
                "RETRYABLE_ERROR_ALL_FALLBACKS_EXHAUSTED: {}",
                last.error.clone().unwrap_or_default()
            ));
        }
        break;
    }
    last
}

/// Commande `claude` avec l'environnement de l'agent (et un jeton de secours éventuel).
fn command(req: &RunRequest, plan: &Plan, token: Option<&String>) -> Command {
    let mut cmd = process::command(&binaries::claude_binary());
    cmd.args(&plan.args).current_dir(&plan.cwd);
    for (k, v) in &config::get().dotenv() {
        cmd.env(k, v);
    }
    for (k, v) in &plan.env {
        if let Some(s) = v.as_str() {
            cmd.env(k, s);
        }
    }
    if let Some(name) = &req.agent {
        cmd.env("SWARM_AGENT_NAME", name);
    }
    if let Some(t) = token {
        cmd.env("ANTHROPIC_AUTH_TOKEN", t);
    }
    cmd
}

fn spawn_error(e: &str) -> RunResult {
    RunResult::err(format!(
        "SPAWN_ERROR: {e} (binaire: {}). Définis CLAUDE_BIN si Claude Code est ailleurs.",
        binaries::claude_binary().display()
    ))
}

#[cfg(test)]
mod tests {
    use super::{command, plan, run, spawn_error};
    use crate::agents::Runner;
    use crate::agents::claude_settings_path;
    use crate::binaries::claude_binary;
    use crate::registry::{self, LiveRun, Outcome, Status};
    use crate::runners::{RunRequest, RunResult};
    use crate::store::{read_json, write_json_atomic};
    use serde_json::{Value, json};
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    static SEQ: AtomicU32 = AtomicU32::new(0);

    /// Nom unique dans le process : l'index des runs est global.
    fn nom() -> String {
        format!("claude-{}-{}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed))
    }

    /// Workspace temporaire unique (pid + rang), supprimé à la fin du test.
    struct TempWs(PathBuf);

    impl TempWs {
        fn new(tag: &str) -> Self {
            let n = SEQ.fetch_add(1, Ordering::Relaxed);
            let dir =
                std::env::temp_dir().join(format!("swarm-claude-{tag}-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            TempWs(dir)
        }

        fn requete(&self, agent: Option<&str>) -> RunRequest {
            RunRequest {
                runner: Runner::Claude,
                prompt: "un tour".into(),
                agent: agent.map(String::from),
                session_id: None,
                auto_resume: true,
                model: None,
                provider: None,
                cwd: None,
                workspace: self.0.clone(),
            }
        }
    }

    impl Drop for TempWs {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Écrit les settings d'un agent (son `env`, plus des clés quelconques à conserver).
    fn settings(ws: &Path, name: &str, settings: &Value) {
        write_json_atomic(&claude_settings_path(ws, name), settings).unwrap();
    }

    fn fermer(live: &LiveRun) {
        live.finish(Status::Done, Some(0), Outcome::default());
    }

    /// Les permissions attendues dans les arguments : `CLAUDE_PERMISSIONS` si elle est
    /// posée dans l'environnement, sinon le défaut.
    fn permissions() -> Vec<String> {
        crate::config::app_var("CLAUDE_PERMISSIONS")
            .unwrap_or_else(|| "--dangerously-skip-permissions".into())
            .split_whitespace()
            .map(String::from)
            .collect()
    }

    /// Valeur qui suit `flag` dans les arguments.
    fn valeur<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
        args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).map(String::as_str)
    }

    #[test]
    fn un_agent_sans_settings_est_refuse_avant_toute_commande() {
        let ws = TempWs::new("sans-settings");
        let agent = nom();
        let live = registry::begin("claude", Some(&agent), &ws.0).unwrap();
        let req = ws.requete(Some(&agent));
        let erreur =
            plan::build(&req, &live, None).err().expect("plan refusé").error.expect("raison");
        assert!(erreur.starts_with("INVALID_AGENT: settings_"), "{erreur}");
        assert!(erreur.contains(&format!("settings_{agent}.json")), "{erreur}");
        // `run` rend la même erreur, sans rien lancer.
        let result = run(&req, &live, None, None);
        assert_eq!(result.error.as_deref(), Some(erreur.as_str()));
        assert!(result.result.is_empty() && result.raw.is_empty() && result.session_id.is_none());
        fermer(&live);
    }

    #[test]
    fn sans_agent_le_plan_vise_les_fichiers_du_workspace() {
        let ws = TempWs::new("anonyme");
        let live = registry::begin("claude", None, &ws.0).unwrap();
        let mut req = ws.requete(None);
        req.prompt = "résume".into();
        let plan = plan::build(&req, &live, None).expect("plan");
        let mut attendus: Vec<String> =
            ["-p", "--output-format", "json"].map(String::from).to_vec();
        attendus.extend(permissions());
        assert_eq!(plan.args, attendus, "ni reprise, ni modèle, ni nom");
        assert_eq!(plan.stdin_prompt, "résume");
        assert_eq!(plan.timeout, crate::config::default_timeout());
        assert_eq!(plan.cwd, ws.0);
        assert_eq!(plan.model, None);
        assert!(plan.env.is_empty() && plan.fallbacks.is_empty());
        // Avec `.claude/settings.json` et `.mcp.json` dans le workspace : passés au CLI,
        // sans `--strict-mcp-config`.
        let settings_ws = ws.0.join(".claude").join("settings.json");
        let mcp_ws = ws.0.join(".mcp.json");
        write_json_atomic(&settings_ws, &json!({ "a": 1 })).unwrap();
        write_json_atomic(&mcp_ws, &json!({ "mcpServers": {} })).unwrap();
        let plan = plan::build(&req, &live, None).expect("plan");
        assert_eq!(
            valeur(&plan.args, "--settings"),
            Some(settings_ws.display().to_string()).as_deref()
        );
        assert_eq!(
            valeur(&plan.args, "--mcp-config"),
            Some(mcp_ws.display().to_string()).as_deref()
        );
        assert!(!plan.args.iter().any(|a| a == "--strict-mcp-config"));
        fermer(&live);
    }

    #[test]
    fn les_arguments_portent_reprise_modele_nom_et_reglages_de_l_agent() {
        let ws = TempWs::new("arguments");
        let agent = nom();
        write_json_atomic(
            &ws.0.join("model-aliases.json"),
            &json!({ "SONNET": "claude-sonnet-4-6" }),
        )
        .unwrap();
        settings(
            &ws.0,
            &agent,
            &json!({
                "env": {
                    "ANTHROPIC_MODEL": "claude-haiku-4-5",
                    "AGENT_TIMEOUT_MS": "1500",
                    "AUTH_FALLBACK_1": "fb-1",
                    "AUTH_FALLBACK_2": "",
                    "NOMBRE": 7,
                },
                "permissions": { "allow": ["Bash"] },
                "runner": "claude",
            }),
        );
        let live = registry::begin("claude", Some(&agent), &ws.0).unwrap();
        let mut req = ws.requete(Some(&agent));
        req.model = Some("sonnet".into());
        let travail = ws.0.join("travail");
        req.cwd = Some(travail.display().to_string());
        let plan = plan::build(&req, &live, Some("s-42")).expect("plan");
        assert_eq!(valeur(&plan.args, "--resume"), Some("s-42"));
        assert_eq!(valeur(&plan.args, "--model"), Some("claude-sonnet-4-6"), "alias résolu");
        assert_eq!(valeur(&plan.args, "--name"), Some(agent.as_str()));
        assert_eq!(plan.model.as_deref(), Some("claude-sonnet-4-6"));
        assert_eq!(plan.timeout, Duration::from_millis(1500));
        assert_eq!(plan.cwd, travail);
        assert_eq!(
            plan.fallbacks,
            ["fb-1".to_string()],
            "les jetons de secours vides sont ignorés"
        );
        assert_eq!(plan.stdin_prompt, "un tour");
        let env = json!({
            "ANTHROPIC_MODEL": "claude-haiku-4-5",
            "AGENT_TIMEOUT_MS": "1500",
            "AUTH_FALLBACK_1": "fb-1",
            "AUTH_FALLBACK_2": "",
            "NOMBRE": 7,
        })
        .as_object()
        .unwrap()
        .clone();
        assert_eq!(plan.env, env);
        // Settings passés au CLI : sans `env` (les jetons voyagent par l'environnement),
        // le reste conservé.
        let fichier = PathBuf::from(valeur(&plan.args, "--settings").expect("--settings"));
        assert!(fichier.is_file(), "{}", fichier.display());
        assert!(fichier.starts_with(ws.0.join(".claude")), "{}", fichier.display());
        let passe = read_json(&fichier).unwrap();
        assert!(passe.get("env").is_none(), "jamais de jeton sur disque");
        assert_eq!(passe["permissions"]["allow"], json!(["Bash"]));
        assert_eq!(passe["runner"], "claude");
        // Ni `.mcp.<agent>.json` ni `.mcp.json` : aucune option MCP.
        assert_eq!(valeur(&plan.args, "--mcp-config"), None);
        // Les fichiers temporaires disparaissent avec le plan.
        drop(plan);
        assert!(!fichier.exists(), "{}", fichier.display());
        fermer(&live);
    }

    #[test]
    fn le_modele_vient_de_la_demande_puis_des_settings_et_le_vide_est_ignore() {
        let ws = TempWs::new("modele");
        let agent = nom();
        settings(&ws.0, &agent, &json!({ "env": { "ANTHROPIC_MODEL": "du-settings" } }));
        let live = registry::begin("claude", Some(&agent), &ws.0).unwrap();
        let plan = plan::build(&ws.requete(Some(&agent)), &live, None).ok().unwrap();
        assert_eq!(plan.model.as_deref(), Some("du-settings"), "sans demande : celui de l'agent");
        let mut demande = ws.requete(Some(&agent));
        demande.model = Some("de-la-demande".into());
        let plan = plan::build(&demande, &live, None).ok().unwrap();
        assert_eq!(plan.model.as_deref(), Some("de-la-demande"), "la demande passe avant");
        let mut vide = ws.requete(Some(&agent));
        vide.model = Some(String::new());
        let plan = plan::build(&vide, &live, None).ok().unwrap();
        assert_eq!(plan.model, None, "vide : ni la demande ni le settings");
        assert_eq!(valeur(&plan.args, "--model"), None);
        fermer(&live);
    }

    #[test]
    fn la_commande_porte_arguments_repertoire_et_environnement() {
        let ws = TempWs::new("commande");
        let agent = nom();
        settings(
            &ws.0,
            &agent,
            &json!({ "env": { "ANTHROPIC_MODEL": "m-1", "CUSTOM": "v", "NOMBRE": 1 } }),
        );
        // `command` y lit les `.env` : la configuration doit exister (initialisation
        // idempotente, sans effet de bord hors de ce workspace temporaire).
        crate::config::init(Some(ws.0.clone()));
        let live = registry::begin("claude", Some(&agent), &ws.0).unwrap();
        let mut req = ws.requete(Some(&agent));
        let ailleurs = ws.0.join("ailleurs");
        req.cwd = Some(ailleurs.display().to_string());
        let plan = plan::build(&req, &live, Some("s-1")).expect("plan");
        let jeton = "jeton-secours".to_string();
        let cmd = command(&req, &plan, Some(&jeton));
        assert_eq!(cmd.get_program(), claude_binary().as_os_str());
        let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(args, plan.args);
        assert_eq!(cmd.get_current_dir(), Some(ailleurs.as_path()));
        let variables: HashMap<String, String> = envs(&cmd);
        assert_eq!(variables.get("SWARM_AGENT_NAME").map(String::as_str), Some(agent.as_str()));
        assert_eq!(
            variables.get("ANTHROPIC_AUTH_TOKEN").map(String::as_str),
            Some("jeton-secours")
        );
        assert_eq!(variables.get("CUSTOM").map(String::as_str), Some("v"));
        assert!(!variables.contains_key("NOMBRE"), "une valeur non textuelle ne part pas");
        // Sans jeton de secours : aucun jeton imposé par nous (sauf `.env` de l'utilisateur).
        let sans = envs(&command(&req, &plan, None));
        if !crate::config::get().dotenv().contains_key("ANTHROPIC_AUTH_TOKEN") {
            assert!(!sans.contains_key("ANTHROPIC_AUTH_TOKEN"));
        }
        // Sans agent : pas de nom d'agent dans l'environnement.
        let anonyme = registry::begin("claude", None, &ws.0).unwrap();
        let req_anonyme = ws.requete(None);
        let plan_anonyme = plan::build(&req_anonyme, &anonyme, None).expect("plan");
        assert!(
            !envs(&command(&req_anonyme, &plan_anonyme, None)).contains_key("SWARM_AGENT_NAME")
        );
        fermer(&live);
        fermer(&anonyme);
    }

    /// Variables posées sur la commande, en texte.
    fn envs(cmd: &std::process::Command) -> HashMap<String, String> {
        cmd.get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.map(|v| v.to_string_lossy().into_owned()).unwrap_or_default(),
                )
            })
            .collect()
    }

    #[test]
    fn une_erreur_de_lancement_dit_le_binaire_a_definir() {
        let erreur = spawn_error("os error 2").error.expect("erreur");
        assert!(erreur.starts_with("SPAWN_ERROR: os error 2"), "{erreur}");
        assert!(erreur.contains("CLAUDE_BIN"), "{erreur}");
        assert!(erreur.contains(&claude_binary().display().to_string()), "{erreur}");
        // Sans jeton ni modèle : rien d'autre à dire.
        let vide = RunResult::default();
        assert!(vide.error.is_none() && vide.model.is_none());
    }
}
