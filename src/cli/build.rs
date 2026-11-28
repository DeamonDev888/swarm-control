//! `swarm-control build <agent>` : recompile au premier plan le binaire dédié d'un agent
//! (PLAN §4.8) — à lancer pour changer de nom, de classe ou de version de swarm-control.
//!
//! Le journal de `cargo` s'affiche et reste dans `<ws>/logs/build-<agent>.log`. Sans
//! `cargo` : erreur claire avec le lien rustup, code 1.

use super::args::Options;
use crate::agents::{self, compile};
use std::path::Path;
use std::process::ExitCode;

/// Code de sortie quand `cargo` est absent (l'agent reste utilisable via l'instance).
pub const NO_CARGO: u8 = 1;

pub fn run(options: &Options, ws: &Path, agent: &str, offline: bool) -> Result<(), String> {
    let _ = options;
    agents::validate_name(agent).map_err(|e| format!("build {agent}: {e}"))?;
    let Some(cargo) = compile::cargo_path() else {
        return Err(compile::no_cargo_message());
    };
    eprintln!("compilation de '{agent}' avec {} (workspace {})", cargo.display(), ws.display());
    let outcome = compile::build(ws, agent, &compile::BuildOptions { offline })?;
    println!(
        "binaire : {} ({} ms) — journal : {}",
        outcome.binary.display(),
        outcome.elapsed_ms,
        outcome.log.display()
    );
    Ok(())
}

/// Code de sortie de la commande : 1 sans `cargo` (erreur annoncée, pas un plantage),
/// code général sinon.
pub fn exit_code(error: &str) -> ExitCode {
    if error == compile::no_cargo_message() {
        return ExitCode::from(NO_CARGO);
    }
    ExitCode::FAILURE
}

#[cfg(test)]
mod tests {
    use super::{NO_CARGO, exit_code, run};
    use crate::cli::args::Options;
    use std::path::Path;
    use std::process::ExitCode;

    #[test]
    fn un_nom_invalide_est_refuse_avant_toute_compilation() {
        let err = run(&Options::default(), Path::new("ws"), "../x", false).unwrap_err();
        assert!(err.contains("Nom d'agent invalide"), "{err}");
    }

    #[test]
    fn sans_cargo_le_code_est_1_et_le_message_porte_rustup() {
        let message = crate::agents::compile::no_cargo_message();
        assert_eq!(exit_code(&message), ExitCode::from(NO_CARGO));
        assert_eq!(exit_code("autre erreur"), ExitCode::FAILURE);
        assert_eq!(NO_CARGO, 1);
        assert!(message.contains("https://rustup.rs"), "{message}");
    }
}
