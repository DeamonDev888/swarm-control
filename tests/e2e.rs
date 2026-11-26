//! Tests de bout en bout : le vrai serveur `overmind`, lancé sur un port
//! éphémère avec un workspace jetable, et un faux CLI (`examples/fake_cli.rs`)
//! à la place de `claude` / `hermes`. Aucun appel réseau, aucun jeton réel.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

static COUNTER: AtomicU32 = AtomicU32::new(0);

struct Harness {
    child: Child,
    port: u16,
    root: PathBuf,
    ws: PathBuf,
    hermes_home: PathBuf,
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn fake_cli() -> PathBuf {
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_overmind"));
    let name = if cfg!(windows) { "fake_cli.exe" } else { "fake_cli" };
    let path = exe.parent().unwrap().join("examples").join(name);
    assert!(path.exists(), "faux CLI absent ({}): lance `cargo test` (build des exemples inclus)", path.display());
    path
}

fn start(extra_env: &[(&str, &str)]) -> Harness {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let root = std::env::temp_dir().join(format!("overmind-e2e-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let ws = root.join("ws");
    let hermes_home = root.join("hermes");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::create_dir_all(hermes_home.join("profiles")).unwrap();
    // Serveurs MCP connus du workspace.
    std::fs::write(
        ws.join(".mcp.json"),
        r#"{"mcpServers":{"memory":{"url":"http://127.0.0.1:3099/mcp"},"db":{"command":"node","args":["db.js"]}}}"#,
    )
    .unwrap();

    let cli = fake_cli();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_overmind"));
    cmd.args(["--port", "0", "--workspace"])
        .arg(&ws)
        .env("USERPROFILE", &root)
        .env("HOME", &root)
        .env("HERMES_HOME", &hermes_home)
        .env("CLAUDE_BIN", &cli)
        .env("HERMES_BIN", &cli)
        .env("ANTHROPIC_AUTH_TOKEN", "primary-token")
        .env_remove("OVERMIND_WORKSPACE")
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().expect("lancement du serveur");
    let mut reader = BufReader::new(child.stderr.take().unwrap());
    let mut port = 0u16;
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut line = String::new();
    while Instant::now() < deadline {
        line.clear();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            break;
        }
        if let Some(rest) = line.split("http://127.0.0.1:").nth(1) {
            port = rest.split('/').next().unwrap().parse().unwrap();
            break;
        }
    }
    assert!(port != 0, "le serveur n'a pas annoncé son port");
    // Vide stderr en arrière-plan pour que le serveur ne bloque jamais dessus.
    std::thread::spawn(move || {
        let mut sink = Vec::new();
        let _ = reader.read_to_end(&mut sink);
    });
    Harness { child, port, root, ws, hermes_home }
}

/// POST /mcp en profil strict ; rend le corps JSON de la réponse.
fn rpc(h: &Harness, method: &str, name: Option<&str>, params: Value) -> Value {
    let mut params = params;
    params["_meta"] = json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {}
    });
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).to_string();
    let mut head = format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nAccept: application/json\r\n\
         MCP-Protocol-Version: 2026-07-28\r\nMcp-Method: {method}\r\n",
        h.port
    );
    if let Some(n) = name {
        head.push_str(&format!("Mcp-Name: {n}\r\n"));
    }
    head.push_str(&format!("Content-Length: {}\r\nConnection: close\r\n\r\n", body.len()));
    let mut stream = TcpStream::connect(("127.0.0.1", h.port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
    stream.write_all(head.as_bytes()).unwrap();
    stream.write_all(body.as_bytes()).unwrap();
    let mut raw = Vec::new();
    let _ = stream.read_to_end(&mut raw);
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (headers, payload) = text.split_once("\r\n\r\n").expect("réponse HTTP");
    let payload = if headers.to_lowercase().contains("transfer-encoding: chunked") {
        dechunk(payload)
    } else {
        payload.to_string()
    };
    serde_json::from_str(payload.trim()).unwrap_or_else(|e| panic!("JSON invalide ({e}): {payload}"))
}

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

/// Appelle un outil : `(isError, structuredContent ou texte)`.
fn call(h: &Harness, tool: &str, args: Value) -> (bool, Value) {
    let resp = rpc(h, "tools/call", Some(tool), json!({"name": tool, "arguments": args}));
    let result = resp.get("result").unwrap_or_else(|| panic!("pas de result: {resp}"));
    let is_error = result.get("isError").and_then(Value::as_bool).unwrap_or(false);
    let payload = result
        .get("structuredContent")
        .cloned()
        .unwrap_or_else(|| result["content"][0]["text"].clone());
    (is_error, payload)
}

fn ok(h: &Harness, tool: &str, args: Value) -> Value {
    let (err, v) = call(h, tool, args);
    assert!(!err, "{tool} a échoué: {v}");
    v
}

fn sessions_file(ws: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(ws.join("bridge/agents.json")).unwrap()).unwrap()
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[test]
fn exposes_the_eight_tools() {
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
            "create_agent",
            "delete_agent",
            "get_agent_configs",
            "list_agents",
            "run_agent",
            "run_agents_parallel",
            "update_agent_config"
        ]
    );
}

