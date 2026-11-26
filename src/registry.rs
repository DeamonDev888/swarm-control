//! Registre des runs d'agents.
//!
//! Deux couches, comme côté TS :
//! - **mémoire** : chaque run vivant (`LiveRun`) avec son PID, son statut, sa
//!   session et la fin de sa sortie horodatée — lu par `agent_control` ;
//! - **disque** : `<workspace>/bridge/process-registry.json`, même format que
//!   l'OverMind TS (`{ "processes": { "claude:x": {...} }, "version": 2 }`),
//!   pour retrouver un process après redémarrage du serveur.
//!
//! Un balayeur passe toutes les 30 s : les PID morts deviennent `orphaned`,
//! les runs terminés depuis plus d'une heure sont oubliés.

use crate::process;
use crate::store::{now_ms, read_json, write_json_atomic};
use serde_json::{Map, Value, json};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

const REGISTRY_FILE: &str = "bridge/process-registry.json";
/// Sortie gardée en mémoire par run (fin de flux).
const MAX_OUTPUT_BYTES: usize = 256 * 1024;
/// Durée de rétention d'un run terminé (mémoire et disque).
const FINISHED_TTL_MS: u64 = 60 * 60 * 1000;
const SWEEP_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Running,
    Done,
    Failed,
    Killed,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Running => "running",
            Status::Done => "done",
            Status::Failed => "failed",
            Status::Killed => "killed",
        }
    }
    /// Valeur écrite sur disque (vocabulaire TS : pas de `killed`).
    fn disk_str(self) -> &'static str {
        match self {
            Status::Killed => "failed",
            other => other.as_str(),
        }
    }
}

/// Résultat final d'un run, gardé pour `agent_control status/wait`.
#[derive(Clone, Debug, Default)]
pub struct Outcome {
    pub result: String,
    pub error: Option<String>,
}

struct State {
    pid: Option<u32>,
    status: Status,
    exit_code: Option<i32>,
    session_id: Option<String>,
    chunks: VecDeque<(u64, String)>,
    bytes: usize,
    last_output_at: u64,
    finished_at: Option<u64>,
    outcome: Option<Outcome>,
}

pub struct LiveRun {
    pub run_id: String,
    pub key: String,
    pub runner: &'static str,
    pub agent: Option<String>,
    pub workspace: PathBuf,
    pub started_at: u64,
    kill_requested: AtomicBool,
    state: Mutex<State>,
}

/// Vue figée d'un run, pour l'affichage.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub run_id: String,
    pub runner: &'static str,
    pub agent: Option<String>,
    pub pid: Option<u32>,
    pub status: Status,
    pub exit_code: Option<i32>,
    pub session_id: Option<String>,
    pub started_at: u64,
    pub last_output_at: u64,
    pub outcome: Option<Outcome>,
}

static LIVE: Mutex<Option<HashMap<String, Arc<LiveRun>>>> = Mutex::new(None);
static DISK_LOCK: Mutex<()> = Mutex::new(());
static RUN_COUNTER: AtomicU64 = AtomicU64::new(0);

fn live() -> MutexGuard<'static, Option<HashMap<String, Arc<LiveRun>>>> {
    LIVE.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn key(runner: &str, agent: &str) -> String {
    format!("{runner}:{agent}")
}

/// Ouvre un run. Un agent nommé ne peut avoir qu'un run vivant à la fois :
/// deux reprises simultanées de la même session la corrompraient.
pub fn begin(runner: &'static str, agent: Option<&str>, workspace: &Path) -> Result<Arc<LiveRun>, String> {
    let n = RUN_COUNTER.fetch_add(1, Ordering::Relaxed);
    let run_id = format!("run-{}-{n}", now_ms());
    let key = match agent {
        Some(a) => key(runner, a),
        None => key(runner, &format!("_anon_{run_id}")),
    };
    let mut guard = live();
    let map = guard.get_or_insert_with(HashMap::new);
    if let Some(existing) = map.get(&key) {
        if existing.snapshot().status == Status::Running {
            return Err(format!(
                "AGENT_BUSY: l'agent '{}' ({runner}) a déjà un run en cours ({}). \
                 Utilise agent_control (wait/kill) avant d'en relancer un.",
                agent.unwrap_or("?"),
                existing.run_id
            ));
        }
    }
    let now = now_ms();
    let run = Arc::new(LiveRun {
        run_id,
        key: key.clone(),
        runner,
        agent: agent.map(String::from),
        workspace: workspace.to_path_buf(),
        started_at: now,
        kill_requested: AtomicBool::new(false),
        state: Mutex::new(State {
            pid: None,
            status: Status::Running,
            exit_code: None,
            session_id: None,
            chunks: VecDeque::new(),
            bytes: 0,
            last_output_at: now,
            finished_at: None,
            outcome: None,
        }),
    });
    map.insert(key, Arc::clone(&run));
    Ok(run)
}

