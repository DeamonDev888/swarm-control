//! Arguments d'un run, communs à `run_agent` et `run_agents_parallel`.
//!
//! Porte aussi l'archétype du tour (PLAN §4.3) : un archétype = la persona d'un
//! modèle (`assets/templates/` ou surcharge `<workspace>/templates/`), seule et
//! sans état. La persona est préfixée au prompt ; l'archétype courant voyage en
//! variable de fil (`CURRENT_ARCHETYPE`) jusqu'au runner Claude, qui en déduit
//! ses permissions (explorer / verifier restreints à la lecture seule).

use super::errors::{invalid, parse_runner};
use crate::agents::{Runner, templates};
use crate::config;
use crate::routing::Forwarded;
use crate::runners::{self, RunRequest, RunResult};
use litemcp::prelude::ToolError;
use serde::Deserialize;
use serde_json::{Value, json};
use std::cell::RefCell;
use std::path::Path;
use std::time::Duration;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunArgs {
    runner: String,
    prompt: String,
    agent_name: Option<String>,
    session_id: Option<String>,
    auto_resume: Option<bool>,
    model: Option<String>,
    provider: Option<String>,
    path: Option<String>,
    config: Option<String>,
    /// Archétype du tour : persona d'un modèle (ex. coder), tour éphémère sans état.
    pub archetype: Option<String>,
    /// Faire relire le résultat par l'archétype `verifier` (champ `verified`).
    #[serde(default)]
    pub verify: bool,
    #[serde(default)]
    pub background: bool,
    pub task_id: Option<String>,
}

impl RunArgs {
    /// Prompt original, sans la persona d'archétype éventuellement préfixée :
    /// c'est lui que la relecture du verifier reçoit comme demande.
    pub fn prompt_text(&self) -> &str {
        &self.prompt
    }

    pub fn request(&self) -> Result<RunRequest, ToolError> {
        let workspace = config::workspace_for(self.config.as_deref());
        let mut prompt = self.prompt.clone();
        let archetype = self.archetype.as_deref().map(str::trim).filter(|s| !s.is_empty());
        if let Some(name) = archetype {
            // Tour éphémère, sans état : pas d'agent nommé ni de session forcée.
            if self.agent_name.is_some() || self.session_id.is_some() {
                return Err(invalid(
                    "archétype : tour éphémère sans état — pas d'agentName ni de sessionId",
                ));
            }
            prompt = prefix_persona(&workspace, name, prompt)?;
        }
        Ok(RunRequest {
            runner: parse_runner(&self.runner)?,
            prompt,
            agent: self.agent_name.clone(),
            session_id: self.session_id.clone(),
            // Agent nommé = agent persistant : on reprend sa session sauf demande contraire.
            // Sans agentName : agent éphémère — un tour, aucune session relue ni gardée.
            auto_resume: self.auto_resume.unwrap_or(self.agent_name.is_some()),
            model: self.model.clone(),
            provider: self.provider.clone(),
            cwd: self.path.clone(),
            workspace,
        })
    }
}

/// Préfixe la persona de l'archétype au prompt (même forme qu'une persona trop
/// longue côté `claude/args` : le modèle la lit en tête du premier tour).
fn prefix_persona(ws: &Path, name: &str, prompt: String) -> Result<String, ToolError> {
    let persona = templates::load(ws, name).map_err(invalid)?.prompt;
    Ok(if persona.trim().is_empty() { prompt } else { format!("{persona}\n\n---\n\n{prompt}") })
}

