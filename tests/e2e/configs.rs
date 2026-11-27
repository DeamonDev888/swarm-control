//! Lecture et mise à jour de la configuration d'un agent.

use crate::harness::start;
use crate::rpc::ok;
use serde_json::{Value, json};

#[test]
fn secrets_are_masked_in_get_agent_configs() {
    let h = start(&[]);
    ok(&h, "create_agent", json!({"name": "p1", "prompt": "a"}));
    let cfg = ok(&h, "get_agent_configs", json!({"name": "p1"}));
    let settings = cfg["files"]["settings.json"].as_str().unwrap();
    assert!(!settings.contains("primary-token"), "{settings}");
    assert_eq!(cfg["files"]["prompt.md"].as_str().unwrap(), "a");
}

#[test]
fn mcp_update_regenerates_the_agent_file() {
    let h = start(&[]);
    ok(&h, "create_agent", json!({"name": "p1", "prompt": "a"}));
    let up = ok(&h, "update_agent_config", json!({"name": "p1", "mcpServers": ["memory"]}));
    assert!(up["changes"].to_string().contains("memory"));
    let path = h.ws.join("agents").join("p1").join(".claude/.mcp.p1.json");
    let mcp: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert!(mcp["mcpServers"].get("memory").is_some());
    assert!(mcp["mcpServers"].get("db").is_none());
}
