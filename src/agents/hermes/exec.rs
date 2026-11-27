//! Exécution d'une commande courte avec délai dur.

use std::io::Read;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Lance `cmd` et renvoie `(succès, stdout+stderr)`. Ne bloque jamais plus de `timeout`.
pub fn run_with_timeout(mut cmd: Command, timeout: Duration) -> Result<(bool, String), String> {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("lancement impossible: {e}"))?;
    let out = drain(child.stdout.take());
    let err = drain(child.stderr.take());
    let status = wait(&mut child, timeout)?;
    let text = format!("{}{}", out.join().unwrap_or_default(), err.join().unwrap_or_default());
    match status {
        Some(s) => Ok((s.success(), text)),
        None => Err(format!("délai dépassé ({}s)", timeout.as_secs())),
    }
}

fn drain<R: Read + Send + 'static>(stream: Option<R>) -> JoinHandle<String> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut s) = stream {
            let _ = s.read_to_end(&mut bytes);
        }
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

/// `None` si le délai est dépassé (le process est alors tué).
fn wait(child: &mut Child, timeout: Duration) -> Result<Option<ExitStatus>, String> {
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(Some(status)),
            Ok(None) if start.elapsed() < timeout => std::thread::sleep(Duration::from_millis(50)),
            Ok(None) => {
                crate::process::kill_tree(child.id());
                let _ = child.kill();
                let _ = child.wait();
                return Ok(None);
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}
