//! Sous-commandes d'agent : `ask`, `status`, `history`, `reset`, `config`, `schema`.
//!
//! Chaque sous-commande parle au serveur MCP de l'agent s'il tourne (annoncé dans le
//! registre LiteMCP, ou URL portée par son bail d'hébergement), sinon agit localement
//! avec les mêmes règles qu'un run normal : bail de run pris puis libéré, même session
//! reprise puis sauvegardée (PLAN §4.2). `schema` répond toujours localement, depuis
//! le même code d'enregistrement que le serveur de l'agent.

use super::args::{AgentCmd, Options};
use crate::agents::{self, Runner};
use crate::config;
use crate::debug;
use crate::history;
use crate::owners;
use crate::peers::{self, CallError};
use crate::registry;
use crate::residents::{self, Ctx};
use crate::runners::{self, RunRequest};
use crate::sessions;
use serde_json::{Value, json};
use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

/// Attente d'une réponse du serveur de l'agent : au-delà du délai d'un run (15 min)
/// et d'une petite file, comme l'attente par défaut de l'outil `ask` du serveur.
const WAIT: Duration = Duration::from_secs(3700);

/// Échec d'une sous-commande : message + code de sortie (2 : usage, 1 : exécution).
pub(super) struct Failure {
    pub code: ExitCode,
    pub message: String,
}

impl Failure {
    /// Mauvaise requête de l'utilisateur (nom d'outil, agent ambigu…).
    pub fn usage(message: impl Into<String>) -> Self {
        Failure { code: ExitCode::from(2), message: message.into() }
    }
    /// Le monde n'a pas répondu comme prévu (agent absent, occupé, serveur en erreur…).
    pub fn run(message: impl Into<String>) -> Self {
        Failure { code: ExitCode::FAILURE, message: message.into() }
    }
}

/// Exécute la sous-commande d'agent : le workspace vient des options (`--workspace`
/// ou la résolution habituelle), l'agent est déjà connu (nom d'exécutable ou `--agent`).
pub fn run(agent: &str, options: &Options, cmd: AgentCmd) -> ExitCode {
    let cfg = config::init(options.workspace.clone());
    let outcome = match cmd {
        AgentCmd::Ask { prompt } => ask(&cfg.workspace, options, agent, prompt),
        AgentCmd::Status => status(&cfg.workspace, options, agent),
        AgentCmd::History { limit } => journal(&cfg.workspace, options, agent, limit),
        AgentCmd::Reset => reset(&cfg.workspace, options, agent),
        AgentCmd::Config => configs(&cfg.workspace, options, agent),
        AgentCmd::Schema { tool } => schema(&cfg.workspace, options, agent, &tool),
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            eprintln!("swarm-control: {}", failure.message);
            failure.code
        }
    }
}

// ---- partagé -------------------------------------------------------------

/// Runner visé : `--runner` s'il est donné, sinon le seul runner possible (erreur
/// explicite si l'agent existe sur plusieurs runners et que `--runner` manque).
fn runner_of(ws: &Path, options: &Options, agent: &str) -> Result<Runner, Failure> {
    let wanted = options.runner.as_deref().map(|r| {
        Runner::parse(r)
            .ok_or_else(|| Failure::usage(format!("--runner: claude, hermes ou antigravity attendu (reçu « {r} »)")))
    });
    match wanted {
        Some(Ok(r)) => agents::resolve(ws, agent, Some(r)).map_err(Failure::run),
        Some(Err(failure)) => Err(failure),
        None => agents::resolve(ws, agent, None).map_err(Failure::run),
    }
}

/// URL du serveur MCP de l'agent s'il tourne : annonce vivante dans le registre
/// LiteMCP, sinon l'URL posée par son bail d'hébergement sur le workspace.
fn server_url(ws: &Path, runner: Runner, agent: &str) -> Option<String> {
    if let Some(peer) = peers::find(agent) {
        if peer.alive {
            return Some(peer.url);
        }
    }
    owners::host_of(ws, runner.as_str(), agent).and_then(|h| h.url)
}

/// Message d'erreur d'un appel MCP, en clair pour la CLI.
fn call_error(e: CallError) -> String {
    match e {
        CallError::Unreachable(m) => format!("serveur injoignable ({m})"),
        CallError::Tool(m) => format!("outil en erreur ({m})"),
        CallError::Protocol(m) => format!("réponse illisible ({m})"),
    }
}

/// JSON imprimé sur stdout par les sous-commandes qui rendent des données.
fn pretty(value: Value) -> String {
    serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string())
}

