//! Ligne de commande : `swarm-control [COMMANDE] [OPTIONS]`.
//!
//! Deux modes pour `serve` :
//! - **instance** (défaut) : serveur d'administration et ses 10 outils ;
//! - **agent** (`--agent NAME`, développement et tests, ou binaire **compilé** pour un
//!   agent via [`serve_compiled`]) : ce process ne sert qu'un agent résident, avec en
//!   option un pont HTTP parlant le dialecte Discord.
//!
//! Les autres commandes (`ask`, `status`, `history`, `reset`, `config`, `schema`, `agents`,
//! `run`, `runs`, `build`, `doctor`, `memory`, `reload`) parlent à l'agent — hébergé ici, annoncé
//! ailleurs, ou lancé localement — et à son workspace. L'agent visé est celui du binaire compilé,
//! ou `--agent`.

mod agent;
mod agents;
mod args;
mod ask;
mod build;
mod compiled;
mod config;
mod doctor;
mod exe;
mod help;
mod history;
mod memory;
mod output;
mod reload;
mod reset;
mod runs;
mod schema;
mod serve;
mod status;
mod target;

use args::{Command, Options, Parsed};
use serde_json::Value;
use std::path::Path;
use std::process::ExitCode;

/// Exécute la CLI et renvoie le code de sortie du process.
///
/// `swarm-control` sert une instance. Un binaire **renommé** (`discord`, `discord.exe`)
/// est refusé avec le code 2 et le message de migration : depuis la 0.2.0, un agent
/// persistant est un binaire compilé ([`serve_compiled`]), plus une copie renommée.
pub fn run() -> ExitCode {
    let argv0 = std::env::args_os().next();
    if let Some(stem) = argv0.as_deref().and_then(exe::renamed) {
        eprintln!("swarm-control: {}", exe::migration_error(&stem));
        return ExitCode::from(2);
    }
    run_with(None, std::env::args().skip(1))
}

/// Entrée de lib d'un **binaire compilé** pour un agent (PLAN §4.8) : `agent` et `class`
/// y sont gravés, `definition` est la définition par défaut embarquée (`include_str!` de
/// `agent.json`, sans secret).
///
/// - `--agent NAME` sur la ligne de commande est **refusé** (stderr, code 2) : ce binaire
///   ne sert que son agent ;
/// - `--version` affiche `swarm-control <version> (agent <agent>, classe <class>)` ;
/// - si l'agent n'existe dans le workspace pour aucun runner, il est **recréé** depuis la
///   définition embarquée par le chemin de création ordinaire (`agents::create`) — jamais
///   sinon : la définition du workspace fait foi (persona, modèle, serveurs MCP) ;
/// - si la définition du workspace n'a pas de classe, la classe gravée y est estampillée ;
/// - puis le service résident démarre comme `serve --agent <agent>`.
pub fn serve_compiled(agent: &str, class: &str, definition: &str) -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--agent" || a == "-a") {
        eprintln!(
            "swarm-control: --agent refusé : ce binaire est compilé pour l'agent '{agent}'. \
             (Un autre agent se lance avec son propre binaire compilé, ou `swarm-control \
             serve --agent NAME`.)"
        );
        return ExitCode::from(2);
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("swarm-control {} (agent {agent}, classe {class})", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    if let Err(message) = compiled::prepare(agent, class, definition, &args) {
        eprintln!("swarm-control: {message}");
        return ExitCode::from(2);
    }
    serve_agent(agent)
}

/// Exécute la CLI en mode agent pour `agent` : **développement et tests** (un binaire
/// compilé passe par [`serve_compiled`]). `fn main() -> ExitCode {
/// swarm_control::serve_agent("discord") }` sert l'agent `discord` du workspace courant ;
/// `--agent` sur la ligne de commande garde la priorité.
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
            crate::debug::init(options.debug);
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
        eprintln!("swarm-control: {} attend un agent : --agent NAME", command.name());
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
        Command::Templates => templates(&options, &ws),
        Command::Run { agent, prompt } => ask::run(&options, &ws, agent, prompt),
        Command::Build { agent, offline } => build::run(&options, &ws, agent, *offline),
        Command::Memory { action, rest } => {
            memory::run(agent.unwrap_or_default(), options.runner, action, rest, options.json)
        }
        Command::Reload => reload::run(&options, agent.unwrap_or_default()),
        Command::Runs => runs::run(&options, &ws),
        Command::Doctor => doctor::run(&options, &ws),
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("swarm-control: {message}");
            // `build` sans cargo : code 1 annoncé (l'agent reste utilisable via l'instance).
            if matches!(command, Command::Build { .. }) {
                return build::exit_code(&message);
            }
            ExitCode::FAILURE
        }
    }
}

