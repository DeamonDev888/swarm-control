//! Ligne de commande : `swarm-control [--port N] [--host H] [--workspace DIR] …`.
//!
//! Deux modes :
//! - **instance** (défaut) : serveur d'administration et ses 10 outils ;
//! - **agent** (`--agent NAME`, ou binaire renommé d'après l'agent) : ce process ne sert
//!   qu'un agent résident, avec en option un pont HTTP compatible `overmind-bridge`.

mod agent;
mod args;
mod exe;
mod help;
mod serve;

use args::Parsed;
use std::process::ExitCode;

/// Exécute la CLI et renvoie le code de sortie du process.
///
/// Le nom de l'exécutable choisit le mode : `swarm-control` sert une instance, un binaire
/// renommé (`sniper`) sert l'agent du même nom (voir [`serve_agent`]).
pub fn run() -> ExitCode {
    let mut argv = std::env::args_os();
    let default_agent = argv.next().as_deref().and_then(exe::agent_name);
    run_with(default_agent, std::env::args().skip(1))
}

/// Exécute la CLI en mode agent pour `agent` : c'est le `main` d'un exécutable propre à un
/// agent (`fn main() -> ExitCode { swarm_control::serve_agent("sniper") }`). `--agent` sur la
/// ligne de commande garde la priorité.
pub fn serve_agent(agent: &str) -> ExitCode {
    run_with(Some(agent.to_string()), std::env::args().skip(1))
}

fn run_with(default_agent: Option<String>, args: impl IntoIterator<Item = String>) -> ExitCode {
    match args::parse(args) {
        Ok(Parsed::Serve(mut options)) => {
            if options.agent.is_none() {
                options.agent = default_agent;
            }
            serve::start(options)
        }
        Ok(Parsed::Help) => {
            println!("{}", help::text());
            ExitCode::SUCCESS
        }
        Ok(Parsed::Version) => {
            println!("swarm-control {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("swarm-control: {message}");
            ExitCode::from(2)
        }
    }
}
