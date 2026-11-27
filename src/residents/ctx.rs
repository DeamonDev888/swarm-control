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
