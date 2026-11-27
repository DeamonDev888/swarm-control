//! Démarrage du serveur.

use super::args::Options;
use crate::{agents, config, instance, registry, residents, server};
use std::path::Path;
use std::process::ExitCode;

pub fn start(options: Options) -> ExitCode {
    if options.agent.is_some() {
        return super::agent::start(options);
    }
    if options.bridge.is_some() {
        eprintln!(
            "swarm-control: --bridge sert un seul agent : ajoute --agent NAME (ou renomme l'exécutable d'après l'agent)"
        );
        return ExitCode::from(2);
    }
    let number = match instance::claim(options.instance) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("swarm-control: {e}");
            return ExitCode::FAILURE;
        }
    };
    let cfg = config::init(options.workspace);
    registry::start_sweeper(cfg.workspace.clone());
    if let Err(e) = host_residents(&options.residents, &cfg.workspace) {
        eprintln!("swarm-control: {e}");
        return ExitCode::FAILURE;
    }
    match server::serve(&instance::name(number), &options.host, options.port, &cfg.workspace) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("swarm-control: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Rend résidents les agents demandés par `--resident` ; la première erreur arrête le démarrage.
fn host_residents(names: &[String], ws: &Path) -> Result<(), String> {
    for name in names {
        let runner =
            agents::resolve(ws, name, None).map_err(|e| format!("--resident {name}: {e}"))?;
        let info =
            residents::start(ws, runner, name).map_err(|e| format!("--resident {name}: {e}"))?;
        eprintln!("agent résident {} ({}) → {}", info.agent, info.runner, info.url);
    }
    Ok(())
}
