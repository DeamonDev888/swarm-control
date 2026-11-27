//! Mode agent : ce process ne sert qu'un agent résident.
//!
//! Pas de serveur d'administration : seulement le serveur MCP de l'agent (`swarm-<agent>`,
//! `ask` / `status` / `stop`) et, avec `--bridge N`, le pont HTTP compatible
//! `overmind-bridge` (`/send`, `/reset`, `/status`, `/health`).

use super::args::Options;
use crate::{agents, config, instance, registry, residents};
use std::process::ExitCode;

pub fn start(options: Options) -> ExitCode {
    match serve(options) {
        Ok(never) => match never {},
        Err(e) => {
            eprintln!("swarm-control: {e}");
            ExitCode::FAILURE
        }
    }
}

fn serve(options: Options) -> Result<std::convert::Infallible, String> {
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
    // Les serveurs tournent dans leurs propres fils : celui-ci n'a plus qu'à durer.
    loop {
        std::thread::park();
    }
}
