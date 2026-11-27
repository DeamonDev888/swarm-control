//! Un PID désigne-t-il un process vivant ?

use std::process::Stdio;

#[cfg(windows)]
pub fn pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    let filter = format!("PID eq {pid}");
    super::command(std::path::Path::new("tasklist"))
        .args(["/FI", &filter, "/FO", "CSV", "/NH"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains(&format!("\"{pid}\"")))
        .unwrap_or(false)
}

/// PID utilisable avec `kill` : `0` vise notre propre groupe, et au-delà de
/// `i32::MAX` la valeur repasse en négatif (`u32::MAX` devient `-1` : tous les process).
#[cfg(unix)]
pub(super) fn valid_unix_pid(pid: u32) -> bool {
    pid != 0 && i32::try_from(pid).is_ok()
}

#[cfg(unix)]
pub fn pid_alive(pid: u32) -> bool {
    if !valid_unix_pid(pid) {
        return false;
    }
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}