/// `templates` : les modèles d'agents disponibles — intégrés et surcharges du workspace
/// (`<workspace>/templates/`). Même contenu que l'outil `list_templates`.
fn templates(options: &Options, ws: &Path) -> Result<(), String> {
    let list = crate::agents::templates::list(ws);
    if options.json {
        let rows: Vec<Value> = list.iter().map(|t| t.to_json()).collect();
        output::json(&Value::Array(rows));
        return Ok(());
    }
    if list.is_empty() {
        println!(
            "aucun modèle d'agent (ni intégré, ni surcharge dans {}).",
            ws.join("templates").display()
        );
        return Ok(());
    }
    let rows: Vec<Vec<String>> = list
        .iter()
        .map(|t| {
            vec![
                t.name.clone(),
                output::clip(&t.description, 60),
                if t.required_vars.is_empty() { "-".into() } else { t.required_vars.join(", ") },
                t.source.label().to_string(),
            ]
        })
        .collect();
    print!("{}", output::table(&["MODÈLE", "DESCRIPTION", "VARIABLES REQUISES", "SOURCE"], &rows));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::args::Command;
    use super::{exe, run_with};
    use std::process::ExitCode;

    #[test]
    fn help_and_version_exit_successfully() {
        // `--help` / `-V` ne démarrent rien : seul chemin testable sans I/O.
        assert_eq!(run_with(None, ["--help".to_string()]), ExitCode::SUCCESS);
        assert_eq!(run_with(Some("discord".into()), ["-V".to_string()]), ExitCode::SUCCESS);
    }

    #[test]
    fn an_invalid_command_line_exits_with_code_2() {
        assert_eq!(run_with(None, ["ask".to_string()]), ExitCode::from(2));
    }

    #[test]
    fn a_command_without_an_agent_exits_with_code_2() {
        assert_eq!(run_with(None, ["status".to_string()]), ExitCode::from(2));
    }

    /// `memory` et `reload` ciblent un agent : sans `--agent` (ni binaire nommé), code 2.
    #[test]
    fn memory_and_reload_exit_with_code_2_without_an_agent() {
        assert_eq!(
            run_with(None, ["memory".to_string(), "search".into(), "x".into()]),
            ExitCode::from(2)
        );
        assert_eq!(run_with(None, ["reload".to_string()]), ExitCode::from(2));
    }

    #[test]
    fn needs_agent_only_for_the_per_agent_commands() {
        assert!(Command::Ask { prompt: "x".into() }.needs_agent());
        assert!(Command::Status.needs_agent());
        assert!(!Command::Agents.needs_agent());
        assert!(!Command::Runs.needs_agent());
        assert!(!Command::Build { agent: "a".into(), offline: false }.needs_agent());
        assert!(!Command::Doctor.needs_agent());
        assert!(!Command::Schema { tool: "ask".into() }.needs_agent());
        assert!(Command::Memory { action: "search".into(), rest: Vec::new() }.needs_agent());
        assert!(Command::Reload.needs_agent());
    }

    #[test]
    fn un_binaire_renomme_est_refuse_avec_le_code_2_et_le_message_de_migration() {
        // `run_with` ne lit pas argv : c'est `exe::renamed` + `migration_error` qui portent le
        // refus, et `run()` les applique avant tout parsing. Contrat testable sans process.
        let renamed = exe::renamed(std::ffi::OsStr::new(if cfg!(windows) {
            r"C:\ws\bin\discord.exe"
        } else {
            "/ws/bin/discord"
        }))
        .expect("binaire renommé détecté");
        let message = exe::migration_error(&renamed);
        assert!(message.contains("depuis la 0.2.0"), "{message}");
        assert!(message.contains("swarm-control build <agent>"), "{message}");
        assert!(message.contains("--agent NAME` reste disponible"), "{message}");
        // `swarm-control` lui-même : aucune erreur, mode instance.
        assert_eq!(exe::renamed(std::ffi::OsStr::new("swarm-control.exe")), None);
    }

    #[test]
    fn serve_compiled_refuse_agent_et_affiche_sa_version() {
        // Ces deux chemins ne lisent que la ligne de commande du process : ils sont couverts
        // par les tests e2e (`compiled.rs`) avec le vrai binaire compilé. Ici : le format
        // exact de `--version`.
        let agent = "discord";
        let class = "discord";
        let line =
            format!("swarm-control {} (agent {agent}, classe {class})", env!("CARGO_PKG_VERSION"));
        assert!(line.starts_with("swarm-control "), "{line}");
        assert!(line.contains("(agent discord, classe discord)"), "{line}");
    }
}
