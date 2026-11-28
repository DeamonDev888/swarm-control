//! Hébergement : arrêt propre, exclusivité entre instances, démarrage par `--resident`.

use crate::harness::{command, fake_cli, start, start_sharing, start_sharing_with_args};
use crate::resident::eventually;
use crate::rpc::{call, host_file, ok, ok_at, port_of};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

fn is_listening(port: u16) -> bool {
    TcpStream::connect(("127.0.0.1", port)).is_ok()
}

/// Tente un `initialize` MCP sur `port` (même profil HTTP que `rpc::rpc_at`) ; rend le
/// nom que le serveur qui répond s'annonce (`serverInfo.name`), ou `None` si le port ne
/// répond pas : refus de connexion, silence, ou réponse illisible.
fn announced_name(port: u16) -> Option<String> {
    let params = json!({
        "protocolVersion": "2026-07-28",
        "capabilities": {},
        "clientInfo": {"name": "e2e", "version": "0"},
        "_meta": {
            "io.modelcontextprotocol/protocolVersion": "2026-07-28",
            "io.modelcontextprotocol/clientCapabilities": {}
        }
    });
    let body =
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": params}).to_string();
    let head = format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nAccept: application/json\r\n\
         MCP-Protocol-Version: 2026-07-28\r\nMcp-Method: initialize\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(500)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    stream.write_all(head.as_bytes()).ok()?;
    stream.write_all(body.as_bytes()).ok()?;
    let mut raw = String::new();
    stream.read_to_string(&mut raw).ok()?;
    let (headers, payload) = raw.split_once("\r\n\r\n")?;
    let payload = if headers.to_lowercase().contains("transfer-encoding: chunked") {
        dechunk(payload)
    } else {
        payload.to_string()
    };
    let value: Value = serde_json::from_str(payload.trim()).ok()?;
    value["result"]["serverInfo"]["name"].as_str().map(String::from)
}

/// Déchunkage minimal d'un corps HTTP chunked (même logique que rpc.rs, qui n'expose pas
/// cette aide).
fn dechunk(mut s: &str) -> String {
    let mut out = String::new();
    while let Some((size, rest)) = s.split_once("\r\n") {
        let n = usize::from_str_radix(size.trim(), 16).unwrap_or(0);
        if n == 0 || rest.len() < n {
            break;
        }
        out.push_str(&rest[..n]);
        s = rest[n..].trim_start_matches("\r\n");
    }
    out
}

/// Le port ne sert plus **cet** agent : le port vient d'être libéré, mais le serveur
/// d'un test parallèle peut l'avoir aussitôt repris (premier port libre de la tranche
/// 23080-23099, ou réattribution OS) — « le port est fermé » n'est donc pas une
/// condition fiable en suite complète. On accepte ici : refus de connexion, silence,
/// ou un serveur qui répond sans s'annoncer `swarm-alice`.
fn no_longer_serves_agent(port: u16) -> bool {
    match announced_name(port) {
        Some(name) => name != "swarm-alice",
        None => true,
    }
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
    // Le port peut être repris à chaud par un autre test : on vérifie que **cet** agent
    // n'y est plus servi, pas que le port soit fermé (voir `no_longer_serves_agent`).
    assert!(eventually(10, || no_longer_serves_agent(port)), "le serveur de l'agent écoute encore");
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