/// Le tour exécuté ici (serveur absent ou injoignable) : mêmes règles qu'un run
/// normal — le bail de run est pris par `prepare` et la session est reprise puis
/// sauvegardée par `execute`, comme le ferait le serveur de l'agent.
fn local_turn(
    ws: &Path,
    options: &Options,
    agent: &str,
    prompt: String,
) -> Result<Value, Failure> {
    let runner = runner_of(ws, options, agent)?;
    let req = RunRequest {
        runner,
        prompt,
        agent: Some(agent.to_string()),
        session_id: None,
        auto_resume: true,
        model: None,
        provider: None,
        cwd: None,
        workspace: ws.to_path_buf(),
    };
    let live = runners::prepare(&req)
        .map_err(|r| Failure::run(r.error.unwrap_or_else(|| "run impossible".into())))?;
    let result = runners::execute(&req, &live, None);
    let error = result.error.is_some();
    let out = json!({
        "agent": agent,
        "runner": runner.as_str(),
        "reply": result.result,
        "sessionId": result.session_id,
        "error": result.error,
    });
    if error {
        return Err(Failure::run(format!("tour local:\n{}", pretty(out))));
    }
    Ok(out)
}

// ---- ask ------------------------------------------------------------------

fn ask(ws: &Path, options: &Options, agent: &str, prompt: String) -> Result<(), Failure> {
    let runner = runner_of(ws, options, agent)?;
    let reply = match server_url(ws, runner, agent) {
        Some(url) => {
            debug::line("mcp", format!("ask → {url}"));
            match peers::call_tool(&url, "ask", json!({"prompt": prompt, "from": "cli"}), WAIT) {
                Ok(reply) => reply,
                Err(CallError::Unreachable(why)) => {
                    debug::line("mcp", format!("{url} injoignable ({why}) : tour local"));
                    local_turn(ws, options, agent, prompt)?
                }
                Err(e) => return Err(Failure::run(format!("ask via {url} : {}", call_error(e)))),
            }
        }
        None => {
            debug::line("ask", "serveur absent : tour local");
            local_turn(ws, options, agent, prompt)?
        }
    };
    println!("{}", pretty(reply));
    Ok(())
}

// ---- status ---------------------------------------------------------------

fn status(ws: &Path, options: &Options, agent: &str) -> Result<(), Failure> {
    let runner = runner_of(ws, options, agent)?;
    let state = match server_url(ws, runner, agent) {
        Some(url) => {
            debug::line("mcp", format!("status → {url}"));
            match peers::call_tool(&url, "status", json!({}), WAIT) {
                Ok(state) => state,
                Err(CallError::Unreachable(why)) => {
                    debug::line("mcp", format!("{url} injoignable ({why}) : état local"));
                    local_status(ws, runner, agent)
                }
                Err(e) => return Err(Failure::run(format!("status via {url} : {}", call_error(e)))),
            }
        }
        None => local_status(ws, runner, agent),
    };
    println!("{}", pretty(state));
    Ok(())
}

/// État sans serveur : session gardée et dernier run, lus sur le workspace.
fn local_status(ws: &Path, runner: Runner, agent: &str) -> Value {
    let last = registry::disk_entry(ws, agent, Some(runner.as_str()))
        .map(|(r, run)| json!({"runner": r, "run": run}))
        .unwrap_or(Value::Null);
    json!({
        "agent": agent,
        "runner": runner.as_str(),
        "resident": false,
        "session": sessions::get(ws, runner.as_str(), agent),
        "lastRun": last,
    })
}

// ---- history --------------------------------------------------------------

fn journal(ws: &Path, options: &Options, agent: &str, limit: usize) -> Result<(), Failure> {
    let runner = runner_of(ws, options, agent)?;
    let entries = match server_url(ws, runner, agent) {
        Some(url) => {
            debug::line("mcp", format!("history → {url}"));
            match peers::call_tool(&url, "history", json!({"limit": limit}), WAIT) {
                Ok(reply) => reply,
                // L'outil `history` du serveur peut ne pas exister encore (lot « journal ») :
                // le journal est sur le même workspace, la lecture locale dit la même chose.
                Err(e) => {
                    debug::line("mcp", format!(
                        "history via {url} indisponible ({}) : journal lu sur disque",
                        call_error(e)
                    ));
                    local_history(ws, runner, agent, limit)
                }
            }
        }
        None => local_history(ws, runner, agent, limit),
    };
    println!("{}", pretty(entries));
    Ok(())
}

fn local_history(ws: &Path, runner: Runner, agent: &str, limit: usize) -> Value {
    let path = history::path(ws, runner.as_str(), agent);
    match history::read(ws, runner.as_str(), agent, limit, None) {
        Ok(entries) => json!({"entries": entries, "path": path}),
        Err(e) => json!({"entries": [], "path": path, "error": e.to_string()}),
    }
}

