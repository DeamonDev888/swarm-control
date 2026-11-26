//! OverMind-rs — agents persistants (Claude Code, Hermes) servis en MCP par LiteMCP.
//!
//! ```text
//! overmind [--port 23080] [--workspace <dossier>] [--host 127.0.0.1]
//! ```
//!
//! Sans `--port`, le premier port libre de 23080..=23099 est pris.

mod agents;
mod config;
mod process;
mod registry;
mod runners;
mod sessions;
mod store;
mod tools;

use litemcp::prelude::*;
use std::path::PathBuf;

struct Cli {
    port: Option<u16>,
    host: String,
    workspace: Option<PathBuf>,
}

fn parse_cli() -> Result<Cli, String> {
    let mut cli = Cli { port: None, host: "127.0.0.1".into(), workspace: None };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or_else(|| format!("{name} attend une valeur"));
        match arg.as_str() {
            "--port" | "-p" => {
                cli.port = Some(value("--port")?.parse().map_err(|_| "--port: nombre 1-65535 attendu".to_string())?);
            }
            "--host" => cli.host = value("--host")?,
            "--workspace" | "-w" => cli.workspace = Some(PathBuf::from(value("--workspace")?)),
            "--help" | "-h" => {
                println!(
                    "overmind {} — agents persistants (Claude Code, Hermes) en MCP\n\n\
                     USAGE: overmind [--port N] [--host H] [--workspace DIR]\n\n\
                     --port N         port d'écoute (défaut: premier libre de 23080..=23099)\n\
                     --host H         adresse d'écoute (défaut 127.0.0.1)\n\
                     --workspace DIR  racine OverMind (.claude/, .mcp.json, bridge/)",
                    env!("CARGO_PKG_VERSION")
                );
                std::process::exit(0);
            }
            "--version" | "-V" => {
                println!("overmind {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            other => return Err(format!("option inconnue: {other} (voir --help)")),
        }
    }
    Ok(cli)
}

fn main() -> std::io::Result<()> {
    let cli = match parse_cli() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("overmind: {e}");
            std::process::exit(2);
        }
    };
    let cfg = config::init(cli.workspace);
    registry::start_sweeper(cfg.workspace.clone());

    let builder = ServerBuilder::new("overmind", env!("CARGO_PKG_VERSION"))
        // Un run d'agent dure des minutes : pas de plafond de 30 s sur les handlers.
        // Chaque outil borne lui-même son exécution (OVERMIND_TIMEOUT_MS, wait).
        .no_tool_timeout();
    let builder = tools::register(builder);

    let bound = match cli.port {
        Some(port) => builder.bind(&format!("{}:{port}", cli.host))?,
        None => builder.bind_default()?,
    };
    eprintln!(
        "overmind {} — MCP sur http://{}/mcp  (workspace: {})",
        env!("CARGO_PKG_VERSION"),
        bound.local_addr()?,
        cfg.workspace.display()
    );
    bound.serve()
}
