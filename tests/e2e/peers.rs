//! Pairs automatiques (PLAN §4.6) : les agents résidents **vivants** du registre
//! entrent dans la config MCP des autres, chacun dans son format (Claude
//! `--mcp-config`, Hermes `mcp_servers`, Antigravity `.agents/mcp_config.json`) ;
//! les serveurs de l'utilisateur restent, et un pair arrêté en sort.

use crate::harness::{Harness, start};
use crate::resident::eventually;
use crate::rpc::ok;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[test]
fn live_peers_enter_each_agent_mcp_config_and_a_stopped_peer_leaves() {
    let h = start(&[]);

    // Le pair vivant : bob résident (vrai process, annoncé dans le registre isolé).
    ok(&h, "create_agent", json!({"name": "bob", "prompt": "Tu es Bob."}));
    ok(&h, "agent_start", json!({"name": "bob"}));
    assert!(eventually(5, || h.announce_file("swarm-bob").is_file()), "annonce de bob absente");

    // Claude : les serveurs persistants du `.mcp.json` restent, bob s'ajoute.
    ok(&h, "create_agent", json!({"name": "alice", "prompt": "Tu es Alice."}));
    ok(&h, "run_agent", json!({"runner": "claude", "agentName": "alice", "prompt": "bonjour"}));
    let mcp = read_json(&mcp_path(&h, "alice"));
    for name in ["memory", "db", "bob"] {
        assert!(mcp["mcpServers"].get(name).is_some(), "serveur {name} manquant : {mcp}");
    }

    // Hermes : bloc ajouté à la section `mcp_servers:` du profil, sans écraser les autres.
    ok(
        &h,
        "create_agent",
        json!({"name": "hera", "runner": "hermes", "prompt": "Tu es Hera.", "model": "glm-5.3-flash"}),
    );
    ok(&h, "run_agent", json!({"runner": "hermes", "agentName": "hera", "prompt": "bonjour"}));
    let yaml = std::fs::read_to_string(h.hermes_home.join("profiles/hera/config.yaml")).unwrap();
    assert!(yaml.contains("bob:"), "{yaml}");
    assert!(yaml.contains("memory:"), "serveurs de l'utilisateur conservés : {yaml}");

    // Antigravity : `.agents/mcp_config.json` du workspace (partagé par les agents).
    ok(
        &h,
        "create_agent",
        json!({"name": "nova", "runner": "antigravity", "prompt": "Tu es Nova.", "model": "gemini-pro"}),
    );
    ok(&h, "run_agent", json!({"runner": "antigravity", "agentName": "nova", "prompt": "bonjour"}));
    let agy = read_json(&h.ws.join(".agents").join("mcp_config.json"));
    assert!(agy["mcpServers"].get("bob").is_some(), "{agy}");

    // Bob s'arrête (annonce retirée) : au tour suivant, il sort des configs.
    ok(&h, "agent_stop", json!({"name": "bob"}));
    let mut gone = false;
    for attempt in 0..6 {
        // Une annonce résiduelle + le cache des PID (1 s) : on laisse le temps passer.
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(700));
        }
        ok(&h, "run_agent", json!({"runner": "claude", "agentName": "alice", "prompt": "suite"}));
        let mcp = read_json(&mcp_path(&h, "alice"));
        if mcp["mcpServers"].get("bob").is_none() {
            assert!(mcp["mcpServers"].get("memory").is_some(), "l'utilisateur a disparu : {mcp}");
            gone = true;
            break;
        }
    }
    assert!(gone, "bob est encore injecté après son arrêt");
}

/// `.mcp.<agent>.json` du profil dédié d'un agent Claude.
fn mcp_path(h: &Harness, agent: &str) -> PathBuf {
    h.ws.join("agents").join(agent).join(".claude").join(format!(".mcp.{agent}.json"))
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}
