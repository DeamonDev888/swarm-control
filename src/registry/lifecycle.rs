//! Étapes de vie d'un run : démarrage du process, session, fin.
//!
//! Seuls les agents nommés sont écrits sur disque (fichier d'état de l'agent).

use super::{LiveRun, Outcome, Status, disk};
use crate::store::now_ms;
use serde_json::json;

impl LiveRun {
    /// Un nouveau process (premier lancement ou retente) démarre.
    pub fn set_pid(&self, pid: u32) {
        self.state().pid = Some(pid);
        self.write_disk(|run| {
            *run = json!({
                "runId": self.run_id,
                "pid": pid,
                "status": "running",
                "exitCode": null,
                "ts": now_ms(),
            });
        });
    }

    /// Comme [`set_pid`](Self::set_pid), sans rien écrire sur disque : pour les runs de
    /// service (compilation d'agent), qui ne décrivent pas un tour de l'agent.
    pub fn set_pid_quiet(&self, pid: u32) {
        self.state().pid = Some(pid);
    }

    pub fn set_session(&self, session_id: &str) {
        self.state().session_id = Some(session_id.to_string());
    }

    pub fn finish(&self, status: Status, exit_code: Option<i32>, outcome: Outcome) {
        {
            let mut s = self.state();
            s.status = status;
            s.exit_code = exit_code;
            s.finished_at = Some(now_ms());
            s.outcome = Some(outcome);
        }
        self.write_disk(|run| {
            run["status"] = json!(status.disk_str());
            run["exitCode"] = json!(exit_code);
            run["lastOutputAt"] = json!(now_ms());
        });
        self.release_lease();
    }

    /// Comme [`finish`](Self::finish), sans rien écrire sur disque : la fin d'un run de
    /// service (compilation d'agent) ne doit pas devenir le `lastRun` de l'agent.
    pub fn finish_quiet(&self, status: Status, exit_code: Option<i32>, outcome: Outcome) {
        let mut s = self.state();
        s.status = status;
        s.exit_code = exit_code;
        s.finished_at = Some(now_ms());
        s.outcome = Some(outcome);
    }

    /// Modifie `lastRun` dans le fichier d'état de l'agent.
    fn write_disk(&self, edit: impl FnOnce(&mut serde_json::Value)) {
        if let Some(agent) = &self.agent {
            disk::record(&self.workspace, self.runner, agent, edit);
        }
    }
}
