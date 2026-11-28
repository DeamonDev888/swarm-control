//! Un run vivant : identité, drapeau d'arrêt, bail et état partagé.

use super::Snapshot;
use super::state::State;
use crate::owners::Lease;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

pub struct LiveRun {
    pub run_id: String,
    pub runner: &'static str,
    pub agent: Option<String>,
    pub workspace: PathBuf,
    pub started_at: u64,
    kill_requested: AtomicBool,
    state: Mutex<State>,
    /// Bail de l'agent (runs nommés seulement) : libéré par `finish`.
    lease: Mutex<Option<Lease>>,
}

impl LiveRun {
    pub fn new(
        run_id: String,
        runner: &'static str,
        agent: Option<&str>,
        workspace: &Path,
        now: u64,
    ) -> Self {
        LiveRun {
            run_id,
            runner,
            agent: agent.map(String::from),
            workspace: workspace.to_path_buf(),
            started_at: now,
            kill_requested: AtomicBool::new(false),
            state: Mutex::new(State::new(now)),
            lease: Mutex::new(None),
        }
    }

    pub(super) fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn snapshot(&self) -> Snapshot {
        let s = self.state();
        Snapshot {
            run_id: self.run_id.clone(),
            runner: self.runner,
            agent: self.agent.clone(),
            pid: s.pid,
            status: s.status,
            exit_code: s.exit_code,
            session_id: s.session_id.clone(),
            started_at: self.started_at,
            last_output_at: s.last_output_at,
            outcome: s.outcome.clone(),
        }
    }

    pub fn request_kill(&self) {
        self.kill_requested.store(true, Ordering::SeqCst);
    }

    pub fn kill_requested(&self) -> bool {
        self.kill_requested.load(Ordering::SeqCst)
    }

    pub(super) fn finished_at(&self) -> Option<u64> {
        self.state().finished_at
    }

    pub(super) fn attach_lease(&self, lease: Option<Lease>) {
        *self.lease.lock().unwrap_or_else(|e| e.into_inner()) = lease;
    }

    /// Battement de cœur du bail (sans effet une fois le run terminé).
    pub(super) fn heartbeat(&self) {
        if let Some(lease) = self.lease.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            lease.touch();
        }
    }

    /// Libère le bail : l'agent peut être relancé, ici ou ailleurs.
    pub(super) fn release_lease(&self) {
        drop(self.lease.lock().unwrap_or_else(|e| e.into_inner()).take());
    }
}

#[cfg(test)]
mod tests {
    use super::LiveRun;
    use crate::owners::{self, Busy};
    use crate::registry::{Outcome, Status};
    use crate::state;
    use crate::store::now_ms;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    static SEQ: AtomicU32 = AtomicU32::new(0);

    /// Workspace temporaire unique (pid + rang), supprimé à la fin du test.
    struct TempWs(PathBuf);

