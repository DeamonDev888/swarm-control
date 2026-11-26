//! Lancement et surveillance des processus d'agents (sans runtime async).
//!
//! Chaque flux (stdout, stderr) est lu par un fil dédié qui alimente à la fois
//! le tampon final et la sortie vivante du registre. Le fil appelant attend la
//! fin du process en surveillant : délai, demande d'arrêt (`agent_control
//! kill`) et annulation de la requête MCP (fermeture du flux SSE).

use crate::registry::LiveRun;
use litemcp::cancel::CancelToken;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Plafond du tampon final par flux (fin de flux conservée).
const MAX_CAPTURE: usize = 10 * 1024 * 1024;
const POLL: Duration = Duration::from_millis(100);
const PROGRESS_EVERY: Duration = Duration::from_secs(10);

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum End {
    Exited,
    TimedOut,
    Killed,
    Cancelled,
}

#[derive(Debug)]
pub struct ProcOutput {
    pub stdout: String,
    pub stderr: String,
    pub code: Option<i32>,
    pub end: End,
}

/// Prépare une commande sans fenêtre console (Windows) et dans son propre
/// groupe de processus (Unix), pour pouvoir tuer tout l'arbre.
pub fn command(program: &std::path::Path) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd
}

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
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    let pid = child.id();
    live.set_pid(pid);

    let out_buf = Arc::new(Mutex::new(Vec::new()));
    let err_buf = Arc::new(Mutex::new(Vec::new()));
    let readers: Vec<JoinHandle<()>> = [
        child.stdout.take().map(|s| reader(s, Arc::clone(live), Arc::clone(&out_buf))),
        child.stderr.take().map(|s| reader(s, Arc::clone(live), Arc::clone(&err_buf))),
    ]
    .into_iter()
    .flatten()
    .collect();

    if let (Some(data), Some(mut stdin)) = (stdin_data, child.stdin.take()) {
        // Écriture dans un fil : un prompt plus gros que le tampon du pipe ne
        // doit pas bloquer la surveillance.
        let data = data.to_owned();
        std::thread::spawn(move || {
            let _ = stdin.write_all(data.as_bytes());
            // stdin fermé ici : le CLI sait que le prompt est complet.
        });
    }

    let started = Instant::now();
    let mut last_progress = Instant::now();
    let (code, end) = loop {
        match child.try_wait() {
            Ok(Some(status)) => break (status.code(), End::Exited),
            Ok(None) => {}
            Err(_) => break (None, End::Exited),
        }
        let reason = if live.kill_requested() {
            Some(End::Killed)
        } else if cancel.is_some_and(CancelToken::is_cancelled) {
            Some(End::Cancelled)
        } else if started.elapsed() >= timeout {
            Some(End::TimedOut)
        } else {
            None
        };
        if let Some(reason) = reason {
            kill_tree(pid);
            let _ = child.kill();
            let status = child.wait().ok();
            break (status.and_then(|s| s.code()), reason);
        }
        if last_progress.elapsed() >= PROGRESS_EVERY {
            last_progress = Instant::now();
            let secs = started.elapsed().as_secs();
            // Sans jeton de progression côté client, c'est une non-action.
            litemcp::progress::report(secs as f64, None, Some(&format!("agent en cours depuis {secs}s")));
        }
        std::thread::sleep(POLL);
    };

    // Les lecteurs finissent quand les pipes se ferment. Un petit-enfant
    // détaché peut garder un pipe ouvert : on n'attend pas plus de 3 s.
    let deadline = Instant::now() + Duration::from_secs(3);
    for handle in readers {
        while !handle.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        if handle.is_finished() {
            let _ = handle.join();
        }
    }
    let take = |buf: &Arc<Mutex<Vec<u8>>>| {
        let bytes = buf.lock().unwrap_or_else(|e| e.into_inner());
        String::from_utf8_lossy(&bytes).into_owned()
    };
    Ok(ProcOutput { stdout: take(&out_buf), stderr: take(&err_buf), code, end })
}

fn reader<R: Read + Send + 'static>(mut src: R, live: Arc<LiveRun>, sink: Arc<Mutex<Vec<u8>>>) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        let mut carry: Vec<u8> = Vec::new();
        loop {
            let n = match src.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            {
                let mut s = sink.lock().unwrap_or_else(|e| e.into_inner());
                s.extend_from_slice(&buf[..n]);
                if s.len() > MAX_CAPTURE {
                    let excess = s.len() - MAX_CAPTURE;
                    s.drain(..excess);
                }
            }
            carry.extend_from_slice(&buf[..n]);
            let text = take_utf8(&mut carry);
            live.push_output(&text);
        }
        if !carry.is_empty() {
            live.push_output(&String::from_utf8_lossy(&carry));
        }
    })
}

/// Extrait la partie décodable de `carry` ; garde une séquence UTF-8
/// incomplète en fin de tampon pour le prochain morceau.
fn take_utf8(carry: &mut Vec<u8>) -> String {
    match std::str::from_utf8(carry) {
        Ok(s) => {
            let out = s.to_owned();
            carry.clear();
            out
        }
        Err(e) if e.error_len().is_none() => {
            let valid = e.valid_up_to();
            let out = String::from_utf8_lossy(&carry[..valid]).into_owned();
            carry.drain(..valid);
            out
        }
        Err(_) => {
            let out = String::from_utf8_lossy(carry).into_owned();
            carry.clear();
            out
        }
    }
}

/// Tue un process et tous ses descendants.
pub fn kill_tree(pid: u32) {
    #[cfg(windows)]
    {
        let _ = command(std::path::Path::new("taskkill"))
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(unix)]
    {
        let group = format!("-{pid}");
        let signal = |sig: &str| {
            let _ = Command::new("kill")
                .args([sig, "--", &group])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        };
        signal("-TERM");
        let deadline = Instant::now() + Duration::from_secs(2);
        while pid_alive(pid) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
        }
        if pid_alive(pid) {
            signal("-KILL");
        }
    }
}

/// Le PID désigne-t-il un process vivant ?
pub fn pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    #[cfg(windows)]
    {
        let filter = format!("PID eq {pid}");
        command(std::path::Path::new("tasklist"))
            .args(["/FI", &filter, "/FO", "CSV", "/NH"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains(&format!("\"{pid}\"")))
            .unwrap_or(false)
    }
    #[cfg(unix)]
    {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_carry_across_chunks() {
        let bytes = "é".as_bytes();
        let mut carry = vec![b'a', bytes[0]];
        assert_eq!(take_utf8(&mut carry), "a");
        assert_eq!(carry, vec![bytes[0]]);
        carry.push(bytes[1]);
        assert_eq!(take_utf8(&mut carry), "é");
        assert!(carry.is_empty());
    }
}