thread_local! {
    /// Archétype du tour exécuté sur CE fil (tour `run_agent`, tâche du lot
    /// parallèle, ou tour verifier d'une relecture). Posé par les outils juste
    /// avant `runners::execute`, lu par le runner Claude pour ses permissions.
    static CURRENT_ARCHETYPE: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Pose (ou retire) l'archétype du tour sur le fil courant.
pub fn set_current_archetype(name: Option<&str>) {
    CURRENT_ARCHETYPE.with_borrow_mut(|slot| *slot = name.map(String::from));
}

/// Archétype du tour en cours sur ce fil, s'il y en a un.
pub fn current_archetype() -> Option<String> {
    CURRENT_ARCHETYPE.with_borrow(Clone::clone)
}

/// Exécute `run` avec `name` posé comme archétype courant, puis le retire.
/// Chaque appel repose la valeur : un appel sans archétype efface le précédent.
pub fn with_archetype<R>(name: Option<&str>, run: impl FnOnce() -> R) -> R {
    set_current_archetype(name);
    let out = run();
    set_current_archetype(None);
    out
}

/// Tour verifier : relit `result` (réponse à `prompt`) sur le workspace `ws`, puis
/// rend le texte de la relecture. Toujours un tour Claude éphémère, en lecture
/// seule (PLAN §4.3). `Err` si le modèle `verifier` manque ou si le tour échoue :
/// le résultat d'origine reste alors intact, annoté par l'appelant.
pub fn run_verifier(ws: &Path, prompt: &str, result: &str) -> Result<String, String> {
    let persona = templates::load(ws, "verifier")
        .map_err(|e| format!("vérification impossible : {e}"))?
        .prompt;
    let req = RunRequest {
        runner: Runner::Claude,
        prompt: format!(
            "{persona}\n\n---\n\nRelis le résultat d'un tour de l'essaim et tranche : \
             réponds « OK » s'il répond à la demande, sinon « A_REPRENDRE : <motif> ».\n\n\
             --- DEMANDE ---\n{prompt}\n\n--- RÉSULTAT À RELIRE ---\n{result}"
        ),
        agent: None,
        session_id: None,
        auto_resume: false,
        model: None,
        provider: None,
        cwd: None,
        workspace: ws.to_path_buf(),
    };
    let live = runners::prepare(&req).map_err(|r| r.error.unwrap_or_default())?;
    let out = with_archetype(Some("verifier"), || runners::execute(&req, &live, None));
    match out.error {
        Some(e) => Err(e),
        None => Ok(out.result),
    }
}

/// Annote la sortie d'un tour avec la relecture du verifier : le résultat d'origine
/// reste en `result`, la relecture arrive en `verified` (`null` + `verifiedError`
/// si la relecture elle-même a échoué).
pub fn annotate_verified(out: &mut Value, review: Result<String, String>) {
    match review {
        Ok(text) => out["verified"] = json!(text),
        Err(e) => {
            out["verified"] = Value::Null;
            out["verifiedError"] = json!(e);
        }
    }
}

/// Résultat d'un run transmis à un hôte : même forme, plus `routedTo`.
pub fn routed_json(req: &RunRequest, routed: &Forwarded, duration: Duration) -> Value {
    let mut v = result_json(req, &routed.result, duration);
    v["routedTo"] = json!({ "instance": routed.host.instance, "url": routed.host.url });
    v
}

/// Résultat d'un run au format JSON de l'outil.
pub fn result_json(req: &RunRequest, result: &RunResult, duration: Duration) -> Value {
    let mut v = json!({
        "runner": req.runner.as_str(),
        "agentName": req.agent,
        "result": result.result,
        "sessionId": result.session_id,
        "durationMs": duration.as_millis() as u64,
    });
    if let Some(m) = &result.model {
        v["model"] = json!(m);
    }
    if let Some(e) = &result.error {
        v["error"] = json!(e);
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
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
                .join(format!("swarm-runargs-{tag}-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(dir.join("templates")).unwrap();
            TempWs(dir)
        }
    }

    impl Drop for TempWs {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn args(archetype: Option<&str>) -> RunArgs {
        RunArgs {
            runner: "claude".into(),
            prompt: "fais le tour".into(),
            agent_name: None,
            session_id: None,
            auto_resume: None,
            model: None,
            provider: None,
            path: None,
            // Workspace factice : évite la config globale (non initialisée en test).
            config: Some(".".into()),
            archetype: archetype.map(String::from),
            verify: false,
            background: false,
            task_id: None,
        }
    }

    /// Modèle `ping` de surcharge workspace : persona courte, schéma minimal.
    fn write_ping_template(ws: &TempWs) {
        std::fs::write(
            ws.0.join("templates").join("ping.json"),
            r#"{"name":"ping","description":"modèle de test"}"#,
        )
        .unwrap();
        std::fs::write(ws.0.join("templates").join("ping.md"), "Tu es le pinger de test.").unwrap();
    }

    #[test]
    fn archetype_prefixes_persona_and_stays_ephemeral() {
        let ws = TempWs::new("persona");
        write_ping_template(&ws);
        let mut a = args(Some("ping"));
        a.config = Some(ws.0.display().to_string());
        let req = a.request().unwrap();
        assert!(req.prompt.starts_with("Tu es le pinger de test."), "{}", req.prompt);
        assert!(req.prompt.contains("---\n\nfais le tour"), "{}", req.prompt);
        assert_eq!(req.agent, None);
        assert!(!req.auto_resume);
    }

    #[test]
    fn archetype_uses_the_workspace_override_before_builtins() {
        // Le modèle intégré `coder` existe : la surcharge `ping` doit gagner.
        let ws = TempWs::new("override");
        write_ping_template(&ws);
        let mut a = args(Some("ping"));
        a.config = Some(ws.0.display().to_string());
        let req = a.request().unwrap();
        assert!(req.prompt.contains("pinger de test"), "{}", req.prompt);
    }

    #[test]
    fn unknown_archetype_is_a_clear_invalid_input() {
        let e = args(Some("nimportequoi"))
            .request()
            .err()
            .expect("un archétype inconnu doit être refusé");
        assert!(e.message.contains("nimportequoi"), "{}", e.message);
    }

    #[test]
    fn archetype_rejects_named_agent_and_session() {
        let mut named = args(Some("coder"));
        named.agent_name = Some("bob".into());
        assert!(named.request().is_err());
        let mut forced = args(Some("coder"));
        forced.session_id = Some("s1".into());
        assert!(forced.request().is_err());
    }

    #[test]
    fn blank_archetype_value_is_ignored() {
        let req = args(Some("   ")).request().unwrap();
        assert_eq!(req.prompt, "fais le tour");
        assert_eq!(current_archetype(), None);
    }

    #[test]
    fn archetype_scope_is_set_and_cleared_around_the_run() {
        assert_eq!(current_archetype(), None);
        let seen = with_archetype(Some("explorer"), current_archetype);
        assert_eq!(seen.as_deref(), Some("explorer"));
        assert_eq!(current_archetype(), None);
    }

    /// Sans modèle `verifier` lisible, le tour est refusé **avant** tout lancement de runner
    /// (ici une surcharge de workspace illisible : le modèle intégré n'est pas atteint).
    #[test]
    fn verifier_turn_needs_the_verifier_template() {
        let ws = TempWs::new("verifier");
        std::fs::write(ws.0.join("templates").join("verifier.json"), "{pas du json").unwrap();
        let e = run_verifier(&ws.0, "demande", "résultat").unwrap_err();
        assert!(e.contains("vérification impossible"), "{e}");
    }

    #[test]
    fn verified_annotation_keeps_the_original_result_visible() {
        let mut ok = json!({ "result": "brut" });
        annotate_verified(&mut ok, Ok("OK".into()));
        assert_eq!(ok["result"], "brut");
        assert_eq!(ok["verified"], "OK");
        let mut ko = json!({ "result": "brut" });
        annotate_verified(&mut ko, Err("modèle inconnu 'verifier'".into()));
        assert_eq!(ko["result"], "brut");
        assert!(ko["verified"].is_null());
        assert!(ko["verifiedError"].as_str().unwrap().contains("verifier"));
    }
}