    impl TempWs {
        fn new(tag: &str) -> Self {
            let n = SEQ.fetch_add(1, Ordering::Relaxed);
            let dir =
                std::env::temp_dir().join(format!("swarm-vivant-{tag}-{}-{n}", std::process::id()));
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

    /// Nom d'agent unique dans le process (les baux et l'index sont globaux).
    fn nom() -> String {
        format!("vivant-{}-{}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed))
    }

    fn ouvrir(ws: &TempWs, agent: Option<&str>, started_at: u64) -> LiveRun {
        LiveRun::new(format!("run-{started_at}"), "claude", agent, &ws.0, started_at)
    }

    #[test]
    fn un_run_neuf_est_vivant_sans_pid_ni_session() {
        let ws = TempWs::new("neuf");
        let run = ouvrir(&ws, Some("alice"), 42);
        let snap = run.snapshot();
        assert_eq!(snap.run_id, "run-42");
        assert_eq!(snap.runner, "claude");
        assert_eq!(snap.agent.as_deref(), Some("alice"));
        assert_eq!(snap.started_at, 42);
        assert_eq!(snap.last_output_at, 42, "aucune sortie : l'heure de départ");
        assert_eq!(snap.status, Status::Running);
        assert_eq!((snap.pid, snap.exit_code), (None, None));
        assert_eq!(snap.session_id, None);
        assert!(snap.outcome.is_none());
        assert_eq!(run.finished_at(), None);
        assert!(!run.kill_requested());
        assert_eq!(run.output_since(None), "");
        assert_eq!(run.workspace, ws.0);
    }

    #[test]
    fn la_demande_d_arret_est_definitive() {
        let ws = TempWs::new("arret");
        let run = ouvrir(&ws, None, 1);
        run.request_kill();
        assert!(run.kill_requested());
        run.request_kill();
        assert!(run.kill_requested(), "idempotente");
        // Le drapeau ne change ni le statut ni la fin : c'est l'attente qui l'applique.
        assert_eq!(run.snapshot().status, Status::Running);
        assert_eq!(run.finished_at(), None);
    }

    #[test]
    fn le_pid_et_la_session_entrent_dans_l_instantane() {
        let ws = TempWs::new("instantane");
        let run = ouvrir(&ws, None, 7);
        run.set_pid(4242);
        run.set_session("s-7");
        let snap = run.snapshot();
        assert_eq!(snap.pid, Some(4242));
        assert_eq!(snap.session_id.as_deref(), Some("s-7"));
        assert_eq!(snap.status, Status::Running);
        assert_eq!(snap.started_at, 7, "figé à la création");
        // Anonyme : aucun fichier d'état n'est écrit.
        assert!(!state::dir(&ws.0).exists());
    }

    #[test]
    fn la_fin_fige_statut_code_et_issue() {
        let ws = TempWs::new("fin");
        let started = now_ms();
        let run = ouvrir(&ws, Some("bob"), started);
        run.set_pid(4242);
        run.set_session("s-2");
        run.finish(Status::Done, Some(0), Outcome { result: "ok".into(), error: None });
        let snap = run.snapshot();
        assert_eq!(snap.status, Status::Done);
        assert_eq!(snap.exit_code, Some(0));
        let finished = run.finished_at().expect("terminé");
        assert!(finished >= started);
        let outcome = snap.outcome.expect("issue gardée");
        assert_eq!(outcome.result, "ok");
        assert_eq!(outcome.error, None);
        // Le run garde son pid et sa session après la fin.
        assert_eq!((snap.pid, snap.session_id.as_deref()), (Some(4242), Some("s-2")));
        // Un run terminé sans code.
        let autre = ouvrir(&ws, None, started);
        autre.finish(Status::Failed, None, Outcome::default());
        assert_eq!((autre.snapshot().status, autre.snapshot().exit_code), (Status::Failed, None));
    }

    #[test]
    fn un_run_nomme_ecrit_son_dernier_run_sur_disque_pas_un_anonyme() {
        let ws = TempWs::new("disque");
        let nomme = ouvrir(&ws, Some("carl"), now_ms());
        nomme.set_pid(4242);
        let doc = state::read(&ws.0, "claude", "carl");
        assert_eq!(doc["lastRun"]["runId"], nomme.run_id);
        assert_eq!(doc["lastRun"]["pid"], 4242);
        assert_eq!(doc["lastRun"]["status"], "running");
        assert!(doc["lastRun"]["exitCode"].is_null());
        assert!(doc["lastRun"]["ts"].as_u64().is_some());
        nomme.finish(Status::Killed, None, Outcome::default());
        let doc = state::read(&ws.0, "claude", "carl");
        assert_eq!(doc["lastRun"]["status"], "failed", "sur disque, killed se dit failed");
        assert!(doc["lastRun"]["lastOutputAt"].as_u64().is_some());
        assert_eq!(nomme.snapshot().status, Status::Killed, "en mémoire, le statut reste tué");
        // Anonyme : rien sur disque, même après un pid et une fin.
        let anonyme = ouvrir(&ws, None, now_ms());
        anonyme.set_pid(7);
        anonyme.finish(Status::Done, Some(0), Outcome::default());
        assert_eq!(state::list(&ws.0).len(), 1, "seul carl a un fichier d'état");
    }

    #[test]
    fn la_sortie_en_direct_s_accumule_et_ignore_les_morceaux_vides() {
        let ws = TempWs::new("sortie");
        let run = ouvrir(&ws, None, 1);
        run.push_output("bon");
        run.push_output("");
        run.push_output("jour");
        assert_eq!(run.output_since(None), "bonjour");
        assert_eq!(run.output_since(Some(u64::MAX)), "");
        assert!(run.snapshot().last_output_at >= 1);
    }

    #[test]
    fn le_bail_de_l_agent_est_battu_puis_libere_a_la_fin() {
        let ws = TempWs::new("bail");
        let agent = nom();
        let bail = owners::acquire(&ws.0, "claude", &agent, "run-1").unwrap();
        let avant = owners::holder_of(&ws.0, "claude", &agent).expect("bail pris").ts;
        let run = ouvrir(&ws, Some(&agent), now_ms());
        run.attach_lease(Some(bail));
        // Pendant la vie du run, le bail est pris : personne d'autre ne le prend.
        assert!(matches!(
            owners::acquire(&ws.0, "claude", &agent, "run-autre"),
            Err(Busy::Held(_))
        ));
        // Le battement rajeunit le bail.
        std::thread::sleep(Duration::from_millis(5));
        run.heartbeat();
        assert!(owners::holder_of(&ws.0, "claude", &agent).expect("toujours pris").ts >= avant);
        // La fin libère le bail : l'agent peut être relancé.
        run.finish(Status::Done, Some(0), Outcome::default());
        assert_eq!(owners::holder_of(&ws.0, "claude", &agent), None, "bail libéré");
        let suivant = owners::acquire(&ws.0, "claude", &agent, "run-2");
        assert!(suivant.is_ok(), "l'agent peut être relancé");
        // Un battement après la fin ne fait rien (le bail n'est plus le nôtre).
        run.heartbeat();
        drop(suivant);
        assert_eq!(owners::holder_of(&ws.0, "claude", &agent), None);
    }

    #[test]
    fn l_instantane_est_figee_et_la_sortie_se_lit_par_increment() {
        let ws = TempWs::new("increment");
        let run = ouvrir(&ws, None, 1);
        let avant = run.snapshot();
        run.set_pid(4242);
        run.push_output("un");
        // Une vue prise avant ne bouge plus quand le run change.
        assert_eq!(avant.pid, None);
        assert_eq!(avant.last_output_at, 1, "la vue d'avant ne suit pas la sortie");
        let apres_un = run.snapshot().last_output_at;
        std::thread::sleep(Duration::from_millis(2));
        run.push_output("deux");
        run.push_output("trois");
        let apres_trois = run.snapshot().last_output_at;
        assert!(apres_trois > apres_un, "chaque morceau avance l'heure de dernière sortie");
        assert_eq!(run.output_since(None), "undeuxtrois");
        assert_eq!(run.output_since(Some(apres_un)), "deuxtrois", "seuls les morceaux postérieurs");
        assert_eq!(run.output_since(Some(apres_trois)), "", "rien de plus récent que le dernier");
        // La sortie reste lisible après la fin (`agent_control stream` / `wait`).
        run.finish(Status::Done, Some(0), Outcome::default());
        assert_eq!(run.output_since(None), "undeuxtrois");
        assert_eq!(run.snapshot().last_output_at, apres_trois, "la fin ne date pas la sortie");
    }

    #[test]
    fn les_etats_ont_un_nom_stable_et_un_run_annule_finit_tue_sans_code() {
        let ws = TempWs::new("etats");
        let run = ouvrir(&ws, Some("eva"), 3);
        assert_eq!(run.snapshot().status.as_str(), "running");
        run.set_pid(4242);
        run.request_kill();
        // L'annulation demandée ne suffit pas : tant que l'attente n'a pas fini le run, il vit.
        assert_eq!(run.snapshot().status.as_str(), "running");
        run.finish(
            Status::Killed,
            None,
            Outcome { result: String::new(), error: Some("arrêté".into()) },
        );
        let snap = run.snapshot();
        assert_eq!(snap.status, Status::Killed);
        assert_eq!(snap.status.as_str(), "killed", "en mémoire, tué se dit killed");
        assert_eq!(snap.exit_code, None, "un run tué n'a pas de code de sortie");
        assert_eq!(snap.outcome.as_ref().and_then(|o| o.error.as_deref()), Some("arrêté"));
        assert!(run.kill_requested(), "la demande d'arrêt reste visible après la fin");
        assert!(run.finished_at().is_some());
        assert_eq!(state::read(&ws.0, "claude", "eva")["lastRun"]["status"], "failed");
        // Les autres états : même nom en mémoire et sur disque.
        let reussi = ouvrir(&ws, Some("fin"), 4);
        reussi.set_pid(1);
        reussi.finish(Status::Done, Some(0), Outcome::default());
        assert_eq!(reussi.snapshot().status.as_str(), "done");
        assert_eq!(state::read(&ws.0, "claude", "fin")["lastRun"]["status"], "done");
        let echoue = ouvrir(&ws, Some("echec"), 5);
        echoue.set_pid(1);
        echoue.finish(Status::Failed, Some(1), Outcome::default());
        assert_eq!(echoue.snapshot().status.as_str(), "failed");
        assert_eq!(state::read(&ws.0, "claude", "echec")["lastRun"]["status"], "failed");
    }
}
