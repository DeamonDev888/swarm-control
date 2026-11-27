//! Un agent Claude garde sa session d'un appel à l'autre.

use crate::harness::start;
use crate::rpc::{call, lease_file, ok, session_of};
use serde_json::json;

#[test]
fn claude_agent_persists_its_session() {
    let h = start(&[]);
    let created = ok(
        &h,
        "create_agent",
        json!({"name": "alice", "prompt": "Tu es Alice.", "model": "claude-sonnet-4-6"}),
    );
    assert_eq!(created["runner"], "claude");
    // Profil dédié à l'agent (PLAN §4.4) : `<ws>/agents/alice/.claude/…`.
    let profile = h.ws.join("agents").join("alice").join(".claude");
    assert!(profile.join("agents/alice.md").is_file());
    assert!(profile.join("settings_alice.json").is_file());
    assert!(profile.join(".mcp.alice.json").is_file());

    // Doublon refusé.
    let (err, _) = call(&h, "create_agent", json!({"name": "alice", "prompt": "x"}));
    assert!(err);

    let first =
        ok(&h, "run_agent", json!({"runner": "claude", "agentName": "alice", "prompt": "bonjour"}));
    let text = first["result"].as_str().unwrap();
    assert!(text.contains("echo:bonjour"), "{text}");
    assert!(text.contains("resumed:none"), "{text}");
    assert!(text.contains("model:claude-sonnet-4-6"), "{text}");
    assert!(text.contains("agent:alice"), "{text}");
    let sid = first["sessionId"].as_str().unwrap().to_string();

    // La session est dans le fichier d'état de l'agent ; le bail est libéré à la fin du run.
    assert_eq!(session_of(&h.ws, "claude", "alice").as_deref(), Some(sid.as_str()));
    assert!(!lease_file(&h.ws, "claude", "alice").exists(), "bail non libéré");
    assert!(!h.ws.join("bridge/agents.json").exists(), "l'ancien fichier ne doit plus être écrit");

    // Deuxième appel : reprise automatique de la même session.
    let second =
        ok(&h, "run_agent", json!({"runner": "claude", "agentName": "alice", "prompt": "encore"}));
    assert!(second["result"].as_str().unwrap().contains(&format!("resumed:{sid}")));
    assert_eq!(second["sessionId"], sid.as_str());

    // autoResume:false → session neuve.
    let fresh = ok(
        &h,
        "run_agent",
        json!({"runner": "claude", "agentName": "alice", "prompt": "neuf!", "autoResume": false}),
    );
    assert!(fresh["result"].as_str().unwrap().contains("resumed:none"));
    assert_ne!(fresh["sessionId"], sid.as_str());

    // Les fichiers temporaires du run sont nettoyés.
    let leftovers = std::fs::read_dir(h.ws.join(".claude"))
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains("_run_"))
        .count();
    assert_eq!(leftovers, 0, "fichiers temporaires restants");
}