// ---- reset ----------------------------------------------------------------

fn reset(ws: &Path, options: &Options, agent: &str) -> Result<(), Failure> {
    let runner = runner_of(ws, options, agent)?;
    let had = sessions::get(ws, runner.as_str(), agent).is_some();
    // L'outil `session` du serveur oublierait aussi la session d'un tour en cours ;
    // tant qu'il n'existe pas côté serveur (lot « journal »), on oublie le fichier
    // directement : c'est le même fichier, lu par chaque tour au démarrage.
    let done = match server_url(ws, runner, agent) {
        Some(url) => {
            debug::line("mcp", format!("session reset → {url}"));
            match peers::call_tool(&url, "session", json!({"action": "reset"}), WAIT) {
                Ok(_) => true,
                Err(e) => {
                    debug::line("mcp", format!(
                        "session reset via {url} indisponible ({}) : oubli local",
                        call_error(e)
                    ));
                    forget(ws, runner, agent)
                }
            }
        }
        None => forget(ws, runner, agent),
    };
    println!("{}", pretty(json!({"agent": agent, "reset": done, "hadSession": had})));
    Ok(())
}

/// Oubli local de la session, comme `Ctx::reset_session` côté serveur.
fn forget(ws: &Path, runner: Runner, agent: &str) -> bool {
    debug::line("session", format!("oubliée : {agent} ({})", runner.as_str()));
    sessions::delete(ws, runner.as_str(), agent)
}

// ---- config ---------------------------------------------------------------

fn configs(ws: &Path, options: &Options, agent: &str) -> Result<(), Failure> {
    let runner = runner_of(ws, options, agent)?;
    let wanted = options.runner.as_deref().and_then(Runner::parse);
    let files = match server_url(ws, runner, agent) {
        Some(url) => {
            debug::line("mcp", format!("config → {url}"));
            match peers::call_tool(&url, "config", json!({}), WAIT) {
                Ok(reply) => reply,
                Err(CallError::Unreachable(why)) => {
                    debug::line("mcp", format!("{url} injoignable ({why}) : fichiers lus localement"));
                    local_configs(ws, wanted, agent)?
                }
                Err(e) => {
                    debug::line("mcp", format!(
                        "config via {url} indisponible ({}) : fichiers lus localement",
                        call_error(e)
                    ));
                    local_configs(ws, wanted, agent)?
                }
            }
        }
        None => local_configs(ws, wanted, agent)?,
    };
    println!("{}", pretty(files));
    Ok(())
}

/// Fichiers de l'agent, secrets masqués — le même travail que l'outil
/// `get_agent_configs` de l'instance.
fn local_configs(ws: &Path, wanted: Option<Runner>, agent: &str) -> Result<Value, Failure> {
    let files = agents::get_configs(ws, agent, wanted).map_err(Failure::run)?;
    let map: serde_json::Map<String, Value> =
        files.into_iter().map(|(name, content)| (name, json!(content))).collect();
    Ok(json!({"name": agent, "files": map}))
}

// ---- schema ---------------------------------------------------------------
/// Schéma MCP d'un outil de l'agent : le catalogue est construit ici avec le même
/// code d'enregistrement que le serveur (`residents::mcp`), et LiteMCP répond un
/// `tools/list` en local, sans réseau ni serveur démarré.
fn schema(ws: &Path, options: &Options, agent: &str, tool: &str) -> Result<(), Failure> {
    let runner = runner_of(ws, options, agent)?;
    let ctx = Ctx::new(agent, runner, ws.to_path_buf(), owners::instance_name());
    let builder = litemcp::prelude::ServerBuilder::new(&format!("swarm-{agent}"), env!("CARGO_PKG_VERSION"))
        .no_tool_timeout();
    let core = residents::mcp::register(builder, Arc::new(ctx)).core();
    let request = br#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;
    let response = core
        .dispatch_unchecked(request)
        .ok_or_else(|| Failure::run("tools/list n'a rien répondu".to_string()))?;
    let catalog: Value = serde_json::from_slice(&response)
        .map_err(|e| Failure::run(format!("tools/list illisible : {e}")))?;
    let tools = catalog["result"]["tools"].as_array().ok_or_else(|| {
        Failure::run(format!("tools/list sans outils : {catalog}"))
    })?;
    let found = tools.iter().find(|t| t["name"].as_str() == Some(tool));
    match found {
        Some(tool) => {
            println!("{}", pretty(tool.clone()));
            Ok(())
        }
        None => {
            let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
            Err(Failure::usage(format!(
                "outil inconnu: {tool} (outils de l'agent : {})",
                names.join(", ")
            )))
        }
    }
}
