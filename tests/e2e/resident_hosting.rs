//! Hébergement : arrêt propre, exclusivité entre instances, démarrage par `--resident`.

use crate::harness::{command, fake_cli, start, start_sharing, start_sharing_with_args};
use crate::resident::eventually;
use crate::rpc::{call, host_file, ok, ok_at, port_of};
use serde_json::json;
use std::net::TcpStream;

fn is_listening(port: u16) -> bool {
    TcpStream::connect(("127.0.0.1", port)).is_ok()
}

#[test]
fn agent_stop_closes_the_server_and_frees_everything() {
    let h = start(&[]);
    ok(&h, "create_agent", json!({"name": "alice", "prompt": "p"}));
    let info = ok(&h, "agent_start", json!({"name": "alice"}));
    let port = port_of(info["url"].as_str().unwrap());
    assert!(eventually(5, || h.announce_file("swarm-alice").is_file()));
    assert!(is_listening(port));

    // Un agent résident ne peut pas être supprimé.
    let (err, msg) = call(&h, "delete_agent", json!({"name": "alice"}));
    assert!(err && msg.to_string().contains("résident"), "{msg}");

    ok(&h, "agent_stop", json!({"name": "alice"}));
    assert!(eventually(10, || !is_listening(port)), "le serveur de l'agent écoute encore");
    assert!(eventually(5, || !h.announce_file("swarm-alice").exists()), "annonce non retirée");
    assert!(eventually(5, || !host_file(&h.ws, "claude", "alice").exists()), "bail non libéré");

    // L'agent reste défini : il se supprime maintenant, et se relance sans problème.
    let again = ok(&h, "agent_start", json!({"name": "alice"}));
    assert!(again["url"].is_string());
    let (err, _) = call(&h, "agent_stop", json!({"name": "ghost"}));
    assert!(err, "arrêter un agent non résident est une erreur");
}

#[test]
fn only_one_instance_hosts_an_agent_and_the_host_can_change() {
    let a = start(&[]);
    let b = start_sharing(&a, &[]);
    ok(&a, "create_agent", json!({"name": "alice", "prompt": "p"}));
    let hosted = ok(&a, "agent_start", json!({"name": "alice"}));
    let url_a = hosted["url"].as_str().unwrap().to_string();

    // B ne peut pas héberger ce qu'A héberge, et l'erreur dit où il se trouve.
    let (err, msg) = call(&b, "agent_start", json!({"name": "alice"}));
    let msg = msg.to_string();
    assert!(err && msg.contains("AGENT_HOSTED") && msg.contains(&url_a), "{msg}");
    // B ne peut pas l'arrêter non plus : il ne l'héberge pas.
    let (err, msg) = call(&b, "agent_stop", json!({"name": "alice"}));
    assert!(err && msg.to_string().contains("n'est pas hébergé par cette instance"), "{msg}");
    // Les deux instances voient la même résidence.
    let seen = ok(&b, "list_agents", json!({}));
    assert_eq!(seen["agents"][0]["resident"]["url"], url_a.as_str());

    // Une fois arrêté chez A, B peut en devenir l'hôte.
    ok(&a, "agent_stop", json!({"name": "alice"}));
    let moved = ok(&b, "agent_start", json!({"name": "alice"}));
    // L'hôte a changé (le port, lui, peut être réutilisé : premier port libre de la tranche).
    assert_ne!(moved["instance"], hosted["instance"], "{moved}");
    let url_b = moved["url"].as_str().unwrap();
    let reply = ok_at(port_of(url_b), "ask", json!({"prompt": "ici"}));
    assert!(reply["reply"].as_str().unwrap().contains("echo:ici"));
}

#[test]
fn the_resident_flag_hosts_agents_at_launch() {
    let a = start(&[]);
    ok(&a, "create_agent", json!({"name": "alice", "prompt": "p"}));
    ok(&a, "create_agent", json!({"name": "bob", "prompt": "p"}));

    let b = start_sharing_with_args(&a, &[], &["--resident", "alice,bob"]);
    let listed = ok(&b, "list_agents", json!({}));
    for agent in listed["agents"].as_array().unwrap() {
        let url = agent["resident"]["url"].as_str().expect("résident dès le lancement");
        let reply = ok_at(port_of(url), "ask", json!({"prompt": "présent"}));
        assert!(reply["reply"].as_str().unwrap().contains("echo:présent"));
    }
    assert_eq!(listed["count"], 2);
}

#[test]
fn an_unknown_resident_agent_stops_the_launch_with_a_clear_error() {
    let h = start(&[]);
    let output = command(&h.ws, &h.ws, &h.hermes_home)
        .args(["--resident", "fantome"])
        .env("CLAUDE_BIN", fake_cli())
        .output()
        .expect("lancement");
    assert!(!output.status.success(), "le démarrage aurait dû échouer");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--resident fantome") && stderr.contains("introuvable"), "{stderr}");
}