impl LiveRun {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn snapshot(&self) -> Snapshot {
        let s = self.state();
        Snapshot {
            run_id: self.run_id.clone(),
            runner: self.runner,
            agent: self.agent.clone(),
            pid: s.pid,
            status: s.status,
            exit_code: s.exit_code,
            session_id: s.session_id.clone(),
            started_at: self.started_at,
            last_output_at: s.last_output_at,
            outcome: s.outcome.clone(),
        }
    }

    /// Un nouveau process (premier lancement ou retente) démarre.
    pub fn set_pid(&self, pid: u32) {
        let session = {
            let mut s = self.state();
            s.pid = Some(pid);
            s.session_id.clone()
        };
        if self.agent.is_some() {
            self.write_disk(|entry| {
                *entry = json!({
                    "id": session.unwrap_or_default(),
                    "ts": now_ms(),
                    "pid": pid,
                    "runner": self.runner,
                    "agentName": self.agent,
                    "status": "running",
                    "exitCode": null,
                });
            });
        }
    }

    pub fn set_session(&self, session_id: &str) {
        self.state().session_id = Some(session_id.to_string());
        if self.agent.is_some() {
            self.write_disk(|entry| {
                entry["id"] = json!(session_id);
                entry["ts"] = json!(now_ms());
            });
        }
    }

    pub fn push_output(&self, chunk: &str) {
        if chunk.is_empty() {
            return;
        }
        let now = now_ms();
        let mut s = self.state();
        s.bytes += chunk.len();
        s.chunks.push_back((now, chunk.to_string()));
        s.last_output_at = now;
        while s.bytes > MAX_OUTPUT_BYTES && s.chunks.len() > 1 {
            if let Some((_, old)) = s.chunks.pop_front() {
                s.bytes -= old.len();
            }
        }
    }

    /// Sortie accumulée depuis `since_ms` (toute la fenêtre si `None`).
    pub fn output_since(&self, since_ms: Option<u64>) -> String {
        let s = self.state();
        s.chunks
            .iter()
            .filter(|(ts, _)| since_ms.is_none_or(|since| *ts > since))
            .map(|(_, c)| c.as_str())
            .collect()
    }

    pub fn request_kill(&self) {
        self.kill_requested.store(true, Ordering::SeqCst);
    }

    pub fn kill_requested(&self) -> bool {
        self.kill_requested.load(Ordering::SeqCst)
    }

    pub fn finish(&self, status: Status, exit_code: Option<i32>, outcome: Outcome) {
        {
            let mut s = self.state();
            s.status = status;
            s.exit_code = exit_code;
            s.finished_at = Some(now_ms());
            s.outcome = Some(outcome);
        }
        if self.agent.is_some() {
            self.write_disk(|entry| {
                entry["status"] = json!(status.disk_str());
                entry["exitCode"] = json!(exit_code);
                entry["lastOutputAt"] = json!(now_ms());
            });
        }
    }

    fn write_disk(&self, edit: impl FnOnce(&mut Value)) {
        let _guard = DISK_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let path = self.workspace.join(REGISTRY_FILE);
        let mut store = load_store(&path);
        let processes = store["processes"].as_object_mut().expect("processes est un objet");
        let entry = processes.entry(self.key.clone()).or_insert_with(|| json!({}));
        if !entry.is_object() {
            *entry = json!({});
        }
        edit(entry);
        let _ = write_json_atomic(&path, &store);
    }
}

fn load_store(path: &Path) -> Value {
    match read_json(path) {
        Some(mut v) if v.get("processes").is_some_and(Value::is_object) => {
            if v.get("version").is_none() {
                v["version"] = json!(2);
            }
            v
        }
        _ => json!({ "processes": {}, "version": 2 }),
    }
}

/// Run vivant (ou récemment terminé) d'un agent. Sans runner : le plus récent
/// des deux runners.
pub fn find(agent: &str, runner: Option<&str>) -> Option<Arc<LiveRun>> {
    let guard = live();
    let map = guard.as_ref()?;
    let candidates: Vec<&Arc<LiveRun>> = match runner {
        Some(r) => map.get(&key(r, agent)).into_iter().collect(),
        None => ["claude", "hermes"].iter().filter_map(|r| map.get(&key(r, agent))).collect(),
    };
    candidates.into_iter().max_by_key(|r| r.started_at).cloned()
}

pub fn find_by_run_id(run_id: &str) -> Option<Arc<LiveRun>> {
    live().as_ref()?.values().find(|r| r.run_id == run_id).cloned()
}

