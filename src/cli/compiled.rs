//! Préparation du démarrage d'un **binaire compilé** (PLAN §4.8) : la définition du
//! workspace fait foi ; l'embarquée ne sert qu'à recréer un dossier d'agent manquant.
//!
//! [`prepare`] est appelé par `cli::serve_compiled` avant le service résident ordinaire
//! (`serve --agent <agent>`), jamais pour un autre agent que celui gravé dans le binaire.

use crate::agents::{self, Runner, templates};
use serde_json::Value;
use std::path::Path;

/// Définition embarquée d'un agent (`agent.json` du crate généré) : de quoi le recréer.
#[derive(Debug, PartialEq, Eq)]
pub struct Embedded {
    pub name: String,
    pub class: String,
    pub runner: Runner,
    pub prompt: String,
    pub model: Option<String>,
    pub template: Option<String>,
}

/// Lit et valide la définition embarquée ; le nom doit valoir `agent` (gravé).
pub fn parse_embedded(agent: &str, definition: &str) -> Result<Embedded, String> {
    let doc: Value = serde_json::from_str(definition)
        .map_err(|e| format!("définition embarquée de '{agent}' illisible : {e}"))?;
    let get = |key: &str| doc.get(key).and_then(Value::as_str).map(String::from);
    let name =
        get("name").ok_or_else(|| format!("définition embarquée sans nom pour '{agent}'"))?;
    if name != agent {
        return Err(format!(
            "définition embarquée incohérente : elle nomme '{name}', le binaire est compilé pour '{agent}'"
        ));
    }
    let runner = get("runner").and_then(|r| Runner::parse(&r)).unwrap_or(Runner::Claude);
    Ok(Embedded {
        name,
        class: get("class").unwrap_or_else(|| "custom".into()),
        runner,
        prompt: get("prompt")
            .ok_or_else(|| format!("définition embarquée de '{agent}' sans prompt"))?,
        model: get("model"),
        template: get("template"),
    })
}

/// Prépare le workspace pour l'agent gravé :
/// - l'agent existe pour au moins un runner → la définition du workspace fait foi, on
///   ne touche à rien (sauf la classe, estampillée si absente) ;
/// - l'agent n'existe nulle part → il est **recréé** depuis la définition embarquée par
///   `agents::create` (chemin de `create_agent`), puis estampillé (modèle et classe).
///
/// `args` : la ligne de commande (sans `argv[0]`) ; `--workspace DIR` y est honoré, sinon
/// la config globale du process fait foi.
pub fn prepare(agent: &str, class: &str, definition: &str, args: &[String]) -> Result<(), String> {
    let embedded = parse_embedded(agent, definition)?;
    let cfg = crate::config::init(workspace_of(args));
    prepare_in(&cfg.workspace, agent, class, &embedded)
}

/// Comme [`prepare`], sur un workspace donné. La recréation passe par `agents::create`,
/// qui lit modèle et jeton via la config globale : `config::init` doit avoir été appelé,
/// comme `prepare` le fait avant.
pub fn prepare_in(ws: &Path, agent: &str, class: &str, embedded: &Embedded) -> Result<(), String> {
    agents::validate_name(agent)?;
    let class = agents::logo::CLASSES.iter().find(|c| **c == class).copied().unwrap_or("custom");
    if embedded.class != class {
        return Err(format!(
            "classe incohérente : binaire compilé pour '{class}', définition embarquée '{}'",
            embedded.class
        ));
    }
    let runner = match agents::peek_runner(ws, agent) {
        Some(runner) => runner,
        None => recreate(ws, agent, embedded)?,
    };
    stamp(ws, agent, runner, embedded, class)
}

/// `--workspace DIR` (ou `-w DIR`) de la ligne de commande, sinon `None` (config globale).
fn workspace_of(args: &[String]) -> Option<std::path::PathBuf> {
    args.windows(2)
        .find(|w| w[0] == "--workspace" || w[0] == "-w")
        .map(|w| std::path::PathBuf::from(&w[1]))
}

