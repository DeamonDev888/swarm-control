//! Mode agent : ce process ne sert qu'un agent résident.
//!
//! Pas de serveur d'administration : seulement le serveur MCP de l'agent (`swarm-<agent>`,
//! `ask` / `status` / `stop`) et, avec `--bridge N`, le pont HTTP compatible `overmind-bridge`
//! (`/send`, `/reset`, `/status`, `/health`).
//!
//! Une fois l'agent en place :
//! - **terminal** et sans `--headless` → la TUI (`crate::tui::run`) ; à sa fermeture l'agent est
//!   arrêté proprement (`residents::stop`) et le process sort ;
//! - sinon (`--headless`, stdout redirigé, systemd) → bannière puis attente, sans interface.
//!
//! Une TUI qui échoue ne tue pas l'agent : message, puis comportement headless.

use super::args::Options;
use crate::residents::Ctx;
use crate::{agents, config, instance, registry, residents};
use std::io::IsTerminal;
use std::process::ExitCode;
use std::sync::Arc;

pub fn start(options: Options) -> ExitCode {
    match boot(options) {
        Ok((ctx, headless, debug)) => after_boot(ctx, headless, debug),
        Err(e) => {
            eprintln!("swarm-control: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Démarre la résidence et annonce l'URL MCP (`… MCP sur http://127.0.0.1:PORT/mcp …`).
fn boot(options: Options) -> Result<(Arc<Ctx>, bool, bool), String> {
    let agent = options.agent.clone().unwrap_or_default();
    agents::validate_name(&agent).map_err(|e| format!("agent: {e}"))?;
    if !options.residents.is_empty() {
        return Err("--resident ne se combine pas avec le mode agent (un agent par process)".into());
    }
    instance::claim(options.instance)?;
    let cfg = config::init(options.workspace);
    registry::start_sweeper(cfg.workspace.clone());

    let runner =
        agents::resolve(&cfg.workspace, &agent, None).map_err(|e| format!("agent {agent}: {e}"))?;
    let listen = options.port.map(|port| format!("{}:{port}", options.host));
    let info = residents::start_on(&cfg.workspace, runner, &agent, listen.as_deref())
        .map_err(|e| format!("agent {agent}: {e}"))?;
    let bridge = match options.bridge {
        Some(port) => {
            let addr = residents::open_bridge(runner, &agent, &format!("{}:{port}", options.host))
                .map_err(|e| format!("--bridge {port}: {e}"))?;
            format!(" · pont HTTP sur http://{addr}")
        }
        None => String::new(),
    };
    // Une ligne contenant l'URL MCP, comme la bannière d'une instance (lue par les tests).
    eprintln!(
        "{} {} — MCP sur {}  (agent: {agent}, runner: {}, workspace: {}){bridge}",
        info.name,
        env!("CARGO_PKG_VERSION"),
        info.url,
        info.runner,
        cfg.workspace.display()
    );
    let ctx = residents::ctx_of(&agent, runner)
        .ok_or_else(|| format!("agent {agent}: contexte résident introuvable après démarrage"))?;
    Ok((ctx, options.headless, options.debug))
}

fn after_boot(ctx: Arc<Ctx>, headless: bool, debug: bool) -> ExitCode {
    if !headless && std::io::stdout().is_terminal() {
        match open_tui(Arc::clone(&ctx), debug) {
            Ok(()) => {
                stop(&ctx);
                return ExitCode::SUCCESS;
            }
            Err(e) => eprintln!("swarm-control: TUI indisponible ({e}) : mode headless"),
        }
    }
    // Les serveurs tournent dans leurs propres fils : celui-ci n'a plus qu'à durer.
    loop {
        std::thread::park();
    }
}

/// Arrêt propre : serveur MCP rendu, file vidée, bail d'hébergement libéré.
fn stop(ctx: &Ctx) {
    match residents::stop(&ctx.workspace, &ctx.agent, Some(ctx.runner)) {
        Ok(_) => eprintln!("agent {} arrêté.", ctx.agent),
        Err(e) => eprintln!("swarm-control: arrêt de l'agent: {e}"),
    }
}

#[cfg(feature = "tui")]
fn open_tui(ctx: Arc<Ctx>, debug: bool) -> Result<(), String> {
    // `Quit` comme `Stop` ferment l'agent : la TUI est son interface au premier plan.
    crate::tui::run(ctx, debug).map(|_exit| ())
}

#[cfg(not(feature = "tui"))]
fn open_tui(_ctx: Arc<Ctx>, _debug: bool) -> Result<(), String> {
    Err("binaire compilé sans la feature `tui`".into())
}
