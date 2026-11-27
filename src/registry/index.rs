//! Index global des runs vivants (mémoire).

use super::{LiveRun, Snapshot, Status};
use crate::owners::{self, Busy};
use crate::store::now_ms;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

type Map = HashMap<String, Arc<LiveRun>>;

/// Durée de rétention d'un run terminé.
const FINISHED_TTL_MS: u64 = 60 * 60 * 1000;
/// Runners connus (pour retrouver un agent sans préciser son runner).
const RUNNERS: [&str; 3] = ["claude", "hermes", "antigravity"];

static LIVE: Mutex<Option<Map>> = Mutex::new(None);
static RUN_COUNTER: AtomicU64 = AtomicU64::new(0);

fn live() -> MutexGuard<'static, Option<Map>> {
    LIVE.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn key(runner: &str, agent: &str) -> String {
    format!("{runner}:{agent}")
}

/// Ouvre un run. Un agent nommé ne peut avoir qu'un run vivant à la fois — dans
/// ce process (table en mémoire) **et** dans tout autre (bail sur disque) :
/// deux reprises simultanées de la même session la corrompraient.
pub fn begin(
    runner: &'static str,
    agent: Option<&str>,
    workspace: &Path,
) -> Result<Arc<LiveRun>, String> {
    let n = RUN_COUNTER.fetch_add(1, Ordering::Relaxed);
    let now = now_ms();
    let run_id = format!("run-{now}-{n}");
    let key = key(runner, &agent.map_or_else(|| format!("_anon_{run_id}"), String::from));
    let mut guard = live();
    let map = guard.get_or_insert_with(HashMap::new);
    if let Some(existing) = map.get(&key) {
        if existing.snapshot().status == Status::Running {
            return Err(busy_here(agent, runner, &existing.run_id));
        }
    }
    let lease = match agent {
        Some(a) => Some(
            owners::acquire(workspace, runner, a, &run_id)
                .map_err(|b| busy_elsewhere(b, a, runner))?,
        ),
        None => None,
    };
    let run = Arc::new(LiveRun::new(run_id, runner, agent, workspace, now));
    run.attach_lease(lease);
    map.insert(key, Arc::clone(&run));
    Ok(run)
}

fn busy_here(agent: Option<&str>, runner: &str, run_id: &str) -> String {
    format!(
        "AGENT_BUSY: l'agent '{}' ({runner}) a déjà un run en cours ({run_id}). \
         Utilise agent_control (wait/kill) avant d'en relancer un.",
        agent.unwrap_or("?")
    )
}

fn busy_elsewhere(busy: Busy, agent: &str, runner: &str) -> String {
    match busy {
        Busy::Held(h) => format!(
            "AGENT_BUSY: l'agent '{agent}' ({runner}) tourne déjà dans une autre instance \
             ({}, run {}). Attends sa fin ou arrête-le là-bas (agent_control kill).",
            h.instance, h.run_id
        ),
        Busy::Pending => format!(
            "AGENT_BUSY: l'agent '{agent}' ({runner}) est en cours de prise en charge par \
             une autre instance : réessaie dans un instant."
        ),
        Busy::Io(e) => format!("LEASE_ERROR: bail de l'agent '{agent}' impossible ({e})."),
    }
}

/// Run vivant (ou récemment terminé) d'un agent. Sans runner : le plus récent.
pub fn find(agent: &str, runner: Option<&str>) -> Option<Arc<LiveRun>> {
    let guard = live();
    let map = guard.as_ref()?;
    let runners: Vec<&str> = runner.map_or_else(|| RUNNERS.to_vec(), |r| vec![r]);
    runners.iter().filter_map(|r| map.get(&key(r, agent))).max_by_key(|run| run.started_at).cloned()
}

pub fn find_by_run_id(run_id: &str) -> Option<Arc<LiveRun>> {
    live().as_ref()?.values().find(|r| r.run_id == run_id).cloned()
}

/// Tous les runs connus, du plus récent au plus ancien.
pub fn all() -> Vec<Snapshot> {
    let guard = live();
    let mut out: Vec<Snapshot> =
        guard.as_ref().map(|m| m.values().map(|r| r.snapshot()).collect()).unwrap_or_default();
    out.sort_by_key(|s| std::cmp::Reverse(s.started_at));
    out
}

/// Workspaces des runs connus.
pub fn workspaces() -> HashSet<PathBuf> {
    live().as_ref().map(|m| m.values().map(|r| r.workspace.clone()).collect()).unwrap_or_default()
}

/// Battement de cœur des baux de tous les runs vivants.
pub fn heartbeat() {
    let runs: Vec<Arc<LiveRun>> =
        live().as_ref().map(|m| m.values().cloned().collect()).unwrap_or_default();
    for run in runs {
        run.heartbeat();
    }
}

/// Oublie les runs terminés depuis plus d'une heure ; renvoie les PID encore actifs.
pub fn prune_finished(now: u64) -> HashSet<u32> {
    let mut active = HashSet::new();
    if let Some(map) = live().as_mut() {
        map.retain(|_, run| keep(run, now, &mut active));
    }
    active
}

fn keep(run: &LiveRun, now: u64, active: &mut HashSet<u32>) -> bool {
    let snap = run.snapshot();
    if snap.status == Status::Running {
        active.extend(snap.pid);
        return true;
    }
    run.finished_at().is_none_or(|t| now.saturating_sub(t) < FINISHED_TTL_MS)
}