/// Tous les runs connus en mémoire.
pub fn all() -> Vec<Snapshot> {
    let guard = live();
    let mut out: Vec<Snapshot> = guard.as_ref().map(|m| m.values().map(|r| r.snapshot()).collect()).unwrap_or_default();
    out.sort_by_key(|s| std::cmp::Reverse(s.started_at));
    out
}

/// Entrée disque d'un agent (process lancé par une vie précédente du serveur).
pub fn disk_entry(workspace: &Path, agent: &str, runner: Option<&str>) -> Option<(String, Value)> {
    let _guard = DISK_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let store = load_store(&workspace.join(REGISTRY_FILE));
    let processes = store["processes"].as_object()?;
    let runners: Vec<&str> = runner.map(|r| vec![r]).unwrap_or_else(|| vec!["claude", "hermes"]);
    runners
        .into_iter()
        .filter_map(|r| {
            let k = key(r, agent);
            processes.get(&k).map(|v| (k, v.clone()))
        })
        .max_by_key(|(_, v)| v.get("ts").and_then(Value::as_u64).unwrap_or(0))
}

/// Marque une entrée disque (process hors de ce serveur) comme arrêtée.
pub fn mark_disk_killed(workspace: &Path, disk_key: &str) {
    let _guard = DISK_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let path = workspace.join(REGISTRY_FILE);
    let mut store = load_store(&path);
    if let Some(entry) = store["processes"].get_mut(disk_key) {
        entry["status"] = json!("failed");
        entry["exitCode"] = Value::Null;
        entry["lastOutputAt"] = json!(now_ms());
        let _ = write_json_atomic(&path, &store);
    }
}

/// Un passage du balayeur sur la mémoire et sur les fichiers des workspaces.
pub fn sweep(workspaces: &HashSet<PathBuf>) {
    let now = now_ms();
    let mut active_pids = HashSet::new();
    {
        let mut guard = live();
        if let Some(map) = guard.as_mut() {
            map.retain(|_, run| {
                let s = run.state();
                if s.status == Status::Running {
                    if let Some(pid) = s.pid {
                        active_pids.insert(pid);
                    }
                    return true;
                }
                s.finished_at.is_none_or(|t| now.saturating_sub(t) < FINISHED_TTL_MS)
            });
        }
    }
    for ws in workspaces {
        let _guard = DISK_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let path = ws.join(REGISTRY_FILE);
        if !path.exists() {
            continue;
        }
        let mut store = load_store(&path);
        let Some(processes) = store["processes"].as_object_mut() else { continue };
        let before = processes.clone();
        let mut kept = Map::new();
        for (k, mut entry) in std::mem::take(processes) {
            let status = entry.get("status").and_then(Value::as_str).unwrap_or("").to_string();
            let pid = entry.get("pid").and_then(Value::as_u64).map(|p| p as u32);
            if status == "running" {
                if let Some(pid) = pid {
                    if !active_pids.contains(&pid) && !process::pid_alive(pid) {
                        entry["status"] = json!("orphaned");
                        entry["lastOutputAt"] = json!(now);
                    }
                }
            }
            let still_running = entry.get("status").and_then(Value::as_str) == Some("running");
            let last = entry
                .get("lastOutputAt")
                .or_else(|| entry.get("ts"))
                .and_then(Value::as_u64)
                .unwrap_or(0);
            if still_running || now.saturating_sub(last) < FINISHED_TTL_MS {
                kept.insert(k, entry);
            }
        }
        if kept != before {
            *processes = kept;
            let _ = write_json_atomic(&path, &store);
        } else {
            *processes = kept;
        }
        cleanup_tmp_debris(&path);
    }
}

/// Supprime les `process-registry.json.tmp.*` laissés par l'ancien serveur.
fn cleanup_tmp_debris(registry: &Path) {
    let Some(dir) = registry.parent() else { return };
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("process-registry.json.tmp.") || name.starts_with("agents.json.tmp.") {
            let old = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age > Duration::from_secs(300));
            if old {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

/// Lance le balayeur en arrière-plan (un passage immédiat, puis toutes les 30 s).
pub fn start_sweeper(default_workspace: PathBuf) {
    std::thread::Builder::new()
        .name("overmind-sweeper".into())
        .spawn(move || loop {
            let mut workspaces: HashSet<PathBuf> = live()
                .as_ref()
                .map(|m| m.values().map(|r| r.workspace.clone()).collect())
                .unwrap_or_default();
            workspaces.insert(default_workspace.clone());
            sweep(&workspaces);
            std::thread::sleep(SWEEP_INTERVAL);
        })
        .expect("thread du balayeur");
}
