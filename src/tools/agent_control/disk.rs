//! Run absent de la mémoire : repli sur l'état de l'agent sur disque.
//!
//! Le run tourne dans une autre instance, ou le serveur a redémarré depuis son
//! lancement : on connaît son PID et son statut, pas sa sortie.

use super::args::ControlArgs;
use crate::process;
use crate::registry;
use crate::tools::errors::invalid;
use litemcp::prelude::{ToolError, ToolResult};
use serde_json::{Value, json};
use std::path::Path;

pub fn from_disk(args: &ControlArgs, workspace: &Path) -> ToolResult {
    let Some(name) = args.agent_name.as_deref() else {
        return Err(invalid(format!("run introuvable: {}", args.run_id.as_deref().unwrap_or("?"))));
    };
    let Some((runner, entry)) = registry::disk_entry(workspace, name, args.runner.as_deref())
    else {
        return Err(invalid(format!("aucun run connu pour l'agent '{name}'")));
    };
    let pid = entry.get("pid").and_then(Value::as_u64).map(|p| p as u32);
    let alive = pid.is_some_and(process::pid_alive);
    let recorded = entry.get("status").and_then(Value::as_str).unwrap_or("unknown");
    let status = if recorded == "running" && !alive { "orphaned" } else { recorded };
    match args.action.as_str() {
        "status" | "stream" | "wait" => Ok(describe(name, pid, status, alive, &entry)),
        "kill" => Ok(kill(workspace, &runner, name, pid, alive && recorded == "running", status)),
        other => Err(ToolError::invalid_input(format!("action inconnue '{other}'"))),
    }
}

fn describe(name: &str, pid: Option<u32>, status: &str, alive: bool, entry: &Value) -> Value {
    json!({
        "source": "disk",
        "agentName": name,
        "pid": pid,
        "status": status,
        "alive": alive,
        "sessionId": entry.get("id"),
        "note": "run hors de ce process (autre instance ou serveur redémarré) : pas de sortie en direct disponible",
    })
}

fn kill(
    workspace: &Path,
    runner: &str,
    agent: &str,
    pid: Option<u32>,
    killable: bool,
    status: &str,
) -> Value {
    match pid {
        Some(pid) if killable => {
            process::kill_tree(pid);
            registry::mark_killed(workspace, runner, agent);
            json!({ "killed": true, "pid": pid, "source": "disk" })
        }
        _ => json!({ "killed": false, "reason": "aucun process vivant", "status": status }),
    }
}

#[cfg(test)]
mod tests {
    use super::from_disk;
    use crate::process;
    use crate::registry;
    use crate::store::now_ms;
    use crate::tools::agent_control::args::ControlArgs;
    use litemcp::prelude::ToolResult;
    use serde_json::{Value, json};
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::{Duration, Instant};

    static SEQ: AtomicU32 = AtomicU32::new(0);

    /// Workspace temporaire unique (pid + rang), supprimé à la fin du test.
    struct TempWs(PathBuf);

