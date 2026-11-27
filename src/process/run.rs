//! Lancement d'un process d'agent et capture de sa sortie.

use super::{ProcOutput, readers, stdin, wait};
use crate::registry::LiveRun;
use litemcp::cancel::CancelToken;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

/// Lance `cmd`, écrit `stdin_data` puis ferme stdin, et attend la fin.
/// `Err` seulement si le process n'a pas pu démarrer.
pub fn run(
    mut cmd: Command,
    stdin_data: Option<&str>,
    live: &Arc<LiveRun>,
    timeout: Duration,
    cancel: Option<&CancelToken>,
) -> Result<ProcOutput, String> {
    cmd.stdin(if stdin_data.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // PLAN §4.2 : une ligne par commande lancée, secrets masqués.
    let program = std::path::Path::new(cmd.get_program()).to_path_buf();
    let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
    crate::debug::line("commande", crate::debug::command_line(&program, &args));
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    live.set_pid(child.id());

    let (out, err) = (readers::new_capture(), readers::new_capture());
    let handles = readers::start(&mut child, live, &out, &err);
    stdin::feed(child.stdin.take(), stdin_data);

    let (code, end) = wait::until_done(&mut child, live, timeout, cancel);
    readers::join_all(handles);
    Ok(ProcOutput { stdout: readers::text(&out), stderr: readers::text(&err), code, end })
}
