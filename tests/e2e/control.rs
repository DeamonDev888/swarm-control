//! Runs en arrière-plan : occupation, suivi, kill.

use crate::harness::start;
use crate::rpc::{call, last_run, lease_file, ok};
use serde_json::{Value, json};
use std::time::Duration;

#[test]
fn background_run_can_be_listed_and_killed() {
    let h = start(&[]);
    ok(&h, "create_agent", json!({"name": "carl", "prompt": "p"}));
    let started = ok(
        &h,
        "run_agent",
        json!({"runner": "claude", "agentName": "carl", "prompt": "SLEEP", "background": true}),
    );
    let run_id = started["runId"].as_str().unwrap().to_string();
    assert_eq!(started["status"], "running");
    // Le run détient le bail de son agent.
    assert!(lease_file(&h.ws, "claude", "carl").is_file());

    // Le même agent ne peut pas lancer un second run en parallèle.
    let (err, msg) =
        call(&h, "run_agent", json!({"runner": "claude", "agentName": "carl", "prompt": "x"}));
    assert!(err && msg.to_string().contains("AGENT_BUSY"), "{msg}");

    // Le PID apparaît, puis l'état de l'agent l'enregistre sur disque.
    let mut status = Value::Null;
    for _ in 0..50 {
        status = ok(&h, "agent_control", json!({"action": "status", "agentName": "carl"}));
        if status["pid"].is_number() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(status["status"], "running");
    assert!(status["pid"].is_number());
    assert_eq!(last_run(&h.ws, "claude", "carl")["status"], "running");
    assert_eq!(ok(&h, "agent_control", json!({"action": "list"}))["count"], 1);

    let killed = ok(&h, "agent_control", json!({"action": "kill", "runId": run_id}));
    assert_eq!(killed["killed"], true);
    assert_eq!(killed["run"]["status"], "killed");
    // Le run tué écrit son état et libère son bail en se terminant, juste après `kill`.
    assert!(
        crate::resident::eventually(5, || last_run(&h.ws, "claude", "carl")["status"] == "failed"),
        "état disque : {}",
        last_run(&h.ws, "claude", "carl")
    );
    assert!(
        crate::resident::eventually(5, || !lease_file(&h.ws, "claude", "carl").exists()),
        "bail non libéré après kill"
    );

    // Agent libre de nouveau.
    ok(&h, "run_agent", json!({"runner": "claude", "agentName": "carl", "prompt": "après"}));
}
