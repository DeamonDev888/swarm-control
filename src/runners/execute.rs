//! Cycle de vie d'un run : reprise, lancement, session, clôture du registre.

use super::{RunRequest, RunResult, antigravity, claude, guard, hermes, session};
use crate::agents::Runner;
use crate::registry::{LiveRun, Outcome, Status};
use crate::sessions;
use litemcp::cancel::CancelToken;
use std::sync::Arc;

/// Exécute le run réservé par `prepare` et clôt son entrée de registre.
///
/// Une panique est rattrapée : le run se termine en échec et libère son bail.
pub fn execute(req: &RunRequest, live: &Arc<LiveRun>, cancel: Option<&CancelToken>) -> RunResult {
    match guard::catch(|| cycle(req, live, cancel)) {
        Ok(result) => result,
        Err(panic) => {
            let error = format!("INTERNAL_ERROR: panique pendant le run ({panic})");
            live.finish(
                Status::Failed,
                None,
                Outcome { result: String::new(), error: Some(error.clone()) },
            );
            RunResult::err(error)
        }
    }
}

fn cycle(req: &RunRequest, live: &Arc<LiveRun>, cancel: Option<&CancelToken>) -> RunResult {
    let resume = resume_id(req);
    crate::debug!(
        "run {} ({}): {}",
        who(req),
        req.runner.as_str(),
        match &resume {
            Some(id) => format!("reprise de la session {id}"),
            None => "session neuve".to_string(),
        }
    );
    let mut result = attempt(req, live, resume.as_deref(), cancel);
    if resume.is_some() && session::is_dead(&result) && !live.kill_requested() {
        forget_session(req);
        result = attempt(req, live, None, cancel);
    }
    persist_session(req, live, &result);
    live.finish(
        status_of(live, &result),
        None,
        Outcome { result: result.result.clone(), error: result.error.clone() },
    );
    result
}

fn who(req: &RunRequest) -> &str {
    req.agent.as_deref().unwrap_or("(sans agent)")
}

/// Session à reprendre : explicite, sinon la dernière de l'agent (`autoResume`).
fn resume_id(req: &RunRequest) -> Option<String> {
    if req.session_id.is_some() {
        return req.session_id.clone();
    }
    let agent = req.agent.as_deref().filter(|_| req.auto_resume)?;
    sessions::get(&req.workspace, req.runner.as_str(), agent)
}

fn attempt(
    req: &RunRequest,
    live: &Arc<LiveRun>,
    resume: Option<&str>,
    cancel: Option<&CancelToken>,
) -> RunResult {
    let started = std::time::Instant::now();
    crate::debug!(
        "run {} ({}): tentative avec {}",
        who(req),
        req.runner.as_str(),
        resume.unwrap_or("une nouvelle session")
    );
    let result = match req.runner {
        Runner::Claude => claude::run(req, live, resume, cancel),
        Runner::Hermes => hermes::run(req, live, resume, cancel),
        Runner::Antigravity => antigravity::run(req, live, resume, cancel),
    };
    match &result.error {
        Some(error) => crate::debug!(
            "run {} ({}): échec en {} ms · {error}",
            who(req),
            req.runner.as_str(),
            started.elapsed().as_millis()
        ),
        None => crate::debug!(
            "run {} ({}): ok en {} ms",
            who(req),
            req.runner.as_str(),
            started.elapsed().as_millis()
        ),
    }
    result
}

fn forget_session(req: &RunRequest) {
    if let Some(agent) = &req.agent {
        crate::debug!("run {agent} ({}): session morte oubliée", req.runner.as_str());
        sessions::delete(&req.workspace, req.runner.as_str(), agent);
    }
}

/// La session rendue devient la reprise du prochain appel.
fn persist_session(req: &RunRequest, live: &LiveRun, result: &RunResult) {
    let (Some(agent), Some(sid)) = (&req.agent, &result.session_id) else {
        return;
    };
    if let Err(e) = sessions::save(&req.workspace, req.runner.as_str(), agent, sid) {
        eprintln!("[swarm-control] sauvegarde de session impossible: {e}");
    }
    live.set_session(sid);
}

fn status_of(live: &LiveRun, result: &RunResult) -> Status {
    if live.kill_requested() {
        Status::Killed
    } else if result.error.is_some() {
        Status::Failed
    } else {
        Status::Done
    }
}
