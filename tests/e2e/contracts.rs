//! Contrats publics figés (TODO-TESTS §1) : chaque sortie est comparée à un fichier de
//! référence dans `tests/golden/` (voir `golden.rs`). Un contrat qui change doit mettre à
//! jour sa référence dans le même commit : `SWARM_BLESS=1 cargo test --test e2e contracts`.
//!
//! Ce qui est figé ici, MCP Control et les scripts le lisent : liste et schémas des outils
//! de l'instance et d'un résident, clés de `status` et de `list_agents {details:true}`,
//! bannière de démarrage, `--help`, `--version`, codes de sortie, noms des variables
//! d'environnement, codes d'erreur MCP.

use crate::golden::{check, check_json, digits};
use crate::harness::{agent_exe, start, start_agent};
use crate::resident::eventually;
use crate::rpc::{call, ok, ok_at, rpc_at};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// Un appel HTTP minimal sur le pont (`GET /status`, `GET /health`).
fn http(port: u16, method: &str, path: &str) -> (u16, Value) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    let head = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(head.as_bytes()).unwrap();
    let mut raw = String::new();
    let _ = stream.read_to_string(&mut raw);
    let (head, body) = raw.split_once("\r\n\r\n").expect("réponse HTTP");
    let status: u16 = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    (status, serde_json::from_str(body).unwrap_or(Value::Null))
}

/// `tools/list` réduit au contrat : nom, description, schéma d'entrée, trié par nom.
fn tools(port: u16) -> Value {
    let listed = rpc_at(port, "tools/list", None, json!({}));
    let mut items: Vec<Value> = listed["result"]["tools"]
        .as_array()
        .expect("une liste d'outils")
        .iter()
        .map(|t| json!({ "name": t["name"], "description": t["description"], "inputSchema": t["inputSchema"] }))
        .collect();
    items.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    Value::Array(items)
}

/// Clés d'un objet, triées, une par ligne — précédées de `label`.
fn keys(label: &str, value: &Value) -> String {
    let mut keys: Vec<&str> =
        value.as_object().expect("un objet").keys().map(String::as_str).collect();
    keys.sort();
    format!("{label}: {}\n", keys.join(", "))
}

/// Bannière comparable : workspace remplacé, nombres masqués (version, ports, instance).
/// Un chemin qui survit (autre forme que celle passée au harnais) fait échouer le test :
/// il ne doit jamais se retrouver figé dans la référence.
fn banner(harness_ws: &Path, banner: &str) -> String {
    let out = digits(&banner.replace(&harness_ws.display().to_string(), "<ws>"));
    let rest = out.replace("http://N.N.N.N:N/mcp", "").replace("http://N.N.N.N:N", "");
    assert!(
        !rest.contains('\\') && !rest.contains('/'),
        "chemin non normalisé dans la bannière : {out}"
    );
    out
}

/// Exécute le binaire avec un workspace jetable ; rend (code de sortie, stdout + stderr).
fn cli(root: &Path, args: &[&str]) -> (i32, String) {
    let ws = root.join("cli-ws");
    std::fs::create_dir_all(&ws).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_swarm-control"));
    cmd.args(["--workspace"]).arg(&ws).args(args);
    cmd.env("USERPROFILE", root).env("HOME", root).env("TEMP", root).env("TMP", root);
    cmd.env("HERMES_HOME", root.join("hermes"));
    cmd.env_remove("SWARM_WORKSPACE");
    let out = cmd.output().expect("binaire lancé");
    let mut text = String::from_utf8_lossy(&out.stdout).to_string();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.code().unwrap_or(-1), text)
}

// --- Outils -------------------------------------------------------------------

#[test]
fn instance_tools_list_is_frozen() {
    let h = start(&[]);
    check_json("tools-instance", &tools(h.port));
}

#[test]
fn resident_tools_list_is_frozen() {
    let h = start(&[]);
    ok(&h, "create_agent", json!({ "name": "alice", "prompt": "Tu es Alice." }));
    let agent = start_agent(&h, &agent_exe(&h, "alice"), &[]);
    check_json("tools-resident", &tools(agent.port));
}

// --- Clés lues par MCP Control -------------------------------------------------

