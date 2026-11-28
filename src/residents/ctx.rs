//! Contexte partagé d'un agent résident : identité, file, état du tour en cours.

use super::activity::{Current, Last};
use super::inbox::Inbox;
use super::turn::Turn;
use crate::agents::Runner;
use crate::runners::RunRequest;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

pub struct Ctx {
    pub agent: String,
    pub runner: Runner,
    pub workspace: PathBuf,
    pub instance: String,
    pub(super) url: OnceLock<String>,
    pub(super) inbox: Inbox,
    pub(super) stopping: AtomicBool,
    pub(super) current: Mutex<Option<Current>>,
    pub(super) last: Mutex<Option<Last>>,
    pub(super) turns_done: AtomicU64,
    /// Fin de la sortie brute du dernier tour (outil `logs`, panneau de la TUI).
    pub(super) last_output: Mutex<String>,
    /// Reset demandé pendant un tour : la session sera oubliée à sa fin (sinon le tour la
    /// réécrirait en se terminant).
    pub(super) reset_after_turn: AtomicBool,
}

impl Ctx {
    pub fn new(agent: &str, runner: Runner, workspace: PathBuf, instance: String) -> Self {
        Ctx {
            agent: agent.to_string(),
            runner,
            workspace,
            instance,
            url: OnceLock::new(),
            inbox: Inbox::default(),
            stopping: AtomicBool::new(false),
            current: Mutex::new(None),
            last: Mutex::new(None),
            turns_done: AtomicU64::new(0),
            last_output: Mutex::new(String::new()),
            reset_after_turn: AtomicBool::new(false),
        }
    }

    /// Nom annoncé dans le registre LiteMCP.
    pub fn name(&self) -> String {
        format!("swarm-{}", self.agent)
    }

    pub fn url(&self) -> Option<&str> {
        self.url.get().map(String::as_str)
    }

    pub fn set_url(&self, url: String) {
        let _ = self.url.set(url);
    }

    pub fn stopping(&self) -> bool {
        self.stopping.load(Ordering::SeqCst)
    }

    /// Fin de la sortie du dernier tour (ou du tour en cours), pour `logs` et la TUI.
    pub fn last_output(&self) -> String {
        self.last_output.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub(super) fn set_output(&self, tail: String) {
        *self.last_output.lock().unwrap_or_else(|e| e.into_inner()) = tail;
    }

    /// Met un tour dans la file. Refusé si l'agent s'arrête.
    pub fn submit(&self, turn: Turn) -> Result<(), String> {
        if self.stopping() {
            return Err("STOPPED: l'agent est en cours d'arrêt.".into());
        }
        self.inbox.push(turn);
        Ok(())
    }

    /// La demande de run correspondant à un tour : toujours la session de l'agent reprise.
    pub fn request_for(&self, turn: &Turn) -> RunRequest {
        RunRequest {
            runner: self.runner,
            prompt: turn.prompt.clone(),
            agent: Some(self.agent.clone()),
            session_id: None,
            auto_resume: true,
            model: turn.model.clone(),
            provider: None,
            cwd: None,
            workspace: self.workspace.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Ctx;
    use crate::agents::Runner;
    use crate::runners::RunResult;
    use crate::sessions;
    use serde_json::json;
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    /// Un Ctx sur un workspace temporaire unique (pid + nom du test) : aucun serveur démarré.
    fn ctx(name: &str) -> Ctx {
        let ws = std::env::temp_dir().join(format!("swarm-ctx-{}-{name}", std::process::id()));
        Ctx::new(name, Runner::Claude, ws, "swarm-control-1".into())
    }

    #[test]
    fn le_reset_demande_pendant_un_tour_survit_a_la_fin_du_tour() {
        let ctx = ctx("differe");
        ctx.begin_turn(Some("bob".into()), "run-1");
        // Pendant un tour, le reset oublie la session tout de suite MAIS pose le drapeau :
        // le run en cours réenregistrera sa session en finissant, il faudra la re-oublier.
        assert!(!ctx.reset_session(), "aucune session sur disque au départ");
        assert!(ctx.reset_after_turn.load(Ordering::SeqCst), "un tour tourne : différé");
        // Le tour se termine et enregistre SA session — ce qu'un reset immédiat raterait.
        sessions::save(&ctx.workspace, ctx.runner.as_str(), &ctx.agent, "session-du-tour").unwrap();
        // Ordre du worker (worker.rs) : le drapeau est consommé avant de publier la fin.
        ctx.apply_pending_reset();
        assert_eq!(
            sessions::get(&ctx.workspace, ctx.runner.as_str(), &ctx.agent),
            None,
            "la session du tour est oubliée"
        );
        assert!(!ctx.reset_after_turn.load(Ordering::SeqCst), "consommé une seule fois (swap)");
        ctx.end_turn(&RunResult::err("boum"), Duration::from_millis(42));
        let status = ctx.status_json();
        assert_eq!(status["state"], json!("idle"));
        assert_eq!(status["turnsDone"], json!(1));
        assert_eq!(status["lastTurn"]["error"], json!("boum"));
    }

    #[test]
    fn sans_tour_en_cours_le_reset_est_immediat_sans_drapeau() {
        let ctx = ctx("immediat");
        sessions::save(&ctx.workspace, ctx.runner.as_str(), &ctx.agent, "sess").unwrap();
        assert!(ctx.reset_session(), "la session existait");
        assert!(
            !ctx.reset_after_turn.load(Ordering::SeqCst),
            "personne ne tourne : rien à différer"
        );
        // Un worker qui passerait derrière ne re-oublierait rien : no-op.
        ctx.apply_pending_reset();
        assert_eq!(sessions::get(&ctx.workspace, ctx.runner.as_str(), &ctx.agent), None);
    }
}
