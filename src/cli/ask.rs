//! `ask` et `run` : un tour pour un agent.
//!
//! Trois chemins, du plus proche au plus lointain :
//! 1. l'agent est **hébergé par ce process** → l'outil `ask` de son propre serveur MCP, qui met
//!    un tour dans sa file et attend (même file, même session) ;
//! 2. un **pair vivant** est annoncé (`swarm-<agent>`) → l'outil `ask` de son serveur MCP ;
//! 3. sinon → un tour **local** (`runners::prepare` + `runners::execute`), même bail et même
//!    session que l'hôte : l'agent se souvient.
//!
//! Le tour hébergé passe par le serveur de l'agent (et non par `runners::execute`) parce que
//! `Turn` n'est constructible que dans `residents` : c'est la file de l'hôte qui garantit un
//! tour à la fois sur sa session.
//!
//! La réponse va sur stdout ; les erreurs sortent en code 1.

use super::args::{ASK_MAX_TIMEOUT_MS, Options};
use super::target;
use crate::owners;
use crate::peers::{self, CallError};
use crate::residents::Ctx;
use crate::runners::{self, RunRequest, RunResult};
use serde_json::json;
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

/// Après une demande d'arrêt, le temps laissé au run pour tuer son processus.
const KILL_GRACE: Duration = Duration::from_secs(5);

pub fn run(options: &Options, ws: &Path, agent: &str, prompt: &str) -> Result<(), String> {
    target::validate(agent)?;
    if prompt.trim().is_empty() {
        return Err("prompt vide.".into());
    }
    if let Some(ctx) = target::hosted(agent, options.runner) {
        return hosted(&ctx, prompt, options);
    }
    if let Some(peer) = target::peer(agent) {
        return remote(&peer.url, agent, prompt, options);
    }
    local(options, ws, agent, prompt)
}

/// Tour confié à l'hôte local : son propre outil `ask`, qui le met dans la même file (un tour à
/// la fois) et attend la réponse.
fn hosted(ctx: &Arc<Ctx>, prompt: &str, options: &Options) -> Result<(), String> {
    match ctx.url() {
        Some(url) => remote(url, &ctx.agent, prompt, options),
        None => Err(format!("agent '{}' hébergé ici mais sans URL MCP.", ctx.agent)),
    }
}

/// Tour confié au serveur MCP d'un agent (même outil `ask` que les agents entre eux).
fn remote(url: &str, agent: &str, prompt: &str, options: &Options) -> Result<(), String> {
    let mut args = json!({
        "prompt": prompt,
        "from": owners::instance_name(),
        "timeoutMs": options.timeout_ms.clamp(1_000, ASK_MAX_TIMEOUT_MS),
    });
    if let Some(model) = &options.model {
        args["model"] = json!(model);
    }
    let wait = Duration::from_millis(options.timeout_ms).saturating_add(Duration::from_secs(60));
    match peers::call_tool(url, "ask", args, wait) {
        Ok(reply) => {
            println!("{}", reply["reply"].as_str().unwrap_or_default());
            Ok(())
        }
        Err(CallError::Tool(message) | CallError::Protocol(message)) => Err(message),
        Err(CallError::Unreachable(why)) => {
            Err(format!("agent '{agent}' annoncé mais injoignable ({why})."))
        }
    }
}

/// Tour local : le run prend le bail de l'agent et reprend sa session, comme l'hôte.
///
/// Le run tourne dans un fil pour que le délai demandé puisse l'interrompre (process tué).
fn local(options: &Options, ws: &Path, agent: &str, prompt: &str) -> Result<(), String> {
    let runner = target::runner(ws, agent, options.runner)?;
    let request = RunRequest {
        runner,
        prompt: prompt.to_string(),
        agent: Some(agent.to_string()),
        session_id: None,
        auto_resume: true,
        model: options.model.clone(),
        provider: None,
        cwd: None,
        workspace: ws.to_path_buf(),
    };
    let live = runners::prepare(&request)
        .map_err(|result| result.error.unwrap_or_else(|| "run refusé.".into()))?;
    let (sender, receiver) = mpsc::channel::<RunResult>();
    let running = Arc::clone(&live);
    std::thread::Builder::new()
        .name("swarm-cli-run".into())
        .spawn(move || {
            let result = runners::execute(&request, &running, None);
            let _ = sender.send(result);
        })
        .map_err(|e| format!("thread de run impossible: {e}"))?;
    match receiver.recv_timeout(Duration::from_millis(options.timeout_ms)) {
        Ok(result) => match result.error {
            Some(error) => Err(error),
            None => {
                println!("{}", result.result);
                Ok(())
            }
        },
        Err(RecvTimeoutError::Disconnected) => {
            Err("le run s'est interrompu sans rendre de résultat.".into())
        }
        Err(RecvTimeoutError::Timeout) => {
            live.request_kill();
            // Laisse au run le temps de tuer son process avant que ce process-ci ne sorte.
            let _ = receiver.recv_timeout(KILL_GRACE);
            Err(format!("pas de réponse après {} ms : le run est interrompu.", options.timeout_ms))
        }
    }
}
