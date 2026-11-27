//! Ligne de commande : `swarm-control [--port N] [--host H] [--workspace DIR]`.

mod args;
mod help;
mod serve;

use args::Parsed;
use std::process::ExitCode;

/// Exécute la CLI et renvoie le code de sortie du process.
pub fn run() -> ExitCode {
    match args::parse(std::env::args().skip(1)) {
        Ok(Parsed::Serve(options)) => serve::start(options),
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
