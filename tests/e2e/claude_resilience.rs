//! Session périmée, timeout et jeton de secours.

use crate::harness::start;
use crate::rpc::{call, lease_file, ok, session_of};
use serde_json::json;
use std::time::{Duration, Instant};

#[test]
fn dead_session_is_dropped_and_retried_fresh() {
    let h = start(&[]);
    ok(&h, "create_agent", json!({"name": "bob", "prompt": "p"}));
    let first =
        ok(&h, "run_agent", json!({"runner": "claude", "agentName": "bob", "prompt": "un"}));
    let old = first["sessionId"].as_str().unwrap().to_string();
    // La session reprise est « périmée » : le serveur doit la jeter et relancer à neuf.
    let again = ok(
        &h,
        "run_agent",
        json!({"runner": "claude", "agentName": "bob", "prompt": "DEADSESSION"}),
    );
    assert!(again["result"].as_str().unwrap().contains("resumed:none"));
    assert_ne!(again["sessionId"], old.as_str());
    assert_eq!(
        session_of(&h.ws, "claude", "bob").as_deref(),
        again["sessionId"].as_str(),
        "la nouvelle session doit remplacer l'ancienne"
    );
}

#[test]
fn timeout_stops_the_process_and_frees_the_agent() {
    let h = start(&[("AGENT_TIMEOUT_MS", "1500")]);
    ok(&h, "create_agent", json!({"name": "dora", "prompt": "p"}));
    let started = Instant::now();
    let (err, msg) =
        call(&h, "run_agent", json!({"runner": "claude", "agentName": "dora", "prompt": "SLEEP"}));
    assert!(err && msg.to_string().contains("TIMEOUT"), "{msg}");
    assert!(started.elapsed() < Duration::from_secs(20), "le timeout n'a pas coupé le process");
    // L'échec libère le bail : l'agent est relançable tout de suite.
    assert!(!lease_file(&h.ws, "claude", "dora").exists(), "bail non libéré après timeout");
    ok(&h, "run_agent", json!({"runner": "claude", "agentName": "dora", "prompt": "après"}));
}

#[test]
fn fallback_token_is_used_on_401() {
    let h = start(&[]);
    ok(&h, "create_agent", json!({"name": "eve", "prompt": "p"}));
    ok(
        &h,
        "update_agent_config",
        json!({"name": "eve", "env": {"AUTH_FALLBACK_1": "backup-token"}}),
    );
    let r =
        ok(&h, "run_agent", json!({"runner": "claude", "agentName": "eve", "prompt": "FAIL401"}));
    assert!(r["result"].as_str().unwrap().contains("token:backup-token"), "{r}");
}
