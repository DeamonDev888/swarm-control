//! Index global des runs vivants (mémoire).

use super::{LiveRun, Snapshot, Status};
use crate::owners::{self, Busy};
use crate::store::now_ms;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

type Map = HashMap<String, Arc<LiveRun>>;

/// Durée de rétention d'un run terminé.
const FINISHED_TTL_MS: u64 = 60 * 60 * 1000;
/// Runners connus (pour retrouver un agent sans préciser son runner).
const RUNNERS: [&str; 3] = ["claude", "hermes", "antigravity"];

static LIVE: Mutex<Option<Map>> = Mutex::new(None);
static RUN_COUNTER: AtomicU64 = AtomicU64::new(0);

fn live() -> MutexGuard<'static, Option<Map>> {
    LIVE.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn key(runner: &str, agent: &str) -> String {
    format!("{runner}:{agent}")
}

/// Ouvre un run. Un agent nommé ne peut avoir qu'un run vivant à la fois — dans
/// ce process (table en mémoire) **et** dans tout autre (bail sur disque) :
/// deux reprises simultanées de la même session la corrompraient.
pub fn begin(
    runner: &'static str,
    agent: Option<&str>,
    workspace: &Path,
) -> Result<Arc<LiveRun>, String> {
    let n = RUN_COUNTER.fetch_add(1, Ordering::Relaxed);
    let now = now_ms();
    let run_id = format!("run-{now}-{n}");
    let key = key(runner, &agent.map_or_else(|| format!("_anon_{run_id}"), String::from));
    let mut guard = live();
    let map = guard.get_or_insert_with(HashMap::new);
    if let Some(existing) = map.get(&key) {
        if existing.snapshot().status == Status::Running {
            return Err(busy_here(agent, runner, &existing.run_id));
        }
    }
    let lease = match agent {
        Some(a) => Some(
            owners::acquire(workspace, runner, a, &run_id)
                .map_err(|b| busy_elsewhere(b, a, runner))?,
        ),
        None => None,
    };
    let run = Arc::new(LiveRun::new(run_id, runner, agent, workspace, now));
    run.attach_lease(lease);
    map.insert(key, Arc::clone(&run));
    Ok(run)
}

fn busy_here(agent: Option<&str>, runner: &str, run_id: &str) -> String {
    format!(
        "AGENT_BUSY: l'agent '{}' ({runner}) a déjà un run en cours ({run_id}). \
         Utilise agent_control (wait/kill) avant d'en relancer un.",
        agent.unwrap_or("?")
    )
}

fn busy_elsewhere(busy: Busy, agent: &str, runner: &str) -> String {
    match busy {
        Busy::Held(h) => format!(
            "AGENT_BUSY: l'agent '{agent}' ({runner}) tourne déjà dans une autre instance \
             ({}, run {}). Attends sa fin ou arrête-le là-bas (agent_control kill).",
            h.instance, h.run_id
        ),
        Busy::Pending => format!(
            "AGENT_BUSY: l'agent '{agent}' ({runner}) est en cours de prise en charge par \
             une autre instance : réessaie dans un instant."
        ),
        Busy::Io(e) => format!("LEASE_ERROR: bail de l'agent '{agent}' impossible ({e})."),
    }
}

/// Run vivant (ou récemment terminé) d'un agent. Sans runner : le plus récent.
pub fn find(agent: &str, runner: Option<&str>) -> Option<Arc<LiveRun>> {
    let guard = live();
    let map = guard.as_ref()?;
    let runners: Vec<&str> = runner.map_or_else(|| RUNNERS.to_vec(), |r| vec![r]);
    runners.iter().filter_map(|r| map.get(&key(r, agent))).max_by_key(|run| run.started_at).cloned()
}

pub fn find_by_run_id(run_id: &str) -> Option<Arc<LiveRun>> {
    live().as_ref()?.values().find(|r| r.run_id == run_id).cloned()
}

/// Tous les runs connus, du plus récent au plus ancien.
pub fn all() -> Vec<Snapshot> {
    let guard = live();
    let mut out: Vec<Snapshot> =
        guard.as_ref().map(|m| m.values().map(|r| r.snapshot()).collect()).unwrap_or_default();
    out.sort_by_key(|s| std::cmp::Reverse(s.started_at));
    out
}

