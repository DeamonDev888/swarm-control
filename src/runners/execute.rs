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
    if retry_fresh(resume.is_some(), &result, live) {
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

/// La session reprise n'existe plus, et l'arrêt n'a pas été demandé : un seul
/// nouvel essai, à neuf.
fn retry_fresh(resumed: bool, result: &RunResult, live: &LiveRun) -> bool {
    resumed && session::is_dead(result) && !live.kill_requested()
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

#[cfg(test)]
mod tests {
    use super::{execute, persist_session, resume_id, retry_fresh, status_of};
    use crate::agents::Runner;
    use crate::registry::{self, Outcome, Status};
    use crate::runners::{RunRequest, RunResult};
    use crate::sessions;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU32, Ordering};

    static SEQ: AtomicU32 = AtomicU32::new(0);

    /// Nom unique dans le process : l'index des runs est global.
    fn nom() -> String {
        format!("cycle-{}-{}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed))
    }

    /// Workspace temporaire unique (pid + rang), supprimé à la fin du test.
    struct TempWs(PathBuf);

    impl TempWs {
        fn new(tag: &str) -> Self {
            let n = SEQ.fetch_add(1, Ordering::Relaxed);
            let dir =
                std::env::temp_dir().join(format!("swarm-cycle-{tag}-{}-{n}", std::process::id()));
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

    /// Une demande Claude sur `ws` : sans `settings_<nom>.json`, le plan échoue avant
    /// toute commande — aucun CLI n'est jamais lancé par ces tests.
    fn requete(ws: &Path, agent: Option<&str>) -> RunRequest {
        RunRequest {
            runner: Runner::Claude,
            prompt: "un tour".into(),
            agent: agent.map(String::from),
            session_id: None,
            auto_resume: true,
            model: None,
            provider: None,
            cwd: None,
            workspace: ws.to_path_buf(),
        }
    }

    #[test]
    fn la_session_a_reprendre_est_celle_demandee_sinon_la_derniere_de_l_agent() {
        let ws = TempWs::new("reprise");
        // Sans agent : jamais de reprise automatique.
        assert_eq!(resume_id(&requete(&ws.0, None)), None);
        // Agent nommé, reprise automatique : rien sur disque au départ.
        let req = requete(&ws.0, Some("alice"));
        assert_eq!(resume_id(&req), None);
        sessions::save(&ws.0, "claude", "alice", "s-ancienne").unwrap();
        assert_eq!(resume_id(&req).as_deref(), Some("s-ancienne"));
        // La session explicite passe avant, même sans reprise automatique.
        let mut explicite = requete(&ws.0, Some("alice"));
        explicite.auto_resume = false;
        explicite.session_id = Some("s-explicite".into());
        assert_eq!(resume_id(&explicite).as_deref(), Some("s-explicite"));
        // Sans reprise automatique, la dernière session n'est pas relue.
        let mut sans = requete(&ws.0, Some("alice"));
        sans.auto_resume = false;
        assert_eq!(resume_id(&sans), None);
        // Un autre agent, ou un autre runner, n'a pas de session.
        assert_eq!(resume_id(&requete(&ws.0, Some("bob"))), None);
        let mut hermes = requete(&ws.0, Some("alice"));
        hermes.runner = Runner::Hermes;
        assert_eq!(resume_id(&hermes), None);
    }

    #[test]
    fn le_statut_final_est_tue_si_l_arret_a_ete_demande_puis_echec_sinon_reussi() {
        let ws = TempWs::new("statut");
        let live = registry::begin("claude", Some(&nom()), &ws.0).unwrap();
        assert_eq!(status_of(&live, &RunResult::default()), Status::Done);
        assert_eq!(status_of(&live, &RunResult::err("boum")), Status::Failed);
        live.request_kill();
        assert_eq!(status_of(&live, &RunResult::err("boum")), Status::Killed);
        assert_eq!(status_of(&live, &RunResult::default()), Status::Killed, "même sans erreur");
        live.finish(Status::Killed, None, Outcome::default());
    }

    #[test]
    fn la_session_rendue_devient_celle_de_l_agent() {
        let ws = TempWs::new("session");
        let agent = nom();
        let live = registry::begin("claude", Some(&agent), &ws.0).unwrap();
        let req = requete(&ws.0, Some(&agent));
        let rendue = RunResult { session_id: Some("s-9".into()), ..Default::default() };
        persist_session(&req, &live, &rendue);
        assert_eq!(sessions::get(&ws.0, "claude", &agent).as_deref(), Some("s-9"));
        assert_eq!(live.snapshot().session_id.as_deref(), Some("s-9"));
        // Sans session rendue, la session existante reste.
        persist_session(&req, &live, &RunResult::default());
        assert_eq!(sessions::get(&ws.0, "claude", &agent).as_deref(), Some("s-9"));
        live.finish(Status::Done, Some(0), Outcome::default());
        // Sans agent (éphémère), rien n'est gardé, même si une session est rendue.
        let anonyme = registry::begin("claude", None, &ws.0).unwrap();
        persist_session(&requete(&ws.0, None), &anonyme, &rendue);
        assert_eq!(anonyme.snapshot().session_id, None);
        assert_eq!(crate::state::list(&ws.0).len(), 1, "seul l'agent nommé a un fichier");
        anonyme.finish(Status::Done, Some(0), Outcome::default());
    }

    #[test]
    fn une_seule_nouvelle_tentative_si_la_session_reprise_est_morte() {
        let ws = TempWs::new("tentative");
        let live = registry::begin("claude", Some(&nom()), &ws.0).unwrap();
        let morte = RunResult::err("EXIT_CODE_1: No conversation found with session ID");
        assert!(retry_fresh(true, &morte, &live));
        assert!(!retry_fresh(false, &morte, &live), "pas de reprise : rien à refaire");
        assert!(!retry_fresh(true, &RunResult::err("TIMEOUT: arrêté après 1s."), &live));
        assert!(!retry_fresh(true, &RunResult::default(), &live), "sans erreur");
        live.request_kill();
        assert!(!retry_fresh(true, &morte, &live), "arrêt demandé : on ne relance pas");
        live.finish(Status::Killed, None, Outcome::default());
    }

    #[test]
    fn un_agent_sans_settings_termine_en_echec_sans_lancer_le_cli() {
        let ws = TempWs::new("execute");
        let agent = nom();
        let live = registry::begin("claude", Some(&agent), &ws.0).unwrap();
        let result = execute(&requete(&ws.0, Some(&agent)), &live, None);
        let error = result.error.expect("échec attendu");
        assert!(error.starts_with("INVALID_AGENT: settings_"), "{error}");
        assert!(error.contains(&format!("settings_{agent}.json")), "{error}");
        assert!(result.result.is_empty() && result.session_id.is_none() && result.raw.is_empty());
        // Le run est clos en échec, avec son issue ; le fichier d'état le dit.
        let snap = live.snapshot();
        assert_eq!(snap.status, Status::Failed);
        assert_eq!(snap.exit_code, None);
        assert_eq!(snap.outcome.map(|o| o.error), Some(Some(error)));
        let (_, entry) = registry::disk_entry(&ws.0, &agent, None).unwrap();
        assert_eq!(entry["status"], "failed");
        // Le bail est libéré : l'agent peut repartir tout de suite.
        std::thread::sleep(std::time::Duration::from_millis(2));
        let suivant = registry::begin("claude", Some(&agent), &ws.0);
        assert!(suivant.is_ok(), "l'agent est libre après son échec");
        suivant.unwrap().finish(Status::Done, Some(0), Outcome::default());
    }

    #[test]
    fn un_arret_demande_avant_le_tour_le_termine_en_tue() {
        let ws = TempWs::new("arrete");
        let agent = nom();
        let live = registry::begin("claude", Some(&agent), &ws.0).unwrap();
        live.request_kill();
        let result = execute(&requete(&ws.0, Some(&agent)), &live, None);
        assert!(result.error.is_some(), "le plan échoue toujours");
        assert_eq!(live.snapshot().status, Status::Killed, "l'arrêt demandé passe avant l'échec");
        let (_, entry) = registry::disk_entry(&ws.0, &agent, None).unwrap();
        assert_eq!(entry["status"], "failed", "sur disque, tué se dit failed");
    }
}
