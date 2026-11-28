use crate::harness::start;
use crate::rpc::rpc;
use serde_json::json;

#[test]
fn exposes_the_eleven_tools() {
    let h = start(&[]);
    let resp = rpc(&h, "tools/list", None, json!({}));
    let mut names: Vec<String> = resp["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "agent_control",
            "agent_start",
            "agent_stop",
            "create_agent",
            "delete_agent",
            "get_agent_configs",
            "list_agents",
            "list_templates",
            "run_agent",
            "run_agents_parallel",
            "update_agent_config"
        ]
    );
}
