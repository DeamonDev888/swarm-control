//! Sous-commandes d'instance : `agents`, `run`, `runs`, `doctor`.
//!
//! Ce sont des commandes de consultation ou de tour unique, sans serveur :
//! elles partent du workspace résolu et rendent du JSON sur stdout. `run`
//! confie le tour à l'hôte de l'agent s'il est résident ailleurs (même
//! routage que l'outil `run_agent`), sinon l'exécute ici, bail et session
//! compris (PLAN §4.2).

use super::agent_cli::Failure;
use super::args::{InstanceCmd, Options};
use crate::agents::{self, Runner};
use crate::config;
use crate::debug;
use crate::owners;
use crate::peers;
use crate::registry;
use crate::routing;
use crate::runners::{self, RunRequest, RunResult};
use serde_json::{Value, json};
use std::path::Path;
use std::process::ExitCode;

/// Exécute la sous-commande d'instance.
pub fn run(options: &Options, cmd: InstanceCmd) -> ExitCode {
    let cfg = config::init(options.workspace.clone());
    // Les baux pris par un `run` local doivent rester frais (battement de cœur),
    // même si le tour dure plus de deux minutes.
    registry::start_sweeper(cfg.workspace.clone());
    let outcome = match cmd {
        InstanceCmd::Agents => agents_list(&cfg.workspace),
        InstanceCmd::Run { agent, prompt } => run_turn(&cfg.workspace, options, &agent, prompt),
        InstanceCmd::Runs => runs(&cfg.workspace),
        InstanceCmd::Doctor => doctor(&cfg.workspace),
    };
    match outcome {
        Ok(report) => {
            println!("{}", pretty(report.value));
            if report.failed {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(failure) => {
            eprintln!("swarm-control: {}", failure.message);
            failure.code
        }
    }
}

/// JSON imprimé sur stdout, avec un code de sortie qui dit si le diagnostic est sain.
struct Report {
    value: Value,
    /// `true` pour `doctor` : des problèmes ont été trouvés (code 1).
    failed: bool,
}

fn report(value: Value) -> Report {
    Report { value, failed: false }
}

fn pretty(value: Value) -> String {
    serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string())
}

/// Runner visé : `--runner` s'il est donné (et connu), sinon `None`.
fn wanted_runner(options: &Options) -> Result<Option<Runner>, Failure> {
    match options.runner.as_deref() {
        Some(r) => Runner::parse(r).map(Some).ok_or_else(|| {
            Failure::usage(format!("--runner: claude, hermes ou antigravity attendu (reçu « {r} »)"))
        }),
        None => Ok(None),
    }
}

// ---- agents ---------------------------------------------------------------

/// Les agents du workspace, avec les résidents vivants annoncés par le registre.
fn agents_list(ws: &Path) -> Result<Report, Failure> {
    let announced = peers::discover();
    let list: Vec<Value> = agents::list(ws)
        .iter()
        .map(|a| {
            let resident = announced
                .iter()
                .find(|p| p.agent == a.name && p.alive)
                .map(|p| json!({"url": p.url, "pid": p.pid}));
            json!({
                "name": a.name,
                "runner": a.runner.as_str(),
                "model": a.model,
                "resident": resident,
            })
        })
        .collect();
    Ok(report(json!({"count": list.len(), "agents": list})))
}

// ---- run ------------------------------------------------------------------

fn run_turn(
    ws: &Path,
    options: &Options,
    agent: &str,
    prompt: String,
) -> Result<Report, Failure> {
    let wanted = wanted_runner(options)?;
    let runner = agents::resolve(ws, agent, wanted).map_err(Failure::run)?;
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
    // Résident ailleurs ? Le tour lui est confié (le client MCP minimal fait l'appel) ;
    // sinon, run local : bail de run pris par `prepare`, session par `execute`.
    if let Some(routed) = routing::forward(&req) {
        debug::line("mcp", format!("run {agent} → hôte {}", routed.host.instance));
        let failed = routed.result.error.is_some();
        return Ok(Report {
            value: run_json(&req, &routed.result, Some(routed.host.instance)),
            failed,
        });
    }
    let live =
        runners::prepare(&req).map_err(|r| Failure::run(r.error.unwrap_or_else(|| "run impossible".into())))?;
    let result = runners::execute(&req, &live, None);
    Ok(Report { value: run_json(&req, &result, None), failed: result.error.is_some() })
}

/// La réponse d'un tour au format de l'outil `run_agent`.
fn run_json(req: &RunRequest, result: &RunResult, routed_to: Option<String>) -> Value {
    json!({
        "agent": req.agent,
        "runner": req.runner.as_str(),
        "routedTo": routed_to,
        "reply": result.result,
        "sessionId": result.session_id,
        "error": result.error,
    })
}

// ---- runs -----------------------------------------------------------------

/// Les runs vivants de ce process, et le dernier run de chaque agent sur disque.
fn runs(ws: &Path) -> Result<Report, Failure> {
    let live: Vec<Value> = registry::all().iter().map(live_json).collect();
    let mut last = Vec::new();
    for agent in agents::list(ws) {
        if let Some((runner, run)) =
            registry::disk_entry(ws, &agent.name, Some(agent.runner.as_str()))
        {
            last.push(json!({"agent": agent.name, "runner": runner, "run": run}));
        }
    }
    Ok(report(json!({"live": live, "last": last})))
}

/// Un run vivant au format de l'outil `agent_control` (list).
fn live_json(s: &registry::Snapshot) -> Value {
    json!({
        "runId": s.run_id,
        "runner": s.runner,
        "agentName": s.agent,
        "pid": s.pid,
        "status": s.status.as_str(),
        "exitCode": s.exit_code,
        "sessionId": s.session_id,
        "startedAt": s.started_at,
        "lastOutputAt": s.last_output_at,
    })
}

// ---- doctor ---------------------------------------------------------------

/// Diagnostic du workspace : annonces du registre, baux d'hébergement, runs
/// « running » sans process vivant. Code de sortie 1 si des problèmes existent.
fn doctor(ws: &Path) -> Result<Report, Failure> {
    let listed = agents::list(ws);
    let announced = peers::discover();
    let mut issues: Vec<String> = Vec::new();

    // Annonce du registre sans process vivant derrière.
    for peer in &announced {
        if !peer.alive {
            issues.push(format!(
                "{} annoncé dans le registre mais son process ({}) ne répond plus",
                peer.name, peer.pid
            ));
        }
    }
    // Bail d'hébergement vivant sans annonce correspondante (le registre vit dans
    // TEMP : une annonce peut manquer après un nettoyage, pas un bail).
    for agent in &listed {
        if let Some(host) = owners::host_of(ws, agent.runner.as_str(), &agent.name) {
            let live_announce = announced.iter().any(|p| p.agent == agent.name && p.alive);
            if !live_announce {
                issues.push(format!(
                    "{} : bail d'hébergement vivant ({}) mais absent du registre",
                    agent.name, host.instance
                ));
            }
        }
    }
    // Dernier run « running » sur disque sans process vivant : orphelin.
    for agent in &listed {
        if let Some((runner, run)) =
            registry::disk_entry(ws, &agent.name, Some(agent.runner.as_str()))
        {
            let running = run.get("status").and_then(Value::as_str) == Some("running");
            let pid = run.get("pid").and_then(Value::as_u64).map(|p| p as u32);
            if running && pid.is_none_or(|p| !crate::process::pid_alive(p)) {
                issues.push(format!(
                    "{} : dernier run « running » sans process vivant (runner {runner})",
                    agent.name
                ));
            }
        }
    }
    Ok(Report {
        value: json!({
            "workspace": ws,
            "agents": listed.len(),
            "announced": announced.len(),
            "issues": issues,
        }),
        failed: !issues.is_empty(),
    })
}