/// Workspaces des runs connus.
pub fn workspaces() -> HashSet<PathBuf> {
    live().as_ref().map(|m| m.values().map(|r| r.workspace.clone()).collect()).unwrap_or_default()
}

/// Battement de cœur des baux de tous les runs vivants.
pub fn heartbeat() {
    let runs: Vec<Arc<LiveRun>> =
        live().as_ref().map(|m| m.values().cloned().collect()).unwrap_or_default();
    for run in runs {
        run.heartbeat();
    }
}

/// Oublie les runs terminés depuis plus d'une heure ; renvoie les PID encore actifs.
pub fn prune_finished(now: u64) -> HashSet<u32> {
    let mut active = HashSet::new();
    if let Some(map) = live().as_mut() {
        map.retain(|_, run| keep(run, now, &mut active));
    }
    active
}

fn keep(run: &LiveRun, now: u64, active: &mut HashSet<u32>) -> bool {
    let snap = run.snapshot();
    if snap.status == Status::Running {
        active.extend(snap.pid);
        return true;
    }
    run.finished_at().is_none_or(|t| now.saturating_sub(t) < FINISHED_TTL_MS)
}

#[cfg(test)]
mod tests {
    use super::{
        FINISHED_TTL_MS, all, begin, find, find_by_run_id, heartbeat, keep, key, prune_finished,
        workspaces,
    };
    use crate::owners::{self, Holder};
    use crate::registry::{LiveRun, Outcome, Status};
    use crate::store::now_ms;
    use std::collections::HashSet;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::{Arc, Barrier};
    use std::time::Duration;

    static SEQ: AtomicU32 = AtomicU32::new(0);