#[test]
fn claude_agent_persists_its_session() {
    let h = start(&[]);
    let created = ok(&h, "create_agent", json!({"name": "alice", "prompt": "Tu es Alice.", "model": "claude-sonnet-4-6"}));
    assert_eq!(created["runner"], "claude");
    assert!(h.ws.join(".claude/agents/alice.md").is_file());
    assert!(h.ws.join(".claude/settings_alice.json").is_file());
    assert!(h.ws.join(".claude/.mcp.alice.json").is_file());

    // Doublon refusé.
    let (err, _) = call(&h, "create_agent", json!({"name": "alice", "prompt": "x"}));
    assert!(err);

    let first = ok(&h, "run_agent", json!({"runner": "claude", "agentName": "alice", "prompt": "bonjour"}));
    let text = first["result"].as_str().unwrap();
    assert!(text.contains("echo:bonjour"), "{text}");
    assert!(text.contains("resumed:none"), "{text}");
    assert!(text.contains("model:claude-sonnet-4-6"), "{text}");
    assert!(text.contains("agent:alice"), "{text}");
    let sid = first["sessionId"].as_str().unwrap().to_string();

    // La session est écrite au format partagé avec l'OverMind TS.
    assert_eq!(sessions_file(&h.ws)["claude:alice"]["id"], sid.as_str());

    // Deuxième appel : reprise automatique de la même session.
    let second = ok(&h, "run_agent", json!({"runner": "claude", "agentName": "alice", "prompt": "encore"}));
    assert!(second["result"].as_str().unwrap().contains(&format!("resumed:{sid}")));
    assert_eq!(second["sessionId"], sid.as_str());

    // autoResume:false → session neuve, et le fichier de session est mis à jour.
    let fresh = ok(&h, "run_agent", json!({"runner": "claude", "agentName": "alice", "prompt": "neuf!", "autoResume": false}));
    assert!(fresh["result"].as_str().unwrap().contains("resumed:none"));
    assert_ne!(fresh["sessionId"], sid.as_str());

    // Les fichiers temporaires du run sont nettoyés.
    let leftovers: Vec<_> = std::fs::read_dir(h.ws.join(".claude"))
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains("_run_"))
        .collect();
    assert!(leftovers.is_empty(), "fichiers temporaires restants");
}

#[test]
fn dead_session_is_dropped_and_retried_fresh() {
    let h = start(&[]);
    ok(&h, "create_agent", json!({"name": "bob", "prompt": "p"}));
    let first = ok(&h, "run_agent", json!({"runner": "claude", "agentName": "bob", "prompt": "un"}));
    let old = first["sessionId"].as_str().unwrap().to_string();
    // La session reprise est « périmée » : le serveur doit la jeter et relancer à neuf.
    let again = ok(&h, "run_agent", json!({"runner": "claude", "agentName": "bob", "prompt": "DEADSESSION"}));
    assert!(again["result"].as_str().unwrap().contains("resumed:none"));
    assert_ne!(again["sessionId"], old.as_str());
    assert_eq!(sessions_file(&h.ws)["claude:bob"]["id"], again["sessionId"]);
}

