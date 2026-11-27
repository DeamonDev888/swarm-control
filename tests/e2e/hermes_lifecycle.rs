//! Cycle complet d'un agent Hermes : création, runs, reprise, mise à jour, suppression.

use crate::harness::start;
use crate::rpc::{ok, session_of, state_file};
use serde_json::json;

#[test]
fn hermes_agent_lifecycle() {
    let h = start(&[]);
    let created = ok(
        &h,
        "create_agent",
        json!({"name": "hera", "runner": "hermes", "prompt": "Tu es Hera.", "model": "glm-5.3-flash"}),
    );
    assert_eq!(created["runner"], "hermes");
    let profile = h.hermes_home.join("profiles/hera");
    assert_eq!(std::fs::read_to_string(profile.join("SOUL.md")).unwrap(), "Tu es Hera.");
    // Les serveurs du .mcp.json sont écrits dans config.yaml.
    let yaml = std::fs::read_to_string(profile.join("config.yaml")).unwrap();
    assert!(yaml.contains("mcp_servers:") && yaml.contains("memory:"), "{yaml}");
    assert!(yaml.contains("url: http://127.0.0.1:3099/mcp"), "{yaml}");

    let first = ok(
        &h,
        "run_agent",
        json!({"runner": "hermes", "agentName": "hera", "prompt": "salut \"quoted\" $VAR"}),
    );
    let text = first["result"].as_str().unwrap();
    assert!(text.contains("echo:salut \"quoted\" $VAR"), "{text}");
    assert!(text.contains("agent:hera"));
    let sid = first["sessionId"].as_str().unwrap().to_string();
    assert_eq!(session_of(&h.ws, "hermes", "hera").as_deref(), Some(sid.as_str()));

    let second =
        ok(&h, "run_agent", json!({"runner": "hermes", "agentName": "hera", "prompt": "suite"}));
    assert!(second["result"].as_str().unwrap().contains(&format!("resumed:{sid}")));

    // update → SOUL.md et listing.
    ok(&h, "update_agent_config", json!({"name": "hera", "prompt": "Nouveau prompt"}));
    assert_eq!(std::fs::read_to_string(profile.join("SOUL.md")).unwrap(), "Nouveau prompt");
    let listed = ok(&h, "list_agents", json!({"details": true}));
    let hera = listed["agents"].as_array().unwrap().iter().find(|a| a["name"] == "hera").unwrap();
    assert_eq!(hera["runner"], "hermes");
    assert_eq!(hera["model"], "glm-5.3-flash");
    assert_eq!(hera["hasSession"], true);

    // delete → profil, session et état disparaissent.
    ok(&h, "delete_agent", json!({"name": "hera"}));
    assert!(!profile.exists());
    assert_eq!(session_of(&h.ws, "hermes", "hera"), None);
    assert!(!state_file(&h.ws, "hermes", "hera").exists(), "état non supprimé");
}
