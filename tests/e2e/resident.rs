//! Un agent résident : serveur MCP propre, `ask` / `status`, annonce, session conservée.

use crate::harness::{AgentProcess, Harness, start, start_agent};
use crate::rpc::{call_at, host_file, lease_file, ok, ok_at, port_of, rpc_at, session_of};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
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
    assert_eq!(
        tools,
        [
            "ask",
            "config",
            "history",
            "logs",
            "memory_forget",
            "memory_search",
            "memory_store",
            "reload",
            "session",
            "status",
            "stop"
        ]
    );

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

/// Requête HTTP brute sur le pont d'un agent ; rend (statut HTTP, corps JSON).
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
    let status: u16 = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    (status, serde_json::from_str(body).unwrap_or(Value::Null))
}

/// `POST /send` sur le pont avec le contexte Discord attendu du routeur ; exige 200.
fn send(port: u16, message: &str) -> Value {
    let (status, body) = http(
        port,
        "POST",
        "/send",
        Some(json!({
            "message": message,
            "userId": "42",
            "username": "bob",
            "channelId": "1442317829998383235"
        })),
    );
    assert_eq!(status, 200, "{body}");
    body
}

/// Démarre `name` en mode agent avec son pont HTTP ; rend (process, port du pont).
fn bridged(h: &Harness, name: &str) -> (AgentProcess, u16) {
    let agent = start_agent(
        h,
        Path::new(env!("CARGO_BIN_EXE_swarm-control")),
        &["--agent", name, "--bridge", "0"],
    );
    let port = agent.bridge.expect("port du pont dans la bannière");
    (agent, port)
}

#[test]
fn the_bridge_serves_a_hermes_agent_reply_on_stdout_and_session_on_stderr() {
    let h = start(&[]);
    ok(&h, "create_agent", json!({"name": "hera", "runner": "hermes", "prompt": "p"}));
    let (_agent, port) = bridged(&h, "hera");

    // Réponse du faux CLI hermes : le texte sort sur stdout, la session sur stderr.
    let first = send(port, "bonjour");
    let text = first["result"].as_str().unwrap();
    assert!(text.contains("echo:") && text.contains("MESSAGE: bonjour"), "{text}");
    assert!(text.contains("resumed:none") && text.contains("agent:hera"), "{text}");
    assert!(!text.contains("session_id:"), "la ligne de stderr ne doit pas fuir : {text}");
    let sid = first["session_id"].as_str().expect("session lue sur stderr").to_string();

    // La session hermes est enregistrée : le tour suivant la reprend.
    assert_eq!(session_of(&h.ws, "hermes", "hera").as_deref(), Some(sid.as_str()));
    let second = send(port, "encore");
    assert!(second["result"].as_str().unwrap().contains(&format!("resumed:{sid}")), "{second}");

    // /status et /health parlent bien de l'agent hermes.
    let (status, state) = http(port, "GET", "/status", None);
    assert_eq!(status, 200);
    assert_eq!(state["agent"], "hera");
    assert_eq!(state["runner"], "hermes");
    assert_eq!(state["turnsDone"], 2);
    assert_eq!(http(port, "GET", "/health", None), (200, json!({"status": "ok", "agent": "hera"})));
}

#[test]
fn a_reset_during_a_running_turn_discards_the_session_of_that_turn() {
    let h = start(&[]);
    ok(&h, "create_agent", json!({"name": "hera", "runner": "hermes", "prompt": "p"}));
    let (_agent, port) = bridged(&h, "hera");

    // Un premier tour crée la session de l'agent.
    let first = send(port, "premier");
    let sid = first["session_id"].as_str().expect("session initiale").to_string();
    assert_eq!(session_of(&h.ws, "hermes", "hera").as_deref(), Some(sid.as_str()));

    // Un tour lent démarre, puis /reset arrive pendant qu'il tourne.
    let slow = std::thread::spawn(move || send(port, "SLOW en cours"));
    assert!(
        eventually(10, || http(port, "GET", "/status", None).1["state"] == "busy"),
        "le tour lent n'a pas démarré"
    );
    let (status, reset) = http(port, "POST", "/reset", None);
    assert_eq!(status, 200, "{reset}");
    assert_eq!(reset["hadSession"], true, "{reset}");
    // La session est oubliée sur disque dès le reset, sans attendre la fin du tour.
    assert_eq!(session_of(&h.ws, "hermes", "hera"), None, "{reset}");

    // Le tour en cours aboutit normalement (le reset ne l'interrompt pas)...
    let slow_reply = slow.join().unwrap();
    let slow_text = slow_reply["result"].as_str().unwrap();
    assert!(
        slow_text.contains("echo:") && slow_text.contains("MESSAGE: SLOW en cours"),
        "{slow_text}"
    );
    // ...mais sa session n'est pas enregistrée : le reset demandé s'applique à sa fin.
    assert_eq!(session_of(&h.ws, "hermes", "hera"), None, "le tour fini a réécrit la session");

    // Le prochain tour repart à neuf, avec une nouvelle session conservée.
    let fresh = send(port, "on recommence");
    let fresh_text = fresh["result"].as_str().unwrap();
    assert!(fresh_text.contains("resumed:none"), "{fresh_text}");
    let new_sid = fresh["session_id"].as_str().expect("nouvelle session").to_string();
    assert_ne!(new_sid, sid, "une nouvelle session doit être créée");
    assert_eq!(session_of(&h.ws, "hermes", "hera").as_deref(), Some(new_sid.as_str()));
}
