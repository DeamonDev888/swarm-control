//! Journal des tours et outils riches du serveur d'agent : `history`, `session`, `logs`, `config`.

use crate::harness::start;
use crate::rpc::{ok, ok_at, port_of};
use serde_json::{Value, json};

/// Trois tours, puis le journal les rend dans l'ordre, avec les durées.
#[test]
fn three_turns_are_journaled_and_history_pages_backwards() {
    let h = start(&[]);
    ok(&h, "create_agent", json!({"name": "alice", "prompt": "p"}));
    let info = ok(&h, "agent_start", json!({"name": "alice"}));
    let port = port_of(info["url"].as_str().unwrap());
    for prompt in ["un", "deux", "trois"] {
        ok_at(port, "ask", json!({"prompt": prompt}));
    }

    let hist = ok_at(port, "history", json!({"limit": 10}));
    let entries = hist["entries"].as_array().expect("entries");
    assert_eq!(entries.len(), 3, "{hist}");
    let prompts: Vec<&str> = entries.iter().map(|e| e["prompt"].as_str().unwrap()).collect();
    assert_eq!(prompts, ["un", "deux", "trois"]);
    assert!(entries[2]["reply"].as_str().unwrap().contains("echo:trois"));
    assert!(entries[2]["durationMs"].as_u64().is_some());
    assert_eq!(entries[2]["runner"], "claude");

    // Pagination : les deux derniers, puis ceux d'avant.
    let last2 = ok_at(port, "history", json!({"limit": 2}));
    assert_eq!(last2["entries"].as_array().unwrap().len(), 2);
    assert_eq!(last2["entries"][1]["prompt"], "trois");
    let before = entries[1]["ts"].as_u64().unwrap();
    let older = ok_at(port, "history", json!({"limit": 10, "before": before}));
    assert_eq!(older["entries"].as_array().unwrap().len(), 1);
    assert_eq!(older["entries"][0]["prompt"], "un");

    // Le journal vit dans le workspace, en JSONL.
    let file = h.ws.join("bridge").join("history").join("claude-alice.jsonl");
    assert!(file.is_file(), "journal absent: {}", file.display());
    assert_eq!(std::fs::read_to_string(&file).unwrap().lines().count(), 3);
}

/// `session` : show / set / reset, et `logs` / `config` répondent.
#[test]
fn session_logs_and_config_tools() {
    let h = start(&[]);
    ok(&h, "create_agent", json!({"name": "bob", "prompt": "p"}));
    let info = ok(&h, "agent_start", json!({"name": "bob"}));
    let port = port_of(info["url"].as_str().unwrap());
    ok_at(port, "ask", json!({"prompt": "bonjour"}));

    let shown = ok_at(port, "session", json!({"action": "show"}));
    assert!(shown["session"].is_string(), "session après un tour");

    let forced = ok_at(port, "session", json!({"action": "set", "id": "session-forcée"}));
    assert_eq!(forced["session"], "session-forcée");
    let shown = ok_at(port, "session", json!({"action": "show"}));
    assert_eq!(shown["session"], "session-forcée");

    let reset = ok_at(port, "session", json!({"action": "reset"}));
    assert_eq!(reset["hadSession"], true);
    assert!(ok_at(port, "session", json!({"action": "show"}))["session"].is_null());

    // Sans id, `set` est refusé.
    let (err, message) = crate::rpc::call_at(port, "session", json!({"action": "set"}));
    assert!(err, "{message}");
    assert!(message.to_string().contains("id"), "{message}");

    // `logs` garde la fin de la sortie du dernier tour (l'enveloppe du faux CLI).
    let logs = ok_at(port, "logs", json!({}));
    assert!(logs["output"].as_str().unwrap().contains("echo:bonjour"), "{logs}");
    assert!(logs["bytes"].as_u64().unwrap() > 0);

    // `config` rend les fichiers de l'agent, sans secret en clair.
    let config = ok_at(port, "config", json!({}));
    let files = config["files"].as_array().expect("files");
    assert!(files.iter().any(|f| f["name"].as_str().unwrap().contains("settings")));
    assert!(!config.to_string().contains("primary-token"), "{config}");
}

/// `logs` tronque proprement, sans casser un caractère UTF-8.
#[test]
fn logs_tail_is_bounded() {
    let h = start(&[]);
    ok(&h, "create_agent", json!({"name": "carl", "prompt": "p"}));
    let info = ok(&h, "agent_start", json!({"name": "carl"}));
    let port = port_of(info["url"].as_str().unwrap());
    ok_at(port, "ask", json!({"prompt": "ééééé"}));
    let logs: Value = ok_at(port, "logs", json!({"tail": 16}));
    let output = logs["output"].as_str().unwrap();
    assert!(output.len() <= 16, "{logs}");
    assert!(std::str::from_utf8(output.as_bytes()).is_ok());
}

/// Rotation : au-delà de `SWARM_HISTORY_MAX`, le journal tourne (une génération gardée)
/// et `history` suit à travers les générations.
#[test]
fn the_journal_rotates_at_the_configured_size() {
    let h = start(&[("SWARM_HISTORY_MAX", "1024")]);
    ok(&h, "create_agent", json!({"name": "dora", "prompt": "p"}));
    let info = ok(&h, "agent_start", json!({"name": "dora"}));
    let port = port_of(info["url"].as_str().unwrap());
    // Une ligne de tour dépasse alors 1 Kio : chaque ajout fait tourner le fichier.
    let long = "x".repeat(1000);
    for i in 0..4u32 {
        ok_at(port, "ask", json!({"prompt": format!("{long} {i}")}));
    }
    let current = h.ws.join("bridge").join("history").join("claude-dora.jsonl");
    let rotated = h.ws.join("bridge").join("history").join("claude-dora.jsonl.1");
    assert!(rotated.is_file(), "aucune génération tournée après 4 tours");
    // Une seule génération conservée : l'avant-dernier tour en `.1`, le dernier dans le courant.
    assert_eq!(std::fs::read_to_string(&rotated).unwrap().lines().count(), 1);
    assert_eq!(std::fs::read_to_string(&current).unwrap().lines().count(), 1);
    // `history` lit les deux générations : les deux derniers tours, dans l'ordre.
    let hist = ok_at(port, "history", json!({"limit": 50}));
    let entries = hist["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2, "{hist}");
    assert_eq!(entries[0]["prompt"].as_str().unwrap(), format!("{long} 2"));
    assert_eq!(entries[1]["prompt"].as_str().unwrap(), format!("{long} 3"));
}
