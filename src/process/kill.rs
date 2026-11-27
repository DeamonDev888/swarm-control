//! Arrêt d'un process et de ses descendants.

use std::process::Stdio;

/// Tue un process et tous ses descendants.
#[cfg(windows)]
pub fn kill_tree(pid: u32) {
    let _ = super::command(std::path::Path::new("taskkill"))
        .args(["/F", "/T", "/PID", &pid.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Tue un groupe de processus : SIGTERM, puis SIGKILL après 2 s de grâce.
#[cfg(unix)]
pub fn kill_tree(pid: u32) {
    use std::time::{Duration, Instant};
    // 1 = init : jamais un process d'agent ; 0 et > i32::MAX : voir `valid_unix_pid`.
    if pid == 1 || !super::alive::valid_unix_pid(pid) {
        return;
    }
    let group = format!("-{pid}");
    let signal = |sig: &str| {
        let _ = std::process::Command::new("kill")
            .args([sig, "--", &group])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    };
    signal("-TERM");
    let deadline = Instant::now() + Duration::from_secs(2);
    while super::pid_alive(pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
    if super::pid_alive(pid) {
        signal("-KILL");
    }
}
