//! Erreurs de fournisseur Hermes : code 1 + message sur stdout.

use crate::harness::start;
use crate::rpc::{call, ok};
use serde_json::json;

#[test]
fn hermes_provider_errors_are_errors_with_hints() {
    let h = start(&[]);
    ok(&h, "create_agent", json!({"name": "hugo", "runner": "hermes", "prompt": "p"}));

    // Code de sortie 1 + texte sur stdout (cas réel : crédits épuisés) => erreur, pas succès.
    let (err, msg) = call(
        &h,
        "run_agent",
        json!({"runner": "hermes", "agentName": "hugo", "prompt": "PROVIDERFAIL"}),
    );
    let msg = msg.to_string();
    assert!(
        err && msg.contains("EXIT_CODE_1") && msg.contains("Billing or credits exhausted"),
        "{msg}"
    );
    assert!(msg.contains("crédits du fournisseur épuisés"), "indice manquant: {msg}");

    // Identifiants absents : message d'aide actionnable.
    let (err, msg) =
        call(&h, "run_agent", json!({"runner": "hermes", "agentName": "hugo", "prompt": "NOCRED"}));
    let msg = msg.to_string();
    assert!(err && msg.contains("hermes -p hugo auth add"), "{msg}");
}