/// Recrée l'agent depuis la définition embarquée : le chemin ordinaire de création
/// (prompt, settings, MCP du workspace). Les identifiants ne sont pas embarqués : ils
/// viennent de l'environnement et des `.env`, comme à toute création.
fn recreate(ws: &Path, agent: &str, embedded: &Embedded) -> Result<Runner, String> {
    agents::create(
        ws,
        &agents::CreateArgs {
            name: agent,
            prompt: &embedded.prompt,
            runner: embedded.runner,
            model: embedded.model.as_deref(),
            copy_env_from: None,
            clone_from: None,
        },
    )
    .map_err(|e| format!("agent '{agent}' absent du workspace et recréation impossible : {e}"))?;
    Ok(embedded.runner)
}

/// Estampille la classe gravée (et le modèle d'origine) dans la définition du workspace,
/// **seulement** si elle n'en a pas déjà une : la définition du workspace fait foi.
fn stamp(
    ws: &Path,
    agent: &str,
    runner: Runner,
    embedded: &Embedded,
    class: &str,
) -> Result<(), String> {
    if templates::class_of(ws, agent, runner).is_some() {
        return Ok(());
    }
    let template = templates::template_of(ws, agent, runner).or_else(|| embedded.template.clone());
    let Some(template) = template else {
        // Sans modèle d'origine : seule la classe est posée (via le même chemin de stamping).
        return templates::stamp_with_port(ws, agent, runner, "", Some(class), None);
    };
    templates::stamp_with_port(ws, agent, runner, &template, Some(class), None)
}

#[cfg(test)]
mod tests {
    use super::{parse_embedded, prepare_in, workspace_of};
    use crate::agents::{self, Runner};
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    static SEQ: AtomicU32 = AtomicU32::new(0);

    /// Workspace temporaire unique, supprimé à la fin du test.
    struct TempWs(PathBuf);

