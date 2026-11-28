//! Modèles d'agents (PLAN §4.1) : outil `list_templates`, `create_agent {template}`,
//! surcharge `<workspace>/templates/` et prérequis vus par `doctor --agent`.

use crate::harness::{command, start};
use crate::rpc::{call, ok, ok_at, port_of};
use serde_json::json;

#[test]
fn list_templates_exposes_the_built_ins() {
    let h = start(&[]);
    let v = ok(&h, "list_templates", json!({}));
    let items = v["templates"].as_array().expect("templates").to_vec();
    // Les 11 modèles intégrés (PLAN §4.1) sont tous listés, en builtin.
    assert!(items.len() >= 11, "{v}");
    for name in [
        "browser",
        "coder",
        "coordinator",
        "custom",
        "discord",
        "explorer",
        "google",
        "monitor",
        "verifier",
        "whatsapp",
        "x",
    ] {
        let t = items
            .iter()
            .find(|t| t["name"] == name)
            .unwrap_or_else(|| panic!("{name} listé : {v}"));
        assert_eq!(t["source"], "builtin", "{name}");
    }
    let coder = items.iter().find(|t| t["name"] == "coder").expect("coder listé");
    assert_eq!(coder["class"], "coder");
    assert_eq!(coder["source"], "builtin");
    assert_eq!(coder["defaults"]["runner"], "claude");
    assert_eq!(coder["requiredVars"], json!([]));
    let discord = items.iter().find(|t| t["name"] == "discord").expect("discord listé");
    assert_eq!(discord["class"], "discord");
    assert_eq!(discord["requiredVars"], json!(["DISCORD_BOT_TOKEN"]));
    assert_eq!(discord["expectedMcpServers"], json!(["serveur_discord"]));
    assert_eq!(discord["defaults"]["executable"], true);
    assert_eq!(discord["defaults"]["bridgePort"], 3011);
    // Schéma strict : un argument inconnu est refusé (-32602).
    let resp = crate::rpc::rpc(
        &h,
        "tools/call",
        Some("list_templates"),
        json!({"name": "list_templates", "arguments": {"nope": 1}}),
    );
    assert_eq!(resp["error"]["code"], -32602, "{resp}");
}

#[test]
fn create_agent_from_a_template_uses_persona_defaults_and_stamps_the_definition() {
    let h = start(&[("DISCORD_BOT_TOKEN", "test-token")]);
    let v = ok(&h, "create_agent", json!({ "name": "canal", "template": "discord" }));
    assert_eq!(v["runner"], "claude", "défaut du modèle");
    assert_eq!(v["template"]["name"], "discord");
    assert_eq!(v["template"]["class"], "discord");
    assert_eq!(v["template"]["source"], "builtin");
    assert_eq!(v["template"]["bridgePort"], 3011);
    // Défaut du modèle appliqué : le crate de l'agent est généré sans executable:true
    // explicite (PLAN §4.8). Le harnais pose SWARM_SKIP_AGENT_BUILD=1 : aucun `cargo
    // build` réel pendant la suite — le binaire manque et le statut le dit.
    let bin = if cfg!(windows) {
        h.ws.join("bin").join("canal.exe")
    } else {
        h.ws.join("bin").join("canal")
    };
    assert_eq!(v["executable"].as_str().unwrap(), bin.display().to_string(), "{v}");
    assert_eq!(v["build"]["status"], "skipped", "compilation suspendue par le harnais : {v}");
    let build = h.ws.join("agents").join("canal").join("build");
    for file in ["Cargo.toml", "build.rs", "agent.json", "src/main.rs"] {
        assert!(build.join(file).is_file(), "{file} attendu dans {}", build.display());
    }
    assert!(!bin.exists(), "sans compilation, pas de binaire : {}", bin.display());
    // Persona = celle du modèle.
    let prompt = std::fs::read_to_string(
        h.ws.join("agents").join("canal").join(".claude").join("agents").join("canal.md"),
    )
    .unwrap();
    assert!(prompt.contains("agent Discord"), "{prompt}");
    // Définition marquée : modèle + classe.
    let settings: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            h.ws.join("agents").join("canal").join(".claude").join("settings_canal.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(settings["template"], "discord");
    assert_eq!(settings["class"], "discord");
    // Port de pont conseillé par le modèle, enregistré dans la définition.
    assert_eq!(settings["bridgePort"], 3011);
}

/// Le champ `class` est exposé partout où l'agent est décrit (TODO §4.1) : `list_agents`
/// et le `status` du résident publient la classe posée par le modèle — jamais déduite du
/// nom (`sniperbot_analyst` de classe `discord` garde le logo Discord de MCP Control).
#[test]
fn status_and_list_agents_expose_the_stamped_class_even_for_a_misleading_name() {
    let h = start(&[("DISCORD_BOT_TOKEN", "test-token")]);
    ok(&h, "create_agent", json!({ "name": "sniperbot_analyst", "template": "discord" }));

    let listed = ok(&h, "list_agents", json!({}));
    assert_eq!(listed["agents"][0]["name"], "sniperbot_analyst");
    assert_eq!(listed["agents"][0]["class"], "discord");

    // Le résident publie la même classe dans son outil `status` (le pont `/status` passe
    // par le même `status_json`).
    let info = ok(&h, "agent_start", json!({ "name": "sniperbot_analyst" }));
    let status = ok_at(port_of(info["url"].as_str().unwrap()), "status", json!({}));
    assert_eq!(status["class"], "discord", "{status}");
}

#[test]
fn a_missing_required_variable_blocks_creation() {
    let h = start(&[]); // pas de DISCORD_BOT_TOKEN
    let (err, msg) = call(&h, "create_agent", json!({ "name": "canal", "template": "discord" }));
    assert!(err, "{msg}");
    assert!(msg.to_string().contains("DISCORD_BOT_TOKEN"), "{msg}");
    assert!(!h.ws.join("agents").join("canal").exists(), "rien ne doit être créé");
    // Ni template ni prompt : refus clair.
    let (err, msg) = call(&h, "create_agent", json!({ "name": "sans-rien" }));
    assert!(err, "{msg}");
    assert!(msg.to_string().contains("prompt ou template requis"), "{msg}");
}

#[test]
fn the_workspace_overrides_a_template() {
    let h = start(&[]);
    let dir = h.ws.join("templates");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("coder.json"),
        r#"{"name":"coder","description":"surcharge e2e","promptFile":"coder.md"}"#,
    )
    .unwrap();
    std::fs::write(dir.join("coder.md"), "persona locale SURCHARGE-E2E").unwrap();
    let v = ok(&h, "create_agent", json!({ "name": "dev", "template": "coder" }));
    assert_eq!(v["template"]["source"], "workspace");
    let prompt = std::fs::read_to_string(
        h.ws.join("agents").join("dev").join(".claude").join("agents").join("dev.md"),
    )
    .unwrap();
    assert!(prompt.contains("SURCHARGE-E2E"), "{prompt}");
    let list = ok(&h, "list_templates", json!({}));
    let coder = list["templates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "coder")
        .expect("coder listé");
    assert_eq!(coder["source"], "workspace");
    assert_eq!(coder["description"], "surcharge e2e");
}

#[test]
fn a_template_works_on_hermes_and_stamps_the_profile() {
    let h = start(&[]);
    let v =
        ok(&h, "create_agent", json!({ "name": "codeh", "template": "coder", "runner": "hermes" }));
    assert_eq!(v["runner"], "hermes");
    let swarm: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(h.hermes_home.join("profiles").join("codeh").join("swarm.json"))
            .expect("swarm.json dans le profil"),
    )
    .unwrap();
    assert_eq!(swarm["template"], "coder");
    assert_eq!(swarm["class"], "coder");
    // coder ne porte pas de port de pont : champ absent, pas null.
    assert!(swarm.get("bridgePort").is_none(), "{swarm}");
    // Un modèle avec port : le swarm.json du profil le reçoit aussi (Hermes).
    ok(&h, "create_agent", json!({ "name": "pont", "template": "custom", "runner": "hermes" }));
    let pont: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(h.hermes_home.join("profiles").join("pont").join("swarm.json"))
            .expect("swarm.json de pont"),
    )
    .unwrap();
    assert_eq!(pont["template"], "custom");
    assert_eq!(pont["bridgePort"], 3013);
}

