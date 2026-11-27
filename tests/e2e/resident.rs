//! Un agent résident : serveur MCP propre, `ask` / `status`, annonce, session conservée.

use crate::harness::start;
use crate::rpc::{call_at, host_file, lease_file, ok, ok_at, port_of, rpc_at, session_of};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

/// Attend (au plus `secs`) qu'une condition devienne vraie.
pub fn eventually(secs: u64, mut cond: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        if cond() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    cond()
}

#[test]
fn a_resident_agent_serves_ask_and_status_and_announces_itself() {
    let h = start(&[]);
    ok(&h, "create_agent", json!({"name": "alice", "prompt": "Tu es Alice."}));
    let info = ok(&h, "agent_start", json!({"name": "alice"}));
    assert_eq!(info["name"], "swarm-alice");
    assert_eq!(info["runner"], "claude");
    let url = info["url"].as_str().unwrap().to_string();
    let port = port_of(&url);
    assert_ne!(port, h.port, "un serveur propre, sur son propre port");

    // Son serveur MCP expose exactement ask / status / stop.
    let listed = rpc_at(port, "tools/list", None, json!({}));
    let mut tools: Vec<String> = listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    tools.sort();
    assert_eq!(tools, ["ask", "status", "stop"]);

    // Annonce LiteMCP et bail d'hébergement (avec l'URL) sont publiés.
    assert!(eventually(5, || h.announce_file("swarm-alice").is_file()), "annonce absente");
    let lease: Value = serde_json::from_str(
        &std::fs::read_to_string(host_file(&h.ws, "claude", "alice")).unwrap(),
    )
    .unwrap();
    assert_eq!(lease["url"], url.as_str());

    // `ask` : la demande traverse la file, le faux CLI répond, la session est créée.
    let first = ok_at(port, "ask", json!({"prompt": "bonjour"}));
    let text = first["reply"].as_str().unwrap();
    assert!(text.contains("echo:bonjour") && text.contains("resumed:none"), "{text}");
    assert!(text.contains("agent:alice"), "{text}");
    assert_eq!(first["via"], json!(["alice"]));
    let sid = first["sessionId"].as_str().unwrap().to_string();

    // Deuxième demande : la même session est reprise — l'agent « se souvient ».
    let second = ok_at(port, "ask", json!({"prompt": "encore"}));
    assert!(second["reply"].as_str().unwrap().contains(&format!("resumed:{sid}")), "{second}");

    // `status` reflète l'activité.
    let status = ok_at(port, "status", json!({}));
    assert_eq!(status["state"], "idle");
    assert_eq!(status["turnsDone"], 2);
    assert_eq!(status["queueLength"], 0);
    assert_eq!(status["session"], sid.as_str());
    assert_eq!(status["lastTurn"]["ok"], true);
    assert_eq!(session_of(&h.ws, "claude", "alice").as_deref(), Some(sid.as_str()));

    // Les baux de run sont libérés entre les tours ; `list_agents` montre la résidence.
    assert!(!lease_file(&h.ws, "claude", "alice").exists());
    let listed = ok(&h, "list_agents", json!({}));
    let alice = listed["agents"].as_array().unwrap().iter().find(|a| a["name"] == "alice").unwrap();
    assert_eq!(alice["resident"]["url"], url.as_str());
}

#[test]
fn create_agent_with_resident_true_hosts_it_at_once() {
    let h = start(&[]);
    let created = ok(&h, "create_agent", json!({"name": "carl", "prompt": "p", "resident": true}));
    let url = created["resident"]["url"].as_str().expect("résidence démarrée").to_string();
    let reply = ok_at(port_of(&url), "ask", json!({"prompt": "salut"}));
    assert!(reply["reply"].as_str().unwrap().contains("echo:salut"));
    // Un agent Hermes peut être résident de la même façon.
    let hermes = ok(
        &h,
        "create_agent",
        json!({"name": "hugo", "runner": "hermes", "prompt": "p", "resident": true}),
    );
    assert!(hermes["resident"]["url"].is_string(), "{hermes}");
}

#[test]
fn a_cycle_in_the_via_chain_is_refused_and_the_chain_is_extended() {
    let h = start(&[]);
    ok(&h, "create_agent", json!({"name": "alice", "prompt": "p", "resident": true}));
    let port =
        port_of(ok(&h, "list_agents", json!({}))["agents"][0]["resident"]["url"].as_str().unwrap());

    let (err, msg) =
        call_at(port, "ask", json!({"prompt": "x", "from": "bob", "via": ["bob", "alice"]}));
    assert!(err && msg.to_string().contains("CYCLE"), "{msg}");

    let ok_reply = ok_at(port, "ask", json!({"prompt": "x", "from": "bob", "via": ["bob"]}));
    assert_eq!(ok_reply["via"], json!(["bob", "alice"]));
}
