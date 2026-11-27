//! Un agent résident hébergé : son contexte, son serveur, son fil de travail.

use super::ctx::Ctx;
use super::info::HostInfo;
use litemcp::http::ServerHandle;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Attente maximale de la fin du fil de travail à l'arrêt.
const JOIN_GRACE: Duration = Duration::from_secs(10);

pub struct AgentHost {
    ctx: Arc<Ctx>,
    server: ServerHandle,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl AgentHost {
    pub fn new(ctx: Arc<Ctx>, server: ServerHandle, worker: JoinHandle<()>) -> Self {
        AgentHost { ctx, server, worker: Mutex::new(Some(worker)) }
    }

    pub fn ctx(&self) -> &Arc<Ctx> {
        &self.ctx
    }

    pub fn info(&self) -> HostInfo {
        HostInfo {
            agent: self.ctx.agent.clone(),
            runner: self.ctx.runner.as_str().to_string(),
            name: self.ctx.name(),
            url: self.ctx.url().unwrap_or_default().to_string(),
            instance: self.ctx.instance.clone(),
        }
    }

    /// Arrête l'agent : tour en cours tué, file vidée, serveur fermé (annonce retirée),
    /// bail d'hébergement libéré par la fin du fil de travail.
    pub fn shutdown(&self) {
        self.ctx.begin_stop();
        self.server.shutdown();
        let worker = self.worker.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(handle) = worker {
            let deadline = Instant::now() + JOIN_GRACE;
            while !handle.is_finished() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
            if handle.is_finished() {
                let _ = handle.join();
            }
        }
    }
}
