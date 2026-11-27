//! La file d'un agent résident : ordre, arrêt d'un tour, attente d'un run à la demande.

use crate::harness::start;
use crate::resident::eventually;
use crate::rpc::{call_at, ok, ok_at, port_of};
use serde_json::{Value, json};
use std::sync::mpsc;
use std::time::Duration;

/// Démarre un serveur, crée l'agent `alice` résident, rend (serveur, port de l'agent).
fn resident_alice() -> (crate::harness::Harness, u16) {
    let h = start(&[]);
    let created = ok(&h, "create_agent", json!({"name": "alice", "prompt": "p", "resident": true}));
    let port = port_of(created["resident"]["url"].as_str().unwrap());
    (h, port)
}

fn state_of(port: u16) -> Value {
    ok_at(port, "status", json!({}))
}

#[test]
fn asks_are_queued_and_served_in_order_on_the_same_session() {
    let (_h, port) = resident_alice();
    let first = std::thread::spawn(move || ok_at(port, "ask", json!({"prompt": "SLOW un"})));
    assert!(eventually(10, || state_of(port)["state"] == "busy"), "le 1er tour n'a pas démarré");
    let second = std::thread::spawn(move || ok_at(port, "ask", json!({"prompt": "deux"})));
    // Pendant que le 1er tourne, le 2e attend dans la file.
    assert!(eventually(5, || state_of(port)["queueLength"] == 1), "le 2e tour n'est pas en file");

    let (first, second) = (first.join().unwrap(), second.join().unwrap());
    assert!(first["queuedMs"].as_u64().unwrap() < 700, "{first}");
    assert!(second["queuedMs"].as_u64().unwrap() >= 700, "le 2e n'a pas attendu: {second}");
    // Le 2e tour reprend la session créée par le 1er : un seul fil de conversation.
    let sid = first["sessionId"].as_str().unwrap();
    assert!(second["reply"].as_str().unwrap().contains(&format!("resumed:{sid}")), "{second}");
    assert_eq!(state_of(port)["turnsDone"], 2);
}

#[test]
fn stop_kills_the_current_turn_and_the_agent_keeps_serving() {
    let (_h, port) = resident_alice();
    let asker = std::thread::spawn(move || call_at(port, "ask", json!({"prompt": "SLEEP"})));
    assert!(eventually(10, || state_of(port)["state"] == "busy"), "le tour n'a pas démarré");

    let stopped = ok_at(port, "stop", json!({}));
    assert_eq!(stopped["stopped"], true);
    let (is_error, message) = asker.join().unwrap();
    assert!(is_error && message.to_string().contains("KILLED"), "{message}");

    // L'agent reste résident : une nouvelle demande aboutit.
    assert!(eventually(5, || state_of(port)["state"] == "idle"));
    let after = ok_at(port, "ask", json!({"prompt": "après"}));
    assert!(after["reply"].as_str().unwrap().contains("echo:après"));
    assert_eq!(state_of(port)["lastTurn"]["ok"], true);
}

#[test]
fn stop_with_clear_queue_cancels_the_waiting_asks() {
    let (_h, port) = resident_alice();
    let running = std::thread::spawn(move || call_at(port, "ask", json!({"prompt": "SLEEP"})));
    assert!(eventually(10, || state_of(port)["state"] == "busy"));
    let waiting = std::thread::spawn(move || call_at(port, "ask", json!({"prompt": "attend"})));
    assert!(eventually(5, || state_of(port)["queueLength"] == 1));

    let stopped = ok_at(port, "stop", json!({"clearQueue": true}));
    assert_eq!((stopped["stopped"].clone(), stopped["cleared"].clone()), (json!(true), json!(1)));
    let (waiting_err, waiting_msg) = waiting.join().unwrap();
    assert!(waiting_err && waiting_msg.to_string().contains("CANCELLED"), "{waiting_msg}");
    assert!(running.join().unwrap().0, "le tour tué rend une erreur");
}

#[test]
fn ask_waits_for_an_on_demand_run_instead_of_failing() {
    let (h, port) = resident_alice();
    // Un run « à la demande » tient le bail de l'agent (ici lancé par l'administration).
    ok(
        &h,
        "run_agent",
        json!({"runner": "claude", "agentName": "alice", "prompt": "SLEEP", "background": true}),
    );
    assert!(eventually(10, || {
        let status = ok(&h, "agent_control", json!({"action": "status", "agentName": "alice"}));
        status["pid"].is_number()
    }));

    let (tx, done) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(call_at(port, "ask", json!({"prompt": "attends ton tour"})));
    });
    // Tant que le run à la demande tient le bail, la demande patiente (sans échouer).
    assert!(done.recv_timeout(Duration::from_secs(1)).is_err(), "la demande aurait dû attendre");

    ok(&h, "agent_control", json!({"action": "kill", "agentName": "alice"}));
    let (is_error, reply) = done.recv_timeout(Duration::from_secs(20)).expect("la demande reprend");
    assert!(!is_error, "{reply}");
    assert!(reply["reply"].as_str().unwrap().contains("echo:attends ton tour"));
}

#[test]
fn an_ask_that_times_out_is_cancelled_before_it_starts() {
    let (_h, port) = resident_alice();
    let running = std::thread::spawn(move || call_at(port, "ask", json!({"prompt": "SLOW long"})));
    assert!(eventually(10, || state_of(port)["state"] == "busy"));
    // Le 2e appelant n'attend que 1 s (minimum accepté) alors que le tour dure 1,5 s.
    let (is_error, message) =
        call_at(port, "ask", json!({"prompt": "trop tard", "timeoutMs": 1000}));
    assert!(is_error && message.to_string().contains("pas de réponse"), "{message}");
    running.join().unwrap();
    // Annulé avant son démarrage : il n'a jamais été exécuté (un seul tour compté).
    assert!(eventually(5, || state_of(port)["queueLength"] == 0));
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(state_of(port)["turnsDone"], 1, "la demande annulée ne doit pas s'exécuter");
}
