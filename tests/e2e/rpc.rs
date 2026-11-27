//! Client JSON-RPC minimal (profil strict de LiteMCP) et aides d'assertion.

use crate::harness::Harness;
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::time::Duration;

/// POST /mcp sur le serveur d'administration du harnais.
pub fn rpc(h: &Harness, method: &str, name: Option<&str>, params: Value) -> Value {
    rpc_at(h.port, method, name, params)
}

/// POST /mcp sur `port` (administration, ou serveur d'un agent résident) ; rend le corps JSON.
pub fn rpc_at(port: u16, method: &str, name: Option<&str>, mut params: Value) -> Value {
    params["_meta"] = json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {}
    });
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).to_string();
    let mut head = format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nAccept: application/json\r\n\
         MCP-Protocol-Version: 2026-07-28\r\nMcp-Method: {method}\r\n"
    );
    if let Some(n) = name {
        head.push_str(&format!("Mcp-Name: {n}\r\n"));
    }
    head.push_str(&format!("Content-Length: {}\r\nConnection: close\r\n\r\n", body.len()));

    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
    stream.write_all(head.as_bytes()).unwrap();
    stream.write_all(body.as_bytes()).unwrap();
    let mut raw = Vec::new();
    let _ = stream.read_to_end(&mut raw);
    parse_response(&String::from_utf8_lossy(&raw))
}

fn parse_response(text: &str) -> Value {
    let (headers, payload) = text.split_once("\r\n\r\n").expect("réponse HTTP");
    let payload = if headers.to_lowercase().contains("transfer-encoding: chunked") {
        dechunk(payload)
    } else {
        payload.to_string()
    };
    serde_json::from_str(payload.trim())
        .unwrap_or_else(|e| panic!("JSON invalide ({e}): {payload}"))
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
pub fn call(h: &Harness, tool: &str, args: Value) -> (bool, Value) {
    call_at(h.port, tool, args)
}

pub fn call_at(port: u16, tool: &str, args: Value) -> (bool, Value) {
    let resp = rpc_at(port, "tools/call", Some(tool), json!({"name": tool, "arguments": args}));
    let result = resp.get("result").unwrap_or_else(|| panic!("pas de result: {resp}"));
    let is_error = result.get("isError").and_then(Value::as_bool).unwrap_or(false);
    let payload = result
        .get("structuredContent")
        .cloned()
        .unwrap_or_else(|| result["content"][0]["text"].clone());
    (is_error, payload)
}

/// Appelle un outil et exige le succès.
pub fn ok(h: &Harness, tool: &str, args: Value) -> Value {
    ok_at(h.port, tool, args)
}

pub fn ok_at(port: u16, tool: &str, args: Value) -> Value {
    let (err, v) = call_at(port, tool, args);
    assert!(!err, "{tool} a échoué: {v}");
    v
}

/// Port d'une URL `http://127.0.0.1:PORT/mcp`.
pub fn port_of(url: &str) -> u16 {
    url.rsplit(':').next().unwrap().split('/').next().unwrap().parse().unwrap()
}

/// Fichier d'état d'un agent : `bridge/state/<runner>-<agent>.json`.
pub fn state_file(ws: &Path, runner: &str, agent: &str) -> std::path::PathBuf {
    ws.join("bridge").join("state").join(format!("{runner}-{agent}.json"))
}

fn state_doc(ws: &Path, runner: &str, agent: &str) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(state_file(ws, runner, agent)).ok()?).ok()
}

/// Session sauvegardée de l'agent (`None` si aucune).
pub fn session_of(ws: &Path, runner: &str, agent: &str) -> Option<String> {
    state_doc(ws, runner, agent)?.pointer("/session/id")?.as_str().map(String::from)
}

/// Dernier run de l'agent tel qu'écrit sur disque (`null` si aucun).
pub fn last_run(ws: &Path, runner: &str, agent: &str) -> Value {
    state_doc(ws, runner, agent).and_then(|d| d.get("lastRun").cloned()).unwrap_or(Value::Null)
}

/// Bail de run de l'agent.
pub fn lease_file(ws: &Path, runner: &str, agent: &str) -> std::path::PathBuf {
    ws.join("bridge").join("owners").join(format!("{runner}-{agent}.json"))
}

/// Bail d'hébergement de l'agent (agent résident).
pub fn host_file(ws: &Path, runner: &str, agent: &str) -> std::path::PathBuf {
    ws.join("bridge").join("hosts").join(format!("{runner}-{agent}.json"))
}