#[test]
fn background_run_can_be_listed_and_killed() {
    let h = start(&[]);
    ok(&h, "create_agent", json!({"name": "carl", "prompt": "p"}));
    let started = ok(&h, "run_agent", json!({"runner": "claude", "agentName": "carl", "prompt": "SLEEP", "background": true}));
    let run_id = started["runId"].as_str().unwrap().to_string();
    assert_eq!(started["status"], "running");

    // Le même agent ne peut pas lancer un second run en parallèle.
    let (err, msg) = call(&h, "run_agent", json!({"runner": "claude", "agentName": "carl", "prompt": "x"}));
    assert!(err && msg.to_string().contains("AGENT_BUSY"), "{msg}");

    // Le PID doit apparaître, puis le registre disque l'enregistre.
    let mut status = Value::Null;
    for _ in 0..50 {
        status = ok(&h, "agent_control", json!({"action": "status", "agentName": "carl"}));
        if status["pid"].is_number() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(status["status"], "running");
    assert!(status["pid"].is_number());
    let disk: Value = serde_json::from_str(&std::fs::read_to_string(h.ws.join("bridge/process-registry.json")).unwrap()).unwrap();
    assert_eq!(disk["processes"]["claude:carl"]["status"], "running");

    let listed = ok(&h, "agent_control", json!({"action": "list"}));
    assert_eq!(listed["count"], 1);

    let killed = ok(&h, "agent_control", json!({"action": "kill", "runId": run_id}));
    assert_eq!(killed["killed"], true);
    assert_eq!(killed["run"]["status"], "killed");
    let disk: Value = serde_json::from_str(&std::fs::read_to_string(h.ws.join("bridge/process-registry.json")).unwrap()).unwrap();
    assert_eq!(disk["processes"]["claude:carl"]["status"], "failed");

    // Agent libre de nouveau.
    ok(&h, "run_agent", json!({"runner": "claude", "agentName": "carl", "prompt": "après"}));
}

#[test]
fn timeout_stops_the_process() {
    let h = start(&[("AGENT_TIMEOUT_MS", "1500")]);
    ok(&h, "create_agent", json!({"name": "dora", "prompt": "p"}));
    let t = Instant::now();
    let (err, msg) = call(&h, "run_agent", json!({"runner": "claude", "agentName": "dora", "prompt": "SLEEP"}));
    assert!(err && msg.to_string().contains("TIMEOUT"), "{msg}");
    assert!(t.elapsed() < Duration::from_secs(20), "le timeout n'a pas coupé le process");
}

#[test]
fn fallback_token_is_used_on_401() {
    let h = start(&[]);
    ok(&h, "create_agent", json!({"name": "eve", "prompt": "p"}));
    ok(
        &h,
        "update_agent_config",
        json!({"name": "eve", "env": {"AUTH_FALLBACK_1": "backup-token"}}),
    );
    let r = ok(&h, "run_agent", json!({"runner": "claude", "agentName": "eve", "prompt": "FAIL401"}));
    assert!(r["result"].as_str().unwrap().contains("token:backup-token"), "{r}");
}

#[test]
fn hermes_agent_lifecycle() {
    let h = start(&[]);
    let created = ok(
        &h,
        "create_agent",
        json!({"name": "hera", "runner": "hermes", "prompt": "Tu es Hera.", "model": "glm-5.3-flash"}),
    );
    assert_eq!(created["runner"], "hermes");
    let profile = h.hermes_home.join("profiles/hera");
    assert_eq!(std::fs::read_to_string(profile.join("SOUL.md")).unwrap(), "Tu es Hera.");
    // Les serveurs du .mcp.json sont écrits dans config.yaml.
    let yaml = std::fs::read_to_string(profile.join("config.yaml")).unwrap();
    assert!(yaml.contains("mcp_servers:") && yaml.contains("memory:") && yaml.contains("url: http://127.0.0.1:3099/mcp"), "{yaml}");

    let first = ok(&h, "run_agent", json!({"runner": "hermes", "agentName": "hera", "prompt": "salut \"quoted\" $VAR"}));
    let text = first["result"].as_str().unwrap();
    assert!(text.contains("echo:salut \"quoted\" $VAR"), "{text}");
    assert!(text.contains("agent:hera"));
    let sid = first["sessionId"].as_str().unwrap().to_string();
    assert_eq!(sessions_file(&h.ws)["hermes:hera"]["id"], sid.as_str());

    let second = ok(&h, "run_agent", json!({"runner": "hermes", "agentName": "hera", "prompt": "suite"}));
    assert!(second["result"].as_str().unwrap().contains(&format!("resumed:{sid}")));

    // update → SOUL.md et listing.
    ok(&h, "update_agent_config", json!({"name": "hera", "prompt": "Nouveau prompt"}));
    assert_eq!(std::fs::read_to_string(profile.join("SOUL.md")).unwrap(), "Nouveau prompt");
    let listed = ok(&h, "list_agents", json!({"details": true}));
    let hera = listed["agents"].as_array().unwrap().iter().find(|a| a["name"] == "hera").unwrap();
    assert_eq!(hera["runner"], "hermes");
    assert_eq!(hera["model"], "glm-5.3-flash");
    assert_eq!(hera["hasSession"], true);

    // delete → profil et session disparaissent.
    ok(&h, "delete_agent", json!({"name": "hera"}));
    assert!(!profile.exists());
    assert!(sessions_file(&h.ws).get("hermes:hera").is_none());
}

#[test]
fn run_validation_errors() {
    let h = start(&[]);
    // Agent inexistant.
    let (err, msg) = call(&h, "run_agent", json!({"runner": "claude", "agentName": "ghost", "prompt": "x"}));
    assert!(err && msg.to_string().contains("INVALID_AGENT"), "{msg}");
    // Nom piégé : refusé avant tout accès disque.
    let (err, msg) = call(&h, "run_agent", json!({"runner": "claude", "agentName": "../etc", "prompt": "x"}));
    assert!(err, "{msg}");
    // Schéma : runner inconnu → erreur JSON-RPC -32602.
    let resp = rpc(&h, "tools/call", Some("run_agent"), json!({"name": "run_agent", "arguments": {"runner": "gemini", "prompt": "x"}}));
    assert_eq!(resp["error"]["code"], -32602, "{resp}");
}

#[test]
fn parallel_agents_and_config_masking() {
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

    // Le jeton n'est jamais renvoyé en clair par get_agent_configs.
    let cfg = ok(&h, "get_agent_configs", json!({"name": "p1"}));
    let settings = cfg["files"]["settings.json"].as_str().unwrap();
    assert!(!settings.contains("primary-token"), "{settings}");
    assert!(cfg["files"]["prompt.md"].as_str().unwrap() == "a");

    // Mise à jour du MCP : fichier de l'agent régénéré.
    let up = ok(&h, "update_agent_config", json!({"name": "p1", "mcpServers": ["memory"]}));
    assert!(up["changes"].to_string().contains("memory"));
    let mcp: Value = serde_json::from_str(&std::fs::read_to_string(h.ws.join(".claude/.mcp.p1.json")).unwrap()).unwrap();
    assert!(mcp["mcpServers"].get("memory").is_some() && mcp["mcpServers"].get("db").is_none());
}

#[test]
fn hermes_provider_errors_are_errors_with_hints() {
    let h = start(&[]);
    ok(&h, "create_agent", json!({"name": "hugo", "runner": "hermes", "prompt": "p"}));
    // Code de sortie 1 + texte sur stdout (cas réel : crédits épuisés) => erreur, pas succès.
    let (err, msg) = call(&h, "run_agent", json!({"runner": "hermes", "agentName": "hugo", "prompt": "PROVIDERFAIL"}));
    let msg = msg.to_string();
    assert!(err && msg.contains("EXIT_CODE_1") && msg.contains("Billing or credits exhausted"), "{msg}");
    assert!(msg.contains("crédits du fournisseur épuisés"), "indice manquant: {msg}");
    // Identifiants absents : message d'aide actionnable.
    let (err, msg) = call(&h, "run_agent", json!({"runner": "hermes", "agentName": "hugo", "prompt": "NOCRED"}));
    let msg = msg.to_string();
    assert!(err && msg.contains("hermes -p hugo auth add"), "{msg}");
}

#[test]
fn hermes_names_credentials_and_clone() {
    let h = start(&[]);
    // Règle Hermes : minuscules uniquement.
    let (err, msg) = call(&h, "create_agent", json!({"name": "Hera", "runner": "hermes", "prompt": "p"}));
    assert!(err && msg.to_string().contains("minuscules"), "{msg}");

    // Profil sans credentials : l'agent est créé mais l'avertissement est explicite.
    let created = ok(&h, "create_agent", json!({"name": "nocreds", "runner": "hermes", "prompt": "p", "model": "glm-5.3-flash"}));
    let warn = created["warnings"].to_string();
    assert!(warn.contains("PAR PROFIL") && warn.contains("auth add zai"), "{warn}");

    // cloneFrom : source inexistante refusée ; source avec .env => pas d'avertissement.
    let (err, _) = call(&h, "create_agent", json!({"name": "c1", "runner": "hermes", "prompt": "p", "cloneFrom": "ghost"}));
    assert!(err);
    let src = h.hermes_home.join("profiles/source");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("config.yaml"), "model:\n  default: x\n").unwrap();
    std::fs::write(src.join(".env"), "ZAI_API_KEY=test\n").unwrap();
    let cloned = ok(&h, "create_agent", json!({"name": "c2", "runner": "hermes", "prompt": "p", "cloneFrom": "source", "model": "glm-5.3-flash"}));
    assert!(cloned.get("warnings").is_none(), "{cloned}");
    assert!(h.hermes_home.join("profiles/c2/.env").is_file());

    // delete : pas de fausse erreur quand le profil a bien disparu.
    let d = ok(&h, "delete_agent", json!({"name": "c2"}));
    assert!(d.get("errors").is_none(), "{d}");
    assert!(!h.hermes_home.join("profiles/c2").exists());
}