    /// Nom unique dans le process : l'index est global et les tests tournent en parallèle.
    fn nom() -> String {
        format!("index-{}-{}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed))
    }

    /// Workspace temporaire unique (pid + rang), supprimé à la fin du test.
    struct TempWs(PathBuf);

    impl TempWs {
        fn new(tag: &str) -> Self {
            let n = SEQ.fetch_add(1, Ordering::Relaxed);
            let dir =
                std::env::temp_dir().join(format!("swarm-index-{tag}-{}-{n}", std::process::id()));
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

    fn finir(run: &LiveRun) {
        run.finish(Status::Done, Some(0), Outcome::default());
    }

    #[test]
    fn la_cle_est_le_runner_puis_l_agent() {
        assert_eq!(key("claude", "alice"), "claude:alice");
        assert_eq!(key("hermes", "a:b"), "hermes:a:b");
    }

    #[test]
    fn un_run_ouvert_est_trouvable_par_agent_et_par_identifiant() {
        let ws = TempWs::new("ouvert");
        let agent = nom();
        let run = begin("claude", Some(&agent), &ws.0).unwrap();
        assert_eq!(run.runner, "claude");
        assert_eq!(run.agent.as_deref(), Some(agent.as_str()));
        assert_eq!(run.workspace, ws.0);
        assert!(run.run_id.starts_with("run-"), "{}", run.run_id);
        assert_eq!(run.snapshot().status, Status::Running);
        // Par agent, avec ou sans runner ; jamais sous un autre runner ni un autre nom.
        assert_eq!(
            find(&agent, Some("claude")).map(|r| r.run_id.clone()),
            Some(run.run_id.clone())
        );
        assert_eq!(find(&agent, None).map(|r| r.run_id.clone()), Some(run.run_id.clone()));
        assert!(find(&agent, Some("hermes")).is_none());
        assert!(find(&format!("{agent}-personne"), None).is_none());
        // Par identifiant.
        assert_eq!(find_by_run_id(&run.run_id).map(|r| r.run_id.clone()), Some(run.run_id.clone()));
        assert!(find_by_run_id("run-inconnu").is_none());
        // Le workspace du run est connu du balayeur.
        assert!(workspaces().contains(&ws.0));
        assert!(all().iter().any(|s| s.run_id == run.run_id));
        finir(&run);
    }

    #[test]
    fn un_agent_ne_peut_pas_avoir_deux_runs_vivants_ici() {
        let ws = TempWs::new("occupe");
        let agent = nom();
        let premier = begin("claude", Some(&agent), &ws.0).unwrap();
        let refuse = begin("claude", Some(&agent), &ws.0).err().expect("second run refusé");
        assert!(refuse.starts_with("AGENT_BUSY:"), "{refuse}");
        assert!(refuse.contains(&premier.run_id), "le message nomme le run en cours : {refuse}");
        assert!(refuse.contains(&agent), "{refuse}");
        // Terminé, le run libère sa place : l'agent repart, avec un nouvel identifiant.
        finir(&premier);
        std::thread::sleep(Duration::from_millis(2));
        let second = begin("claude", Some(&agent), &ws.0).unwrap();
        assert_ne!(second.run_id, premier.run_id);
        assert_eq!(
            find(&agent, Some("claude")).map(|r| r.run_id.clone()),
            Some(second.run_id.clone())
        );
        finir(&second);
    }

    #[test]
    fn sans_agent_les_runs_sont_anonymes_et_independants() {
        let ws = TempWs::new("anonymes");
        let un = begin("claude", None, &ws.0).unwrap();
        let deux = begin("claude", None, &ws.0).unwrap();
        assert!(un.agent.is_none() && deux.agent.is_none());
        assert_ne!(un.run_id, deux.run_id, "deux runs anonymes en même temps");
        // Joignables par leur identifiant, jamais par un nom.
        assert_eq!(find_by_run_id(&un.run_id).map(|r| r.run_id.clone()), Some(un.run_id.clone()));
        assert_eq!(
            find_by_run_id(&deux.run_id).map(|r| r.run_id.clone()),
            Some(deux.run_id.clone())
        );
        let connus: Vec<String> = all().into_iter().map(|s| s.run_id).collect();
        assert!(connus.contains(&un.run_id) && connus.contains(&deux.run_id));
        finir(&un);
        finir(&deux);
    }

    #[test]
    fn sans_runner_le_run_le_plus_recent_est_rendu() {
        let ws = TempWs::new("recent");
        let agent = nom();
        let ancien = begin("claude", Some(&agent), &ws.0).unwrap();
        finir(&ancien);
        std::thread::sleep(Duration::from_millis(2));
        let recent = begin("hermes", Some(&agent), &ws.0).unwrap();
        assert_eq!(find(&agent, None).map(|r| r.run_id.clone()), Some(recent.run_id.clone()));
        assert_eq!(
            find(&agent, Some("claude")).map(|r| r.run_id.clone()),
            Some(ancien.run_id.clone())
        );
        assert_eq!(
            find(&agent, Some("hermes")).map(|r| r.run_id.clone()),
            Some(recent.run_id.clone())
        );
        // `all()` range du plus récent au plus ancien (les autres tests y ajoutent leurs
        // propres runs : on compare la position relative des deux nôtres).
        let position = |id: &str| all().iter().position(|s| s.run_id == id).expect("listé");
        assert!(position(&recent.run_id) < position(&ancien.run_id));
        finir(&recent);
    }

    #[test]
    fn les_runs_termines_depuis_une_heure_sont_oublies_les_autres_gardes() {
        let ws = TempWs::new("oublis");
        let vivant = begin("claude", Some(&nom()), &ws.0).unwrap();
        vivant.set_pid(4242);
        let fini = begin("claude", Some(&nom()), &ws.0).unwrap();
        finir(&fini);
        let fin = fini.finished_at().expect("terminé");
        // À l'heure courante : rien n'est oublié, le PID du run vivant est rendu.
        assert!(prune_finished(now_ms()).contains(&4242));
        assert!(find_by_run_id(&fini.run_id).is_some());
        // Juste avant la limite : gardé ; à la limite : oublié.
        assert!(prune_finished(fin + FINISHED_TTL_MS - 1).contains(&4242));
        assert!(find_by_run_id(&fini.run_id).is_some(), "pas encore une heure");
        assert!(prune_finished(fin + FINISHED_TTL_MS).contains(&4242));
        assert!(find_by_run_id(&fini.run_id).is_none(), "oublié après une heure");
        // Le run vivant, lui, reste suivi.
        assert!(find_by_run_id(&vivant.run_id).is_some());
        heartbeat();
        finir(&vivant);
    }

    #[test]
    fn un_run_vivant_est_toujours_garde_et_son_pid_rendu() {
        let ws = TempWs::new("sans-pid");
        let run = begin("claude", Some(&nom()), &ws.0).unwrap();
        // Règle de rétention testée directement : l'index est global et les autres tests
        // y laissent leurs propres PID.
        let mut actifs = HashSet::new();
        assert!(
            keep(&run, u64::MAX, &mut actifs),
            "vivant : jamais oublié, quelle que soit l'heure"
        );
        assert!(actifs.is_empty(), "sans pid, rien à rendre au balayeur");
        run.set_pid(777);
        assert!(keep(&run, u64::MAX, &mut actifs));
        assert!(actifs.contains(&777), "le pid d'un run vivant est rendu");
        finir(&run);
    }

    #[test]
    fn un_bail_detenu_par_une_autre_instance_bloque_le_run_sans_l_inscrire_dans_l_index() {
        let ws = TempWs::new("ailleurs");
        let agent = nom();
        let bail = ws.0.join("bridge").join("owners").join(format!("claude-{agent}.json"));
        std::fs::create_dir_all(bail.parent().unwrap()).unwrap();
        // Bail d'une autre instance : PID vivant, frais, autre run.
        let etranger = Holder {
            instance: "swarm-control-9".into(),
            pid: std::process::id(),
            run_id: "run-etranger".into(),
            ts: now_ms(),
            ..Default::default()
        };
        std::fs::write(&bail, serde_json::to_vec(&etranger).unwrap()).unwrap();
        let refuse = begin("claude", Some(&agent), &ws.0).err().expect("refusé");
        assert!(refuse.starts_with("AGENT_BUSY:"), "{refuse}");
        assert!(refuse.contains("autre instance"), "{refuse}");
        assert!(refuse.contains("swarm-control-9") && refuse.contains("run-etranger"), "{refuse}");
        assert!(refuse.contains(&agent), "{refuse}");
        // Bail en cours de création ailleurs (fichier jeune, contenu pas encore écrit).
        std::fs::write(&bail, b"").unwrap();
        let refuse = begin("claude", Some(&agent), &ws.0).err().expect("refusé");
        assert!(refuse.starts_with("AGENT_BUSY:") && refuse.contains("réessaie"), "{refuse}");
        // Dans les deux cas rien n'est inscrit : l'agent reste libre pour le balayeur.
        assert!(find(&agent, None).is_none());
        assert!(!workspaces().contains(&ws.0));
        // Dossier des baux inutilisable : erreur de bail, pas un faux « occupé ».
        let ws_bloque = TempWs::new("inutilisable");
        std::fs::create_dir_all(ws_bloque.0.join("bridge")).unwrap();
        std::fs::write(ws_bloque.0.join("bridge").join("owners"), b"").unwrap();
        let refuse = begin("claude", Some(&nom()), &ws_bloque.0).err().expect("refusé");
        assert!(refuse.starts_with("LEASE_ERROR:"), "{refuse}");
    }

    #[test]
    fn la_course_pour_un_meme_agent_ne_laisse_qu_un_gagnant_et_les_lecteurs_ne_bloquent_pas() {
        const FILS: usize = 16;
        let ws = TempWs::new("course");
        let agent = nom();
        // Des lecteurs hammer l'index pendant que les candidats se disputent l'agent.
        let fini = Arc::new(AtomicBool::new(false));
        let lecteurs: Vec<_> = (0..4)
            .map(|_| {
                let (agent, fini) = (agent.clone(), Arc::clone(&fini));
                std::thread::spawn(move || {
                    while !fini.load(Ordering::SeqCst) {
                        let _ = find(&agent, None);
                        let _ = find_by_run_id("run-personne");
                        let _ = all();
                        let _ = workspaces();
                        let _ = prune_finished(now_ms());
                    }
                })
            })
            .collect();
        let barriere = Arc::new(Barrier::new(FILS));
        let fils: Vec<_> = (0..FILS)
            .map(|_| {
                let (agent, ws, barriere) = (agent.clone(), ws.0.clone(), Arc::clone(&barriere));
                std::thread::spawn(move || {
                    barriere.wait();
                    begin("claude", Some(&agent), &ws)
                })
            })
            .collect();
        let resultats: Vec<_> = fils.into_iter().map(|f| f.join().unwrap()).collect();
        fini.store(true, Ordering::SeqCst);
        for lecteur in lecteurs {
            lecteur.join().unwrap();
        }
        let gagnants: Vec<&Arc<LiveRun>> =
            resultats.iter().filter_map(|r| r.as_ref().ok()).collect();
        let refus = resultats.iter().filter(|r| r.is_err()).count();
        assert_eq!((gagnants.len(), refus), (1, FILS - 1), "un seul gagnant, les autres refusés");
        let gagnant = gagnants[0];
        for refuse in resultats.iter().filter_map(|r| r.as_ref().err()) {
            assert!(
                refuse.starts_with("AGENT_BUSY:") && refuse.contains(&gagnant.run_id),
                "{refuse}"
            );
        }
        assert_eq!(
            find(&agent, Some("claude")).map(|r| r.run_id.clone()),
            Some(gagnant.run_id.clone())
        );
        finir(gagnant);
        // Runs anonymes : pas de bail, chacun des seize obtient le sien.
        let barriere = Arc::new(Barrier::new(FILS));
        let fils: Vec<_> = (0..FILS)
            .map(|_| {
                let (ws, barriere) = (ws.0.clone(), Arc::clone(&barriere));
                std::thread::spawn(move || {
                    barriere.wait();
                    begin("claude", None, &ws)
                })
            })
            .collect();
        let anonymes: Vec<Arc<LiveRun>> = fils
            .into_iter()
            .map(|f| f.join().unwrap().expect("anonyme : toujours ouvert"))
            .collect();
        let distincts: HashSet<String> = anonymes.iter().map(|r| r.run_id.clone()).collect();
        assert_eq!(distincts.len(), FILS, "autant d'identifiants que de runs");
        for run in &anonymes {
            finir(run);
        }
    }

    #[test]
    fn un_run_remplace_disparait_de_l_index_et_le_pid_d_un_run_fini_n_est_plus_suivi() {
        let ws = TempWs::new("remplace");
        let agent = nom();
        let pid = 900_000 + SEQ.fetch_add(1, Ordering::Relaxed);
        let premier = begin("claude", Some(&agent), &ws.0).unwrap();
        premier.set_pid(pid);
        finir(&premier);
        // Terminé : son PID n'est plus un PID suivi par le balayeur des orphelins…
        assert!(!prune_finished(now_ms()).contains(&pid));
        // …mais le run reste retrouvable (rétention d'une heure) jusqu'à son remplacement.
        assert!(find_by_run_id(&premier.run_id).is_some());
        let second = begin("claude", Some(&agent), &ws.0).unwrap();
        assert!(find_by_run_id(&premier.run_id).is_none(), "remplacé : plus retrouvable");
        assert!(!all().iter().any(|s| s.run_id == premier.run_id), "plus listé");
        assert_eq!(
            find(&agent, None).map(|r| r.run_id.clone()),
            Some(second.run_id.clone()),
            "seul le nouveau reste"
        );
        finir(&second);
    }

    #[test]
    fn le_battement_de_l_index_rafraichit_les_baux_des_runs_vivants_uniquement() {
        let ws = TempWs::new("battement");
        let agent = nom();
        let run = begin("claude", Some(&agent), &ws.0).unwrap();
        let avant = owners::holder_of(&ws.0, "claude", &agent).expect("bail pris par begin").ts;
        std::thread::sleep(Duration::from_millis(2));
        heartbeat();
        let apres = owners::holder_of(&ws.0, "claude", &agent).expect("toujours détenu").ts;
        assert!(apres >= avant, "rajeuni par le battement de l'index ({avant} → {apres})");
        finir(&run);
        heartbeat();
        assert_eq!(
            owners::holder_of(&ws.0, "claude", &agent),
            None,
            "un run fini n'a plus de bail"
        );
    }
}