#[test]
fn cli_templates_lists_the_built_ins() {
    let h = start(&[]);
    let root = h.ws.parent().unwrap().to_path_buf();
    let out = command(&root, &h.ws, &h.hermes_home).args(["templates"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("coder") && text.contains("discord"), "{text}");
    assert!(text.contains("DISCORD_BOT_TOKEN"), "{text}");
    assert!(text.contains("intégré"), "{text}");
    // --json : le même contenu, structuré.
    let out = command(&root, &h.ws, &h.hermes_home).args(["templates", "--json"]).output().unwrap();
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(doc.as_array().unwrap().iter().any(|t| t["name"] == "discord"), "{doc}");
}

#[test]
fn doctor_agent_checks_template_prerequisites() {
    // Instance avec le jeton : toutes les variables du modèle sont là.
    let h = start(&[("DISCORD_BOT_TOKEN", "test-token")]);
    ok(&h, "create_agent", json!({ "name": "d1", "template": "discord" }));
    let root = h.ws.parent().unwrap().to_path_buf();
    let out = command(&root, &h.ws, &h.hermes_home)
        .env("DISCORD_BOT_TOKEN", "test-token")
        .args(["doctor", "--agent", "d1"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("variables requises"), "{text}");
    assert!(text.contains("présente(s)"), "{text}");
    assert!(text.contains("classe : discord"), "{text}");
    // `serveur_discord` n'est pas dans le .mcp.json du harnais : signalé, non bloquant.
    assert!(text.contains("serveur_discord"), "{text}");
    assert!(text.contains("binaire du runner"), "{text}");
    assert!(text.contains("--bridge 3011"), "{text}");
    // Sans --agent : aucune ligne de prérequis de modèle.
    let out = command(&root, &h.ws, &h.hermes_home).args(["doctor"]).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(!text.contains("variables requises"), "{text}");
}

#[test]
fn doctor_reports_missing_variables_and_agents_without_a_template() {
    // L'agent est créé avec le jeton côté instance ; le doctor ci-dessous tourne SANS :
    // la variable manquante se voit, sans faire sortir en erreur.
    let h = start(&[("DISCORD_BOT_TOKEN", "test-token")]);
    ok(&h, "create_agent", json!({ "name": "d2", "template": "discord" }));
    let root = h.ws.parent().unwrap().to_path_buf();
    let out =
        command(&root, &h.ws, &h.hermes_home).args(["doctor", "--agent", "d2"]).output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("DISCORD_BOT_TOKEN"), "{text}");
    // Agent sans modèle : une ligne d'information, aucun prérequis vérifié.
    ok(&h, "create_agent", json!({ "name": "brut", "prompt": "persona brute" }));
    let out =
        command(&root, &h.ws, &h.hermes_home).args(["doctor", "--agent", "brut"]).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("créé sans modèle"), "{text}");
    assert!(!text.contains("variables requises"), "{text}");
    // Agent inconnu : signalé, sortie inchangée (non essentiel).
    let out = command(&root, &h.ws, &h.hermes_home)
        .args(["doctor", "--agent", "ghost"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("introuvable"));
}