    impl TempWs {
        fn new(tag: &str) -> Self {
            let n = SEQ.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir()
                .join(format!("swarm-compiled-{tag}-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            TempWs(dir)
        }
    }

    impl Drop for TempWs {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn definition(agent: &str, class: &str) -> String {
        json!({ "name": agent, "class": class, "runner": "claude", "prompt": format!("Tu es {agent}.") })
            .to_string()
    }

    #[test]
    fn la_definition_embarquee_doit_nommer_l_agent_grave() {
        let ok = parse_embedded("alice", &definition("alice", "discord")).unwrap();
        assert_eq!(
            (ok.name.as_str(), ok.class.as_str(), ok.runner),
            ("alice", "discord", Runner::Claude)
        );
        let err = parse_embedded("alice", &definition("bob", "discord")).unwrap_err();
        assert!(err.contains("'bob'") && err.contains("'alice'"), "{err}");
        assert!(parse_embedded("alice", "pas du json").is_err());
        let err = parse_embedded("alice", r#"{"name":"alice"}"#).unwrap_err();
        assert!(err.contains("prompt"), "{err}");
        // Classe absente : repli `custom` ; runner inconnu : claude.
        let minimal =
            parse_embedded("alice", r#"{"name":"alice","prompt":"p","runner":"x"}"#).unwrap();
        assert_eq!((minimal.class.as_str(), minimal.runner), ("custom", Runner::Claude));
    }

    #[test]
    fn workspace_est_lu_sur_la_ligne_de_commande() {
        let args = ["serve".to_string(), "--workspace".to_string(), "/ws".to_string()];
        assert_eq!(workspace_of(&args), Some(PathBuf::from("/ws")));
        let args = ["-w".to_string(), "ws".to_string()];
        assert_eq!(workspace_of(&args), Some(PathBuf::from("ws")));
        assert_eq!(workspace_of(&["--workspace".to_string()]), None);
        assert_eq!(workspace_of(&[]), None);
    }

    #[test]
    fn un_agent_absent_est_recree_depuis_la_definition_embarquee() {
        let ws = TempWs::new("recree");
        // Comme `prepare` avant `prepare_in` : la recréation passe par `agents::create`,
        // qui lit modèle et jeton via la config globale (`config::get`, qui panique sans
        // `init`). Initialisation idempotente, figée pour le process (même convention que
        // les tests de `runners::claude`).
        crate::config::init(Some(ws.0.clone()));
        let embedded = parse_embedded("alice", &definition("alice", "custom")).unwrap();
        // La création Claude ne lance rien ; il lui faut seulement un jeton (env système
        // ou `.env` chargés — posé par le harnais e2e, absent d'un `cargo test --lib` nu) :
        // l'issue varie avec l'environnement, pas le contrat.
        match super::recreate(&ws.0, "alice", &embedded) {
            Err(error) => {
                assert!(error.contains("recréation impossible"), "{error}");
                assert!(
                    error.contains("MISSING_AUTH_TOKEN"),
                    "seul un jeton absent refuse la création : {error}"
                );
                assert!(
                    !agents::exists_for(&ws.0, "alice", Runner::Claude),
                    "rien n'est créé à moitié"
                );
            }
            Ok(runner) => {
                assert_eq!(runner, Runner::Claude);
                assert!(
                    agents::exists_for(&ws.0, "alice", Runner::Claude),
                    "créé quand l'environnement porte un jeton"
                );
            }
        }
        // Un agent Hermes sans CLI hermes : erreur claire, jamais de démarrage en silence.
        // Avec un CLI installé, `recreate` lancerait le vrai programme (lent, effets de bord
        // sur le home natif) : cette branche n'est vérifiée que sans CLI résoluble.
        if !crate::binaries::hermes_binary().is_file() {
            let hermes =
                parse_embedded("bob", r#"{"name":"bob","prompt":"p","runner":"hermes"}"#).unwrap();
            let err = super::recreate(&ws.0, "bob", &hermes).unwrap_err();
            assert!(err.contains("'bob'"), "{err}");
        }
    }

    #[test]
    fn la_classe_gravee_est_estempillee_seulement_si_absente() {
        let ws = TempWs::new("classe");
        let embedded = parse_embedded("alice", &definition("alice", "discord")).unwrap();
        // Définition du workspace sans classe : la classe gravée est posée.
        let path = crate::agents::claude_settings_path(&ws.0, "alice");
        crate::store::write_json_atomic(&path, &json!({ "env": {} })).unwrap();
        super::stamp(&ws.0, "alice", Runner::Claude, &embedded, "discord").unwrap();
        assert_eq!(agents::class_of(&ws.0, "alice", Runner::Claude).as_deref(), Some("discord"));
        // La définition du workspace fait foi : sa classe n'est jamais écrasée.
        super::stamp(&ws.0, "alice", Runner::Claude, &embedded, "coder").unwrap();
        assert_eq!(agents::class_of(&ws.0, "alice", Runner::Claude).as_deref(), Some("discord"));
        // Le modèle d'origine, quand la définition embarquée en a un, est conservé au stamping.
        let path = crate::agents::claude_settings_path(&ws.0, "bob");
        crate::store::write_json_atomic(&path, &json!({ "env": {} })).unwrap();
        let with_template =
            parse_embedded("bob", r#"{"name":"bob","prompt":"p","template":"discord"}"#).unwrap();
        super::stamp(&ws.0, "bob", Runner::Claude, &with_template, "discord").unwrap();
        assert_eq!(agents::class_of(&ws.0, "bob", Runner::Claude).as_deref(), Some("discord"));
        assert_eq!(
            crate::agents::templates::template_of(&ws.0, "bob", Runner::Claude).as_deref(),
            Some("discord")
        );
    }

    #[test]
    fn prepare_refuse_une_classe_incoherente_avant_tout_acces_au_disque() {
        let ws = TempWs::new("incoherente");
        // La classe gravée et celle de la définition embarquée doivent coïncider.
        let embedded = parse_embedded("alice", &definition("alice", "discord")).unwrap();
        let err = prepare_in(&ws.0, "alice", "coder", &embedded).unwrap_err();
        assert!(err.contains("classe incohérente"), "{err}");
        assert!(!ws.0.join("agents").exists(), "rien n'est écrit avant le contrôle");
        // Un nom invalide est refusé lui aussi avant tout chemin.
        let embedded = parse_embedded("a", &definition("a", "custom")).unwrap();
        assert!(prepare_in(&ws.0, "../x", "custom", &embedded).is_err());
    }
}
