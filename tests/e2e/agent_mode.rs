//! Mode agent : un exécutable par agent, et le pont HTTP compatible `overmind-bridge`.

use crate::harness::{agent_exe, command, start, start_agent};
use crate::rpc::{ok, ok_at, rpc_at};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

/// Requête HTTP brute sur le pont ; rend (statut, corps JSON).
fn http(port: u16, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
    let payload = body.map(|b| b.to_string()).unwrap_or_default();
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
    let head = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n",
        payload.len()
    );
    stream.write_all(head.as_bytes()).unwrap();
    stream.write_all(payload.as_bytes()).unwrap();
    let mut raw = String::new();
    stream.read_to_string(&mut raw).unwrap();
    let (head, body) = raw.split_once("\r\n\r\n").expect("réponse HTTP");
    let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    (status, serde_json::from_str(body).unwrap_or(Value::Null))
}

fn send(port: u16, message: &str) -> Value {
    let (status, body) = http(
        port,
        "POST",
        "/send",
        Some(
            json!({"message": message, "userId": "42", "username": "bob", "channelId": "1442317829998383235"}),
        ),
    );
    assert_eq!(status, 200, "{body}");
    body
}

#[test]
fn a_renamed_binary_serves_its_agent_alone() {
    let h = start(&[]);
    ok(&h, "create_agent", json!({"name": "alice", "prompt": "Tu es Alice."}));
    let agent = start_agent(&h, &agent_exe(&h, "alice"), &[]);

    // Son serveur MCP : les outils de l'agent, pas ceux d'une instance d'administration.
    let listed = rpc_at(agent.port, "tools/list", None, json!({}));
    let mut tools: Vec<&str> = listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    tools.sort();
    assert_eq!(tools, ["ask", "status", "stop"]);
    let reply = ok_at(agent.port, "ask", json!({"prompt": "bonjour"}));
    assert!(reply["reply"].as_str().unwrap().contains("echo:bonjour"), "{reply}");
    assert_eq!(agent.bridge, None, "pas de pont sans --bridge");

    // L'instance d'administration voit l'agent comme résident, hébergé ailleurs.
    let seen = ok(&h, "list_agents", json!({}));
    assert!(
        seen["agents"][0]["resident"]["url"].as_str().unwrap().contains(&agent.port.to_string())
    );
}

#[test]
fn the_bridge_speaks_the_overmind_bridge_dialect() {
    let h = start(&[]);
    ok(&h, "create_agent", json!({"name": "sniper", "prompt": "Tu es Sniper."}));
    // `--agent` choisit l'agent quel que soit le nom de l'exécutable.
    let agent = start_agent(
        &h,
        std::path::Path::new(env!("CARGO_BIN_EXE_swarm-control")),
        &["--agent", "sniper", "--bridge", "0"],
    );
    let port = agent.bridge.expect("port du pont dans la bannière");

    // Le message arrive avec le contexte Discord attendu par les SOUL.md existantes.
    let first = send(port, "analyse AAPL");
    let text = first["result"].as_str().unwrap();
    for part in [
        "[DISCORD CONTEXT]",
        "CHANNEL_ID: 1442317829998383235",
        "USER_NAME: bob",
        "USER_ID: 42",
        "MESSAGE: analyse AAPL",
        "resumed:none",
    ] {
        assert!(text.contains(part), "{part} absent de : {text}");
    }
    let sid = first["session_id"].as_str().expect("session_id").to_string();

    // Même session au message suivant : l'agent se souvient.
    let second = send(port, "et MSFT ?");
    assert!(second["result"].as_str().unwrap().contains(&format!("resumed:{sid}")), "{second}");

    // /reset : la session est oubliée, le prochain message repart à neuf.
    let (status, reset) = http(port, "POST", "/reset", None);
    assert_eq!((status, &reset["hadSession"]), (200, &json!(true)), "{reset}");
    let fresh = send(port, "on recommence");
    assert!(fresh["result"].as_str().unwrap().contains("resumed:none"), "{fresh}");

    // /status et /health ; erreurs propres.
    let (status, state) = http(port, "GET", "/status", None);
    assert_eq!(
        (status, state["agent"].as_str(), state["turnsDone"].as_u64()),
        (200, Some("sniper"), Some(3))
    );
    assert_eq!(
        http(port, "GET", "/health", None),
        (200, json!({"status": "ok", "agent": "sniper"}))
    );
    let (status, missing) = http(port, "POST", "/send", Some(json!({"userId": "42"})));
    assert_eq!((status, missing["error"].as_str()), (400, Some("Message is required")));
    assert_eq!(http(port, "GET", "/send", None).0, 405);
    assert_eq!(http(port, "GET", "/nope", None).0, 404);
}

#[test]
fn bad_agent_mode_launches_fail_clearly() {
    let h = start(&[]);
    let run = |args: &[&str]| {
        let out = command(&h.ws, &h.ws, &h.hermes_home).args(args).output().expect("lancement");
        (out.status.code(), String::from_utf8_lossy(&out.stderr).into_owned())
    };
    let (code, err) = run(&["--bridge", "0"]);
    assert_eq!(code, Some(2), "{err}");
    assert!(err.contains("--agent"), "{err}");
    let (code, err) = run(&["--agent", "fantome"]);
    assert_ne!(code, Some(0));
    assert!(err.contains("fantome") && err.contains("introuvable"), "{err}");
}
