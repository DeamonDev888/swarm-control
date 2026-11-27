//! Plusieurs instances sur le même workspace : un agent n'a qu'un seul exécutant à la fois.

use crate::harness::{start, start_sharing};
use crate::rpc::{call, last_run, lease_file, ok, session_of, state_file};
use serde_json::{Value, json};
use std::time::Duration;

/// Attend que le run en arrière-plan de `agent` ait un PID (le CLI est lancé).
fn wait_for_pid(h: &crate::harness::Harness, agent: &str) {
    for _ in 0..80 {
        let status = ok(h, "agent_control", json!({"action": "status", "agentName": agent}));
        if status["pid"].is_number() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("le run de {agent} n'a jamais démarré");
}

#[test]
fn another_instance_cannot_run_an_agent_that_is_running() {
    let a = start(&[]);
    let b = start_sharing(&a, &[]);
    ok(&a, "create_agent", json!({"name": "alice", "prompt": "p"}));

    ok(
        &a,
        "run_agent",
        json!({"runner": "claude", "agentName": "alice", "prompt": "SLEEP", "background": true}),
    );
    wait_for_pid(&a, "alice");

    // L'instance B (autre process) voit le bail de A et refuse.
    let (err, msg) =
        call(&b, "run_agent", json!({"runner": "claude", "agentName": "alice", "prompt": "x"}));
    let msg = msg.to_string();
    assert!(err && msg.contains("AGENT_BUSY") && msg.contains("autre instance"), "{msg}");

    // B ne peut pas non plus supprimer un agent qui tourne ailleurs.
    let (err, msg) = call(&b, "delete_agent", json!({"name": "alice"}));
    assert!(err && msg.to_string().contains("run en cours"), "{msg}");

    // B retrouve le run de A depuis le disque (il n'est pas dans sa mémoire).
    let status = ok(&b, "agent_control", json!({"action": "status", "agentName": "alice"}));
    assert_eq!(status["source"], "disk", "{status}");
    assert_eq!(status["status"], "running");
    assert_eq!(status["alive"], true);

    // Une fois le run de A arrêté, le bail est libéré et B peut lancer l'agent.
    ok(&a, "agent_control", json!({"action": "kill", "agentName": "alice"}));
    assert!(!lease_file(&a.ws, "claude", "alice").exists());
    ok(&b, "run_agent", json!({"runner": "claude", "agentName": "alice", "prompt": "après"}));
}

#[test]
fn two_instances_share_the_agents_session() {
    let a = start(&[]);
    let b = start_sharing(&a, &[]);
    ok(&a, "create_agent", json!({"name": "alice", "prompt": "p"}));

    let first =
        ok(&a, "run_agent", json!({"runner": "claude", "agentName": "alice", "prompt": "un"}));
    let sid = first["sessionId"].as_str().unwrap().to_string();

    // L'instance B reprend la session créée par A.
    let second =
        ok(&b, "run_agent", json!({"runner": "claude", "agentName": "alice", "prompt": "deux"}));
    assert!(second["result"].as_str().unwrap().contains(&format!("resumed:{sid}")), "{second}");
    assert_eq!(session_of(&a.ws, "claude", "alice").as_deref(), Some(sid.as_str()));
}

#[test]
fn different_agents_run_in_parallel_across_instances() {
    let a = start(&[]);
    let b = start_sharing(&a, &[]);
    ok(&a, "create_agent", json!({"name": "alice", "prompt": "p"}));
    ok(&a, "create_agent", json!({"name": "bob", "prompt": "p"}));

    ok(
        &a,
        "run_agent",
        json!({"runner": "claude", "agentName": "alice", "prompt": "SLEEP", "background": true}),
    );
    wait_for_pid(&a, "alice");
    // Un autre agent n'est pas bloqué par le bail d'alice.
    ok(&b, "run_agent", json!({"runner": "claude", "agentName": "bob", "prompt": "libre"}));
    // Chaque agent a son propre fichier d'état : pas de réécriture croisée.
    assert!(state_file(&a.ws, "claude", "alice").is_file());
    assert!(state_file(&a.ws, "claude", "bob").is_file());
    assert_eq!(last_run(&a.ws, "claude", "alice")["status"], "running");
    assert_eq!(last_run(&a.ws, "claude", "bob")["status"], "done");
}

#[test]
fn legacy_agents_json_is_still_read() {
    let h = start(&[]);
    ok(&h, "create_agent", json!({"name": "alice", "prompt": "p"}));
    // Session créée par l'ancien OverMind : seul `bridge/agents.json` la connaît.
    let now =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()
            as u64;
    std::fs::create_dir_all(h.ws.join("bridge")).unwrap();
    std::fs::write(
        h.ws.join("bridge/agents.json"),
        json!({"claude:alice": {"id": "legacy-1", "ts": now}}).to_string(),
    )
    .unwrap();

    let r =
        ok(&h, "run_agent", json!({"runner": "claude", "agentName": "alice", "prompt": "salut"}));
    assert!(r["result"].as_str().unwrap().contains("resumed:legacy-1"), "{r}");
    // La session est désormais dans l'état de l'agent ; l'ancien fichier n'est pas modifié.
    assert_eq!(session_of(&h.ws, "claude", "alice").as_deref(), Some("legacy-1"));
    let legacy: Value =
        serde_json::from_str(&std::fs::read_to_string(h.ws.join("bridge/agents.json")).unwrap())
            .unwrap();
    assert_eq!(legacy["claude:alice"]["id"], "legacy-1");
}

#[test]
fn a_dead_processs_lease_does_not_block_the_agent() {
    let h = start(&[]);
    ok(&h, "create_agent", json!({"name": "alice", "prompt": "p"}));
    // Bail laissé par un process mort (PID impossible) : doit être repris.
    let owners = h.ws.join("bridge").join("owners");
    std::fs::create_dir_all(&owners).unwrap();
    let now =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()
            as u64;
    std::fs::write(
        lease_file(&h.ws, "claude", "alice"),
        json!({"instance": "mort", "pid": 4_000_000_000u64, "run_id": "r0", "ts": now}).to_string(),
    )
    .unwrap();

    ok(&h, "run_agent", json!({"runner": "claude", "agentName": "alice", "prompt": "reprise"}));
    assert!(!lease_file(&h.ws, "claude", "alice").exists(), "bail repris puis libéré");
}
