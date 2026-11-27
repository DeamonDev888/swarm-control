//! Ligne de commande : `swarm-control [COMMANDE] [OPTIONS]`.
//!
//! Deux modes pour `serve` :
//! - **instance** (défaut) : serveur d'administration et ses 10 outils ;
//! - **agent** (`--agent NAME`, ou binaire renommé d'après l'agent) : ce process ne sert
//!   qu'un agent résident, avec en option un pont HTTP compatible `overmind-bridge`.
//!
//! Les autres commandes (`ask`, `status`, `history`, `reset`, `config`, `schema`, `agents`,
//! `run`, `runs`, `doctor`) parlent à l'agent — hébergé ici, annoncé ailleurs, ou lancé
//! localement — et à son workspace. L'agent visé est celui de l'exécutable (`sniper`), ou
//! `--agent`.

mod agent;
mod agents;
mod args;
mod ask;
mod config;
mod doctor;
mod exe;
mod help;
mod history;
mod output;
mod reset;
mod runs;
mod schema;
mod serve;
mod status;
mod target;

use args::{Command, Parsed};
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
    let (mut options, command) = match args::parse(args) {
        Ok(Parsed::Help) => {
            println!("{}", help::text());
            return ExitCode::SUCCESS;
        }
        Ok(Parsed::Version) => {
            println!("swarm-control {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Ok(Parsed::Serve(mut options)) => {
            if options.agent.is_none() {
                options.agent = default_agent;
            }
            return serve::start(options);
        }
        Ok(Parsed::Command(options, command)) => (options, command),
        Err(message) => {
            eprintln!("swarm-control: {message}");
            return ExitCode::from(2);
        }
    };
    crate::debug::init(options.debug);
    if options.agent.is_none() {
        options.agent = default_agent;
    }
    if command.needs_agent() && options.agent.is_none() {
        eprintln!(
            "swarm-control: {} attend un agent : --agent NAME, ou un exécutable renommé d'après \
             l'agent (sniper, sniper.exe)",
            command.name()
        );
        return ExitCode::from(2);
    }
    let agent = options.agent.clone();
    let agent = agent.as_deref();
    let ws = crate::config::init(options.workspace.clone()).workspace.clone();
    let outcome = match &command {
        Command::Ask { prompt } => ask::run(&options, &ws, agent.unwrap_or_default(), prompt),
        Command::Status => status::run(&options, &ws, agent.unwrap_or_default()),
        Command::History { limit, before } => {
            history::run(&options, &ws, agent.unwrap_or_default(), *limit, *before)
        }
        Command::Reset => reset::run(&options, &ws, agent.unwrap_or_default()),
        Command::Config { file } => {
            config::run(&options, &ws, agent.unwrap_or_default(), file.as_deref())
        }
        Command::Schema { tool } => schema::run(&options, agent, tool),
        Command::Agents => agents::run(&options, &ws),
        Command::Run { agent, prompt } => ask::run(&options, &ws, agent, prompt),
        Command::Runs => runs::run(&options, &ws),
        Command::Doctor => doctor::run(&options, &ws),
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("swarm-control: {message}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::args::Command;
    use super::run_with;
    use std::process::ExitCode;

    #[test]
    fn help_and_version_exit_successfully() {
        // `--help` / `-V` ne démarrent rien : seul chemin testable sans I/O.
        assert_eq!(run_with(None, ["--help".to_string()]), ExitCode::SUCCESS);
        assert_eq!(run_with(Some("sniper".into()), ["-V".to_string()]), ExitCode::SUCCESS);
    }

    #[test]
    fn an_invalid_command_line_exits_with_code_2() {
        assert_eq!(run_with(None, ["ask".to_string()]), ExitCode::from(2));
    }

    #[test]
    fn a_command_without_an_agent_exits_with_code_2() {
        assert_eq!(run_with(None, ["status".to_string()]), ExitCode::from(2));
    }

    #[test]
    fn needs_agent_only_for_the_per_agent_commands() {
        assert!(Command::Ask { prompt: "x".into() }.needs_agent());
        assert!(Command::Status.needs_agent());
        assert!(!Command::Agents.needs_agent());
        assert!(!Command::Runs.needs_agent());
        assert!(!Command::Doctor.needs_agent());
        assert!(!Command::Schema { tool: "ask".into() }.needs_agent());
    }
}