#[test]
fn status_keys_are_frozen_for_the_resident_and_the_bridge() {
    let h = start(&[]);
    ok(&h, "create_agent", json!({ "name": "alice", "prompt": "Tu es Alice." }));
    let agent = start_agent(&h, &agent_exe(&h, "alice"), &["--bridge", "0"]);
    let status = ok_at(agent.port, "status", json!({}));
    let bridge = agent.bridge.expect("pont ouvert");
    assert!(eventually(5, || http(bridge, "GET", "/health").0 == 200), "pont muet");
    let (code, via_bridge) = http(bridge, "GET", "/status");
    assert_eq!(code, 200, "{via_bridge}");
    // Les deux documents portent les mêmes clés : c'est le même `status_json`.
    check(
        "status-keys.txt",
        &format!("{}{}", keys("resident", &status), keys("bridge", &via_bridge)),
    );
    assert_eq!(status.as_object().map(|o| o.len()), via_bridge.as_object().map(|o| o.len()));
}

#[test]
fn list_agents_keys_are_frozen() {
    let h = start(&[]);
    ok(
        &h,
        "create_agent",
        json!({ "name": "alice", "prompt": "Tu es Alice.", "executable": true }),
    );
    let listed = ok(&h, "list_agents", json!({ "details": true }));
    let first = &listed["agents"][0];
    check("list-agents-keys.txt", &keys("list_agents.details", first));
    // Sans `details`, les clés sont un sous-ensemble strict : MCP Control ne doit pas y
    // chercher une clé qui n'existe que dans la vue détaillée.
    let plain = ok(&h, "list_agents", json!({}));
    let short = &plain["agents"][0];
    let short_keys: Vec<&str> = short.as_object().unwrap().keys().map(String::as_str).collect();
    for key in short_keys {
        assert!(first.get(key).is_some(), "clé {key} absente de la vue détaillée");
    }
}

// --- Bannière, aide, version ----------------------------------------------------

#[test]
fn banner_is_frozen_in_instance_and_agent_mode() {
    let h = start(&[]);
    let instance = banner(&h.ws, &h.banner);
    ok(&h, "create_agent", json!({ "name": "alice", "prompt": "Tu es Alice." }));
    let agent = start_agent(&h, &agent_exe(&h, "alice"), &["--bridge", "0"]);
    check(
        "banner.txt",
        &format!(
            "instance: {}\nagent: {}\n",
            instance.trim_end(),
            banner(&h.ws, &agent.banner).trim_end()
        ),
    );
}

