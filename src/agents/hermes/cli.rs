//! Appel du CLI `hermes` pour administrer les profils.

use super::exec::run_with_timeout;
use crate::{binaries, config, process};
use std::process::Stdio;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(120);

/// Exécute `hermes <args>` avec le home natif. `Err` = sortie d'erreur.
pub fn run(args: &[&str]) -> Result<String, String> {
    let mut cmd = process::command(&binaries::hermes_binary());
    cmd.args(args)
        .env("HERMES_HOME", config::native_hermes_home())
        .env("PYTHONIOENCODING", "utf-8")
        .stdin(Stdio::null());
    let (ok, output) = run_with_timeout(cmd, TIMEOUT)?;
    if ok { Ok(output) } else { Err(output.trim().to_string()) }
}
