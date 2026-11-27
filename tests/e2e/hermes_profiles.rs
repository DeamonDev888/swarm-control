//! Noms de profil, credentials par profil, `cloneFrom`, suppression.

use crate::harness::start;
use crate::rpc::{call, ok};
use serde_json::json;

#[test]
fn hermes_names_credentials_and_clone() {
    let h = start(&[]);
    // Règle Hermes : minuscules uniquement.
    let (err, msg) =
        call(&h, "create_agent", json!({"name": "Hera", "runner": "hermes", "prompt": "p"}));
    assert!(err && msg.to_string().contains("minuscules"), "{msg}");

    // Profil sans credentials : l'agent est créé mais l'avertissement est explicite.
    let created = ok(
        &h,
        "create_agent",
        json!({"name": "nocreds", "runner": "hermes", "prompt": "p", "model": "glm-5.3-flash"}),
    );
    let warn = created["warnings"].to_string();
    assert!(warn.contains("PAR PROFIL") && warn.contains("auth add zai"), "{warn}");

    // cloneFrom : source inexistante refusée ; source avec .env => pas d'avertissement.
    let (err, _) = call(
        &h,
        "create_agent",
        json!({"name": "c1", "runner": "hermes", "prompt": "p", "cloneFrom": "ghost"}),
    );
    assert!(err);
    let src = h.hermes_home.join("profiles/source");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("config.yaml"), "model:\n  default: x\n").unwrap();
    std::fs::write(src.join(".env"), "ZAI_API_KEY=test\n").unwrap();
    let cloned = ok(
        &h,
        "create_agent",
        json!({"name": "c2", "runner": "hermes", "prompt": "p", "cloneFrom": "source", "model": "glm-5.3-flash"}),
    );
    assert!(cloned.get("warnings").is_none(), "{cloned}");
    assert!(h.hermes_home.join("profiles/c2/.env").is_file());

    // delete : pas de fausse erreur quand le profil a bien disparu.
    let deleted = ok(&h, "delete_agent", json!({"name": "c2"}));
    assert!(deleted.get("errors").is_none(), "{deleted}");
    assert!(!h.hermes_home.join("profiles/c2").exists());
}
