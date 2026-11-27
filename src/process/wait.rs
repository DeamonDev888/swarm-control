//! Attente de la fin d'un process, avec délai, arrêt et annulation.

use super::{End, kill_tree};
use crate::registry::LiveRun;
use litemcp::cancel::CancelToken;
use std::process::Child;
use std::sync::Arc;
use std::time::{Duration, Instant};

const POLL: Duration = Duration::from_millis(100);
const PROGRESS_EVERY: Duration = Duration::from_secs(10);

/// Bloque jusqu'à la fin du process ; le tue si besoin. Renvoie code et cause.
pub fn until_done(
    child: &mut Child,
    live: &Arc<LiveRun>,
    timeout: Duration,
    cancel: Option<&CancelToken>,
) -> (Option<i32>, End) {
    let pid = child.id();
    let started = Instant::now();
    let mut last_progress = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return (status.code(), End::Exited),
            Ok(None) => {}
            Err(_) => return (None, End::Exited),
        }
        if let Some(reason) = stop_reason(live, cancel, started, timeout) {
            kill_tree(pid);
            let _ = child.kill();
            let status = child.wait().ok();
            return (status.and_then(|s| s.code()), reason);
        }
        if last_progress.elapsed() >= PROGRESS_EVERY {
            last_progress = Instant::now();
            report_progress(started);
        }
        std::thread::sleep(POLL);
    }
}

fn stop_reason(
    live: &LiveRun,
    cancel: Option<&CancelToken>,
    started: Instant,
    timeout: Duration,
) -> Option<End> {
    if live.kill_requested() {
        Some(End::Killed)
    } else if cancel.is_some_and(CancelToken::is_cancelled) {
        Some(End::Cancelled)
    } else if started.elapsed() >= timeout {
        Some(End::TimedOut)
    } else {
        None
    }
}

/// Sans jeton de progression côté client, c'est une non-action.
fn report_progress(started: Instant) {
    let secs = started.elapsed().as_secs();
    litemcp::progress::report(secs as f64, None, Some(&format!("agent en cours depuis {secs}s")));
}
