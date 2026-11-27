//! Refus d'entrées invalides.

use crate::harness::start;
use crate::rpc::{call, rpc};
use serde_json::json;

#[test]
fn run_validation_errors() {
    let h = start(&[]);
    // Agent inexistant.
    let (err, msg) =
        call(&h, "run_agent", json!({"runner": "claude", "agentName": "ghost", "prompt": "x"}));
    assert!(err && msg.to_string().contains("INVALID_AGENT"), "{msg}");
    // Nom piégé : refusé avant tout accès disque.
    let (err, msg) =
        call(&h, "run_agent", json!({"runner": "claude", "agentName": "../etc", "prompt": "x"}));
    assert!(err, "{msg}");
    // Schéma : runner inconnu → erreur JSON-RPC -32602.
    let resp = rpc(
        &h,
        "tools/call",
        Some("run_agent"),
        json!({"name": "run_agent", "arguments": {"runner": "gemini", "prompt": "x"}}),
    );
    assert_eq!(resp["error"]["code"], -32602, "{resp}");
}
