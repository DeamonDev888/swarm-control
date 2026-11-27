//! Le détenteur d'un bail, tel qu'écrit dans le fichier.

use crate::process;
use crate::store::now_ms;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;

/// Sans battement de cœur depuis 2 minutes, un bail est périmé.
pub const STALE_MS: u64 = 120_000;
/// Un fichier plus jeune est peut-être en cours d'écriture par son créateur.
const YOUNG: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Holder {
    pub instance: String,
    pub pid: u32,
    /// Identifiant du run (bail de run) ou jeton d'hébergement (bail d'hébergement).
    pub run_id: String,
    pub ts: u64,
    /// Renseignés par les baux d'hébergement seulement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

impl Holder {
    /// Ce process, pour le run `run_id`, maintenant.
    pub fn current(run_id: &str) -> Self {
        Holder {
            instance: instance_name(),
            pid: std::process::id(),
            run_id: run_id.to_string(),
            ts: now_ms(),
            ..Default::default()
        }
    }

    /// Ce process, hébergeant `agent` (l'URL est ajoutée une fois le serveur lié).
    pub fn hosting(token: &str, runner: &str, agent: &str) -> Self {
        Holder {
            runner: Some(runner.to_string()),
            agent: Some(agent.to_string()),
            ..Self::current(token)
        }
    }

    pub fn is_stale(&self, now: u64) -> bool {
        !process::pid_alive(self.pid) || now.saturating_sub(self.ts) > STALE_MS
    }
}

static NAME: OnceLock<String> = OnceLock::new();

/// Fixe le nom de cette instance (`swarm-control-N`) ; sans effet après le premier appel.
pub fn set_instance_name(name: String) {
    let _ = NAME.set(name);
}

/// Nom de cette instance : `swarm-control-N` une fois numérotée, `swarm-control@<pid>` avant
/// (tests unitaires, usage en bibliothèque).
pub fn instance_name() -> String {
    NAME.get().cloned().unwrap_or_else(|| format!("swarm-control@{}", std::process::id()))
}

pub fn read(path: &Path) -> Option<Holder> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// Fichier créé il y a moins de 5 s (son contenu peut ne pas être écrit).
pub fn is_young(path: &Path) -> bool {
    path.metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age < YOUNG)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn holder(pid: u32, ts: u64) -> Holder {
        Holder { instance: "x".into(), pid, run_id: "r".into(), ts, ..Default::default() }
    }

    #[test]
    fn a_live_fresh_holder_is_not_stale() {
        assert!(!holder(std::process::id(), now_ms()).is_stale(now_ms()));
    }

    #[test]
    fn a_dead_pid_is_stale() {
        assert!(holder(u32::MAX, now_ms()).is_stale(now_ms()));
    }

    #[test]
    fn a_silent_holder_is_stale_even_if_its_pid_lives() {
        let old = now_ms() - STALE_MS - 1;
        assert!(holder(std::process::id(), old).is_stale(now_ms()));
    }

    #[test]
    fn optional_fields_are_omitted_for_run_leases_and_roundtrip_for_hosts() {
        let run = serde_json::to_string(&Holder::current("r1")).unwrap();
        assert!(!run.contains("url") && !run.contains("agent"));
        let mut host = Holder::hosting("t", "claude", "alice");
        host.url = Some("http://127.0.0.1:1/mcp".into());
        let back: Holder = serde_json::from_str(&serde_json::to_string(&host).unwrap()).unwrap();
        assert_eq!(back, host);
    }
}
