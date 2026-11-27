//! Cycle de vie, sessions, résilience et mode résident d'un agent Antigravity (`agy`).

use crate::harness::start;
use crate::rpc::{call, lease_file, ok, session_of};
use serde_json::json;

#[test]
fn antigravity_agent_lifecycle_and_sessions() {
    let h = start(&[]);
    let created = ok(
        &h,
        "create_agent",
        json!({
            "name": "nova",
            "runner": "antigravity",
            "prompt": "Tu es Nova.",
            "model": "gemini-pro"
        }),
    );
    assert_eq!(created["runner"], "antigravity");
    assert!(h.ws.join(".antigravity/agents/nova.md").is_file());
    assert!(h.ws.join(".antigravity/settings_nova.json").is_file());

    // Doublon refusé.
    let (err, _) =
        call(&h, "create_agent", json!({"name": "nova", "runner": "antigravity", "prompt": "x"}));
    assert!(err);

    // Présent dans list_agents (avec details:true pour le modèle).
    let listed = ok(&h, "list_agents", json!({"details": true}));
    let agents = listed["agents"].as_array().unwrap();
    let nova = agents.iter().find(|a| a["name"] == "nova").expect("nova listé");
    assert_eq!(nova["runner"], "antigravity");
    assert_eq!(nova["model"], "gemini-pro");

    // Premier run : persona injectée, conversation_id sauvegardé.
    let first = ok(
        &h,
        "run_agent",
        json!({"runner": "antigravity", "agentName": "nova", "prompt": "bonjour"}),
    );
    let text = first["result"].as_str().unwrap();
    assert!(text.contains("Tu es Nova."), "{text}");
    assert!(text.contains("bonjour"), "{text}");
    assert!(text.contains("resumed:none"), "{text}");
    assert!(text.contains("model:gemini-pro"), "{text}");
    assert!(text.contains("agent:nova"), "{text}");
    let cid = first["sessionId"].as_str().unwrap().to_string();
    assert_eq!(session_of(&h.ws, "antigravity", "nova").as_deref(), Some(cid.as_str()));
    assert!(!lease_file(&h.ws, "antigravity", "nova").exists());

    // Deuxième run : reprise via --conversation sans réinjecter la persona.
    let second = ok(
        &h,
        "run_agent",
        json!({"runner": "antigravity", "agentName": "nova", "prompt": "encore"}),
    );
    let text2 = second["result"].as_str().unwrap();
    assert!(text2.contains(&format!("resumed:{cid}")), "{text2}");
    assert!(!text2.contains("Tu es Nova."), "{text2}");
    assert_eq!(second["sessionId"], cid.as_str());

    // autoResume:false → nouvelle conversation avec persona.
    let fresh = ok(
        &h,
        "run_agent",
        json!({"runner": "antigravity", "agentName": "nova", "prompt": "neuf!", "autoResume": false}),
    );
    assert!(fresh["result"].as_str().unwrap().contains("resumed:none"));
    assert!(fresh["result"].as_str().unwrap().contains("Tu es Nova."));
    assert_ne!(fresh["sessionId"], cid.as_str());

    // update_agent_config + get_agent_configs (secrets masqués).
    ok(
        &h,
        "update_agent_config",
        json!({
            "name": "nova",
            "model": "gemini-flash",
            "env": {"GEMINI_API_KEY": "super-secret-token-123456"}
        }),
    );
    let cfg = ok(&h, "get_agent_configs", json!({"name": "nova"}));
    let settings_str = cfg["files"]["settings.json"].as_str().unwrap();
    assert!(settings_str.contains("gemini-flash"), "{settings_str}");
    assert!(!settings_str.contains("super-secret-token-123456"), "secret non masqué");

    // Suppression propre.
    ok(&h, "delete_agent", json!({"name": "nova"}));
    assert!(!h.ws.join(".antigravity/agents/nova.md").exists());
    assert!(!h.ws.join(".antigravity/settings_nova.json").exists());
}

#[test]
fn antigravity_dead_conversation_retries_once() {
    let h = start(&[]);
    ok(
        &h,
        "create_agent",
        json!({"name": "phoenix", "runner": "antigravity", "prompt": "Tu es Phoenix."}),
    );
    let first = ok(
        &h,
        "run_agent",
        json!({"runner": "antigravity", "agentName": "phoenix", "prompt": "tour1"}),
    );
    let old_cid = first["sessionId"].as_str().unwrap().to_string();

    // DEADSESSION échoue avec "Conversation not found" sur reprise → relance à neuf.
    let retry = ok(
        &h,
        "run_agent",
        json!({"runner": "antigravity", "agentName": "phoenix", "prompt": "DEADSESSION"}),
    );
    assert!(retry["error"].is_null(), "{retry}");
    assert!(retry["result"].as_str().unwrap().contains("resumed:none"));
    assert_ne!(retry["sessionId"].as_str().unwrap(), old_cid);
}

#[test]
fn antigravity_error_envelope_is_reported() {
    let h = start(&[]);
    ok(
        &h,
        "create_agent",
        json!({"name": "errbot", "runner": "antigravity", "prompt": "Tu es ErrBot."}),
    );
    let (err, msg) = call(
        &h,
        "run_agent",
        json!({"runner": "antigravity", "agentName": "errbot", "prompt": "AGYFAIL"}),
    );
    let msg = msg.to_string();
    assert!(err && msg.contains("ANTIGRAVITY_ERROR") && msg.contains("Unknown model"), "{msg}");
}