    impl TempWs {
        fn new(tag: &str) -> Self {
            let n = SEQ.fetch_add(1, Ordering::Relaxed);
            let dir =
                std::env::temp_dir().join(format!("swarm-disque-{tag}-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            TempWs(dir)
        }
    }

    impl Drop for TempWs {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn args(action: &str, agent: Option<&str>) -> ControlArgs {
        ControlArgs {
            action: action.into(),
            agent_name: agent.map(String::from),
            run_id: None,
            runner: None,
            config: None,
            timeout_ms: None,
            since_timestamp: None,
        }
    }

    /// Dernier run de `agent` sur disque (et sa session), comme un serveur l'aurait écrit.
    fn dernier_run(ws: &Path, runner: &str, agent: &str, run: Value) {
        crate::state::update(ws, runner, agent, |doc| {
            doc["lastRun"] = run.clone();
            doc["session"] = json!({ "id": format!("s-{agent}"), "ts": now_ms() });
        })
        .unwrap();
    }

    /// Message d'un outil refusé.
    fn refusal(result: ToolResult) -> String {
        result.expect_err("refusé").message
    }

    /// Commande système inoffensive qui dort (jamais un CLI d'agent).
    fn dort() -> Command {
        let mut cmd = if cfg!(windows) {
            let script = "ping -n 60 127.0.0.1 >nul";
            let mut cmd = process::command(Path::new("cmd"));
            cmd.args(["/C", script]);
            cmd
        } else {
            let mut cmd = process::command(Path::new("sh"));
            cmd.args(["-c", "sleep 60"]);
            cmd
        };
        cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        cmd
    }

    #[test]
    fn sans_agent_le_run_est_introuvable_et_sans_run_l_agent_n_a_rien() {
        let ws = TempWs::new("introuvable");
        assert!(refusal(from_disk(&args("status", None), &ws.0)).contains("run introuvable: ?"));
        let mut avec_id = args("status", None);
        avec_id.run_id = Some("run-7".into());
        assert!(refusal(from_disk(&avec_id, &ws.0)).contains("run introuvable: run-7"));
        let rien = refusal(from_disk(&args("kill", Some("personne")), &ws.0));
        assert!(rien.contains("aucun run connu pour l'agent 'personne'"), "{rien}");
        // Un agent connu d'un runner seulement : rien sous l'autre.
        dernier_run(&ws.0, "claude", "alice", json!({ "pid": 4242, "status": "done", "ts": 1 }));
        let mut hermes = args("status", Some("alice"));
        hermes.runner = Some("hermes".into());
        assert!(
            refusal(from_disk(&hermes, &ws.0)).contains("aucun run connu pour l'agent 'alice'")
        );
    }

    #[test]
    fn un_run_termine_est_decrit_depuis_le_disque() {
        let ws = TempWs::new("termine");
        dernier_run(
            &ws.0,
            "claude",
            "bob",
            json!({ "pid": 4242, "status": "done", "exitCode": 0, "ts": 5 }),
        );
        let corps = from_disk(&args("status", Some("bob")), &ws.0).unwrap();
        assert_eq!(corps["source"], "disk");
        assert_eq!(corps["agentName"], "bob");
        assert_eq!(corps["pid"], 4242);
        assert_eq!(corps["status"], "done");
        assert_eq!(corps["alive"], false, "un PID mort n'est pas vivant");
        assert_eq!(corps["sessionId"], "s-bob", "la session vient du fichier d'état");
        assert!(corps["note"].as_str().unwrap().contains("pas de sortie en direct"), "{corps}");
        // `stream` et `wait` rendent la même description : sans run en mémoire, pas de sortie à suivre.
        assert_eq!(from_disk(&args("stream", Some("bob")), &ws.0).unwrap(), corps);
        assert_eq!(from_disk(&args("wait", Some("bob")), &ws.0).unwrap(), corps);
        // Sans statut écrit : « unknown », jamais un silence.
        dernier_run(&ws.0, "claude", "bob", json!({ "pid": 4242 }));
        assert_eq!(from_disk(&args("status", Some("bob")), &ws.0).unwrap()["status"], "unknown");
    }

    #[test]
    fn un_run_running_au_pid_mort_est_orphelin_et_pas_tuable() {
        let ws = TempWs::new("orphelin");
        dernier_run(
            &ws.0,
            "claude",
            "carl",
            json!({ "pid": u32::MAX, "status": "running", "ts": 1 }),
        );
        let corps = from_disk(&args("status", Some("carl")), &ws.0).unwrap();
        assert_eq!(corps["status"], "orphaned");
        assert_eq!(corps["alive"], false);
        let tue = from_disk(&args("kill", Some("carl")), &ws.0).unwrap();
        assert_eq!(tue["killed"], false);
        assert_eq!(tue["reason"], "aucun process vivant");
        assert_eq!(tue["status"], "orphaned");
        // Rien n'a été réécrit sur disque.
        let (_, entree) = registry::disk_entry(&ws.0, "carl", None).unwrap();
        assert_eq!(entree["status"], "running");
    }

    #[test]
    fn un_run_vivant_est_tue_puis_marque_sur_disque() {
        let ws = TempWs::new("tuer");
        let mut enfant = dort().spawn().unwrap();
        dernier_run(
            &ws.0,
            "claude",
            "dana",
            json!({ "pid": enfant.id(), "status": "running", "ts": 1 }),
        );
        let tue = from_disk(&args("kill", Some("dana")), &ws.0).unwrap();
        assert_eq!(tue["killed"], true);
        assert_eq!(tue["pid"], enfant.id());
        assert_eq!(tue["source"], "disk");
        // Le process est bien mort (attendu, pas laissé orphelin) et le dernier run est
        // marqué arrêté.
        let echeance = Instant::now() + Duration::from_secs(10);
        while enfant.try_wait().unwrap().is_none() && Instant::now() < echeance {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(enfant.try_wait().unwrap().is_some(), "le process dort encore");
        let (_, entree) = registry::disk_entry(&ws.0, "dana", None).unwrap();
        assert_eq!(entree["status"], "failed", "sur disque, tué se dit failed");
        assert!(entree["exitCode"].is_null());
        assert!(entree["lastOutputAt"].as_u64().is_some());
    }

    #[test]
    fn une_action_inconnue_est_refusee() {
        let ws = TempWs::new("action");
        dernier_run(&ws.0, "claude", "erin", json!({ "pid": 4242, "status": "done", "ts": 1 }));
        let err = refusal(from_disk(&args("hop", Some("erin")), &ws.0));
        assert!(err.contains("action inconnue 'hop'"), "{err}");
    }
}
