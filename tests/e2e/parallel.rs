//! Plusieurs agents en parallèle.

use crate::harness::start;
use crate::rpc::ok;
use serde_json::json;

#[test]
fn parallel_agents_report_each_task() {
    let h = start(&[]);
    ok(&h, "create_agent", json!({"name": "p1", "prompt": "a"}));
    ok(&h, "create_agent", json!({"name": "p2", "prompt": "b"}));
    let r = ok(
        &h,
        "run_agents_parallel",
        json!({"agents": [
            {"runner": "claude", "agentName": "p1", "prompt": "un", "taskId": "t1"},
            {"runner": "claude", "agentName": "p2", "prompt": "deux"},
            {"runner": "claude", "agentName": "ghost", "prompt": "trois"}
        ]}),
    );
    assert_eq!(r["total"], 3);
    assert_eq!(r["failed"], 1);
    let results = r["results"].as_array().unwrap();
    assert_eq!(results[0]["taskId"], "t1");
    assert!(results[0]["result"].as_str().unwrap().contains("echo:un"));
    assert!(results[1]["result"].as_str().unwrap().contains("echo:deux"));
    assert!(results[2]["error"].as_str().unwrap().contains("INVALID_AGENT"));
}
