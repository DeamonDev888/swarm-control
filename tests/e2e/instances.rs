//! Instances numérotées et routage d'un run vers l'hôte d'un agent résident.

use crate::harness::{command, start, start_sharing, start_sharing_with_args};
use crate::resident::eventually;
use crate::rpc::{ok, ok_at, port_of};
use serde_json::json;

#[test]
fn instances_get_distinct_numbers_and_announce_themselves() {
    let a = start(&[]);
    let b = start_sharing(&a, &[]);
    let c = start_sharing_with_args(&a, &[], &["--instance", "7"]);
    for name in ["swarm-control-1", "swarm-control-2", "swarm-control-7"] {
        assert!(eventually(5, || a.announce_file(name).is_file()), "{name} non annoncée");
    }
    // Chaque instance se présente sous son numéro, comme hôte de ses agents.
    ok(&a, "create_agent", json!({"name": "alice", "prompt": "p"}));
    ok(&b, "create_agent", json!({"name": "bob", "prompt": "p"}));
    ok(&c, "create_agent", json!({"name": "carol", "prompt": "p"}));
    assert_eq!(ok(&a, "agent_start", json!({"name": "alice"}))["instance"], "swarm-control-1");
    assert_eq!(ok(&b, "agent_start", json!({"name": "bob"}))["instance"], "swarm-control-2");
    assert_eq!(ok(&c, "agent_start", json!({"name": "carol"}))["instance"], "swarm-control-7");
}

#[test]
fn an_explicit_instance_number_already_taken_is_refused() {
    let a = start(&[]);
    // Même TEMP que `a` (donc mêmes réservations de numéros) : l'instance 1 est prise.
    let root = a.ws.parent().unwrap();
    let output =
        command(root, &a.ws, &a.hermes_home).args(["--instance", "1"]).output().expect("lancement");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--instance 1") && stderr.contains("déjà prise"), "{stderr}");
}

#[test]
fn run_agent_is_routed_to_the_instance_hosting_the_agent() {
    let a = start(&[]);
    let b = start_sharing(&a, &[]);
    ok(&a, "create_agent", json!({"name": "alice", "prompt": "p"}));
    ok(&a, "agent_start", json!({"name": "alice"}));

    // B reçoit la demande ; A, l'hôte, l'exécute : la file et la session sont les siennes.
    let first =
        ok(&b, "run_agent", json!({"runner": "claude", "agentName": "alice", "prompt": "un"}));
    assert_eq!(first["routedTo"]["instance"], "swarm-control-1", "{first}");
    assert!(first["result"].as_str().unwrap().contains("echo:un|resumed:none"), "{first}");
    let sid = first["sessionId"].as_str().expect("session").to_string();

    // Depuis A même, la session est reprise : un seul fil de conversation pour tous.
    let second =
        ok(&a, "run_agent", json!({"runner": "claude", "agentName": "alice", "prompt": "deux"}));
    assert_eq!(second["routedTo"]["instance"], "swarm-control-1", "{second}");
    assert!(second["result"].as_str().unwrap().contains(&format!("resumed:{sid}")), "{second}");
}

#[test]
fn runs_that_cannot_be_forwarded_or_have_no_host_stay_local() {
    let a = start(&[]);
    let b = start_sharing(&a, &[]);
    ok(&a, "create_agent", json!({"name": "alice", "prompt": "p"}));
    ok(&a, "create_agent", json!({"name": "bob", "prompt": "p"}));
    ok(&a, "agent_start", json!({"name": "alice"}));

    // Sans hôte : à la demande, comme avant.
    let bob = ok(&b, "run_agent", json!({"runner": "claude", "agentName": "bob", "prompt": "x"}));
    assert!(bob.get("routedTo").is_none() && bob["result"].as_str().unwrap().contains("echo:x"));
    // `autoResume:false` impose un run neuf : local, même si l'agent est hébergé.
    let fresh = ok(
        &b,
        "run_agent",
        json!({"runner": "claude", "agentName": "alice", "prompt": "y", "autoResume": false}),
    );
    assert!(fresh.get("routedTo").is_none(), "{fresh}");
    // Un agent arrêté n'est plus routé.
    ok(&a, "agent_stop", json!({"name": "alice"}));
    let after =
        ok(&b, "run_agent", json!({"runner": "claude", "agentName": "alice", "prompt": "z"}));
    assert!(after.get("routedTo").is_none(), "{after}");
}

#[test]
fn parallel_tasks_for_hosted_agents_are_routed_too() {
    let a = start(&[]);
    let b = start_sharing(&a, &[]);
    ok(&a, "create_agent", json!({"name": "alice", "prompt": "p"}));
    ok(&a, "create_agent", json!({"name": "bob", "prompt": "p"}));
    let info = ok(&a, "agent_start", json!({"name": "alice"}));

    let r = ok(
        &b,
        "run_agents_parallel",
        json!({"agents": [
            {"runner": "claude", "agentName": "alice", "prompt": "un", "taskId": "h"},
            {"runner": "claude", "agentName": "bob", "prompt": "deux", "taskId": "l"}
        ]}),
    );
    assert_eq!(r["failed"], 0, "{r}");
    let results = r["results"].as_array().unwrap();
    assert_eq!(results[0]["routedTo"]["instance"], "swarm-control-1", "{r}");
    assert!(results[1].get("routedTo").is_none(), "{r}");
    // La demande est bien passée par la file de l'hôte.
    let status = ok_at(port_of(info["url"].as_str().unwrap()), "status", json!({}));
    assert!(status.to_string().contains("alice"), "{status}");
}
