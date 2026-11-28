//! Archétypes de l'essaim (PLAN §4.3) : tour éphémère avec persona de modèle,
//! relecture `verify` par l'archétype verifier.

use crate::harness::start;
use crate::rpc::{call, ok};
use serde_json::json;
use std::fs;

/// Pose un modèle `verifier` de surcharge dans le workspace du harnais.
fn write_verifier_template(ws: &std::path::Path) {
    let dir = ws.join("templates");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("verifier.json"), r#"{"name":"verifier","description":"relecture"}"#)
        .unwrap();
    fs::write(dir.join("verifier.md"), "Tu es le verifier de test.").unwrap();
}

#[test]
fn archetype_run_is_an_ephemeral_turn_with_the_template_persona() {
    let h = start(&[]);
    let r = ok(
        &h,
        "run_agent",
        json!({"runner": "claude", "archetype": "coder", "prompt": "va au travail"}),
    );
    let text = r["result"].as_str().unwrap();
    // Le faux CLI renvoie en écho le prompt reçu : persona du modèle en tête, demande ensuite.
    assert!(text.contains("agent codeur"), "{text}");
    assert!(text.contains("va au travail"), "{text}");
    // Tour éphémère : aucun agent nommé n'a été créé ni aucune session gardée.
    assert!(r["agentName"].is_null(), "{}", r["agentName"]);
}

#[test]
fn archetype_and_named_agent_are_rejected_together() {
    let h = start(&[]);
    let (err, v) = call(
        &h,
        "run_agent",
        json!({"runner": "claude", "archetype": "coder", "agentName": "x", "prompt": "p"}),
    );
    assert!(err, "{v}");
    assert!(v.to_string().contains("archétype"), "{v}");
}

#[test]
fn verify_chains_a_verifier_review_field() {
    let h = start(&[]);
    write_verifier_template(&h.ws);
    let r = ok(
        &h,
        "run_agent",
        json!({"runner": "claude", "prompt": "calcule la somme", "verify": true}),
    );
    // Résultat d'origine intact, relecture à côté : le tour verifier a reçu sa demande.
    assert!(r["result"].as_str().unwrap().contains("calcule la somme"));
    let verified = r["verified"].as_str().unwrap();
    assert!(verified.contains("Relis le résultat"), "{verified}");
    assert!(verified.contains("calcule la somme"), "{verified}");
}

#[test]
fn a_broken_verifier_template_is_annotated_not_fatal() {
    let h = start(&[]);
    // Le modèle intégré `verifier` existe : ce qui casse ici, c'est la surcharge du
    // workspace, illisible — la relecture échoue, le résultat du tour reste intact.
    let dir = h.ws.join("templates");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("verifier.json"), "{pas du json").unwrap();
    let r = ok(&h, "run_agent", json!({"runner": "claude", "prompt": "seul", "verify": true}));
    assert!(r["result"].as_str().unwrap().contains("seul"));
    assert!(r["verified"].is_null(), "{}", r["verified"]);
    assert!(r["verifiedError"].as_str().unwrap().contains("vérification impossible"));
}