#[test]
fn help_and_version_are_frozen() {
    let root = std::env::temp_dir().join(format!("swarm-contracts-help-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (code, help) = cli(&root, &["--help"]);
    assert_eq!(code, 0, "{help}");
    // Seule la version varie (ligne 1) : le reste de l'aide est figé, chiffres compris
    // (plage de ports, délais par défaut) — des constantes du contrat, pas des valeurs locales.
    check("help.txt", &help.replace(env!("CARGO_PKG_VERSION"), "<version>"));
    let (code, version) = cli(&root, &["--version"]);
    assert_eq!(code, 0);
    assert_eq!(version.trim(), format!("swarm-control {}", env!("CARGO_PKG_VERSION")));
    let (code, short) = cli(&root, &["-V"]);
    assert_eq!((code, short.trim()), (0, version.trim()));
    let _ = std::fs::remove_dir_all(&root);
}

// --- Codes de sortie ------------------------------------------------------------

#[test]
fn exit_codes_are_zero_one_two() {
    let root = std::env::temp_dir().join(format!("swarm-contracts-exit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    // 0 : succès — `schema <outil>` imprime le schéma lu sur un serveur vivant (`--port`),
    // sans rien écrire dans le workspace.
    {
        let h = start(&[]);
        let port = h.port.to_string();
        let (code, out) = cli(&root, &["schema", "create_agent", "--port", &port]);
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("\"properties\""), "{out}");
    }
    // 1 : échec d'exécution — un agent inexistant, sans serveur pour le servir.
    let (code, out) = cli(&root, &["status", "--agent", "ghost"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("ghost"), "l'erreur nomme l'agent : {out}");
    // 2 : ligne de commande invalide (`schema` exige un nom d'outil).
    for args in [
        &["--nope"][..],
        &["--port"][..],
        &["--port", "abc"][..],
        &["--instance", "0"][..],
        &["bidon"][..],
        &["schema"][..],
    ] {
        let (code, out) = cli(&root, args);
        assert_eq!(code, 2, "{args:?} → {out}");
    }
    let _ = std::fs::remove_dir_all(&root);
}

// --- Variables d'environnement ----------------------------------------------------

/// Noms des variables d'environnement que le code lit ou pose : les noms `SWARM_*` écrits
/// dans `src/` (plus longs que le préfixe seul), les quatre héritées (`CLAUDE_BIN`,
/// `HERMES_BIN`, `AGY_BIN`, `HERMES_HOME`) et les clés de `app_var("CLÉ")` /
/// `app_lookup_in(…, "CLÉ")`, qui lisent `SWARM_CLÉ` sans que le nom complet soit écrit.
/// Les noms de test (`*_TEST_*`) sont exclus ; un `OVERMIND_*` qui réapparaît casse la
/// référence (K9).
fn environment_variables() -> Vec<String> {
    let mut names = std::collections::BTreeSet::new();
    let mut stack = vec![PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = std::fs::read_to_string(&path).unwrap();
                for token in upper_snake_tokens(&text) {
                    if let Some(name) = env_name(token) {
                        names.insert(name);
                    }
                }
                for key in app_var_keys(&text) {
                    if let Some(name) = env_name(&format!("SWARM_{key}")) {
                        names.insert(name);
                    }
                }
            }
        }
    }
    names.into_iter().collect()
}

/// Suites de majuscules, chiffres et `_` commençant par une lettre (`SWARM_TOKEN`,
/// `HERMES_BIN`…), où qu'elles soient : code, test ou commentaire. Un nom que la doc du
/// code cite fait partie du contrat, et rien ne peut décaler le balayage — un balayage des
/// guillemets se décale dès une séquence d'échappement et rate les littéraux qui suivent.
fn upper_snake_tokens(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for (i, b) in text.bytes().enumerate() {
        let part = b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_';
        match (part, start) {
            (true, None) if b.is_ascii_uppercase() => start = Some(i),
            (true, _) => {}
            (false, Some(s)) => {
                out.push(&text[s..i]);
                start = None;
            }
            (false, None) => {}
        }
    }
    if let Some(s) = start {
        out.push(&text[s..]);
    }
    out
}

/// Clés littérales passées à `app_var("CLÉ")` et `app_lookup_in(…, "CLÉ")` : elles lisent
/// `SWARM_CLÉ` sans que le nom complet apparaisse dans le code.
fn app_var_keys(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut keys = Vec::new();
    for call in ["app_var(", "app_lookup_in("] {
        let mut from = 0;
        while let Some(at) = text[from..].find(call) {
            let open = from + at + call.len();
            from = open;
            let close = text[open..].find(')').map_or(text.len(), |end| open + end);
            let Some(quote) = text[open..close].find('"').map(|q| open + q) else {
                continue;
            };
            // Un argument de l'appel (juste après `(` ou `,`), pas une sous-chaîne.
            let argument = bytes[..quote]
                .iter()
                .rev()
                .find(|b| !b.is_ascii_whitespace())
                .is_some_and(|b| matches!(b, b'(' | b','));
            if !argument {
                continue;
            }
            let Some(end) = text[quote + 1..close].find('"') else {
                continue;
            };
            keys.push(&text[quote + 1..quote + 1 + end]);
        }
    }
    keys
}

/// Un nom de variable : `SWARM_…` (plus long que le préfixe seul), `OVERMIND_…` (K9 : présent,
/// il casse la référence) ou l'une des quatre héritées ; jamais un nom de test.
fn env_name(literal: &str) -> Option<String> {
    let legacy = ["CLAUDE_BIN", "HERMES_BIN", "AGY_BIN", "HERMES_HOME"];
    let candidate = (literal.starts_with("SWARM_") && literal.len() > "SWARM_".len())
        || literal.starts_with("OVERMIND_")
        || legacy.contains(&literal);
    if !candidate
        || literal.contains("_TEST_")
        || !literal.bytes().all(|b| b.is_ascii_uppercase() || b == b'_' || b.is_ascii_digit())
    {
        return None;
    }
    Some(literal.to_string())
}

#[test]
fn un_nom_compose_ou_cite_est_lu_mais_pas_le_prefixe_ni_les_tests() {
    let text = "// pose SWARM_DEBUG=1\n\
        let t = config::app_var(\"TOKEN\").ok();\n\
        let w = env::app_lookup_in(dotenv, \"WORKSPACE\").map(|raw| raw);\n\
        assert!(text.contains(\"${SWARM_TEST_SURELY_ABSENT}\"));\n\
        let name = format!(\"SWARM_{}\", key); // OVERMIND_WORKSPACE\n";
    let found: Vec<String> = upper_snake_tokens(text).into_iter().filter_map(env_name).collect();
    assert_eq!(found, ["SWARM_DEBUG", "OVERMIND_WORKSPACE"]);
    assert_eq!(app_var_keys(text), ["TOKEN", "WORKSPACE"]);
    for key in app_var_keys(text) {
        assert!(env_name(&format!("SWARM_{key}")).is_some(), "{key}");
    }
    // Le préfixe seul n'est pas une variable ; un nom de test est exclu.
    assert_eq!(env_name("SWARM_"), None);
    assert_eq!(env_name("SWARM_TEST_SURELY_ABSENT"), None);
}

#[test]
fn environment_variables_are_frozen_and_documented() {
    let names = environment_variables();
    assert!(!names.is_empty());
    assert!(
        !names.iter().any(|n| n.starts_with("OVERMIND_")),
        "K9 : plus aucun OVERMIND_* ({names:?})"
    );
    check("env-vars.txt", &names.join("\n"));
    // La doc README reprend chaque nom : un test qui compare les deux.
    let readme =
        std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("README.md"))
            .unwrap();
    let missing: Vec<&String> = names.iter().filter(|n| !readme.contains(n.as_str())).collect();
    assert!(missing.is_empty(), "variables lues mais absentes du README : {missing:?}");
}

// --- Erreurs MCP --------------------------------------------------------------------

#[test]
fn mcp_errors_are_frozen() {
    let h = start(&[]);
    let mut cases = Vec::new();
    // Outil inconnu.
    let unknown =
        rpc_at(h.port, "tools/call", Some("nope"), json!({ "name": "nope", "arguments": {} }));
    cases.push(json!({ "case": "unknown tool", "code": unknown["error"]["code"], "message": unknown["error"]["message"] }));
    // Arguments invalides : runner inconnu → -32602 ; le problème nomme le champ fautif
    // (`arguments.runner`, sans répéter la valeur reçue) et `data.issues` les liste.
    let invalid = rpc_at(
        h.port,
        "tools/call",
        Some("run_agent"),
        json!({ "name": "run_agent", "arguments": { "runner": "gemini", "prompt": "x" } }),
    );
    assert_eq!(invalid["error"]["code"], -32602, "{invalid}");
    let issues = invalid["error"]["data"]["issues"].as_array().cloned().expect("data.issues");
    assert!(
        issues.iter().any(|i| i.as_str().is_some_and(|s| s.contains("arguments.runner"))),
        "le problème nomme le champ fautif : {invalid}"
    );
    cases.push(
        json!({ "case": "invalid arguments", "code": invalid["error"]["code"], "issues": issues }),
    );
    // Erreur métier : un agent inexistant rend `isError` avec un code lisible, pas un -32602.
    let (is_error, payload) =
        call(&h, "run_agent", json!({ "runner": "claude", "agentName": "ghost", "prompt": "x" }));
    assert!(is_error, "{payload}");
    assert!(payload.to_string().contains("INVALID_AGENT"), "{payload}");
    cases.push(json!({ "case": "unknown agent", "isError": is_error, "code": "INVALID_AGENT" }));
    check_json("mcp-errors", &Value::Array(cases));
}

// --- Ce que les scripts lisent -------------------------------------------------------

/// Les scripts e2e et les humains repèrent le port par la bannière : les deux marques
/// restent exactement `MCP sur http://…` et `pont HTTP sur http://…`.
#[test]
fn banner_marks_are_stable() {
    let h = start(&[]);
    assert!(h.banner.contains("MCP sur http://127.0.0.1:"), "{}", h.banner);
    ok(&h, "create_agent", json!({ "name": "alice", "prompt": "Tu es Alice." }));
    let agent = start_agent(&h, &agent_exe(&h, "alice"), &["--bridge", "0"]);
    assert!(agent.banner.contains("MCP sur http://127.0.0.1:"), "{}", agent.banner);
    assert!(agent.banner.contains("pont HTTP sur http://127.0.0.1:"), "{}", agent.banner);
}
