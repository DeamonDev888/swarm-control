//! Reprise sérialisée d'un bail périmé.
//!
//! Sans garde, deux repreneurs pourraient chacun retirer le bail puis le recréer :
//! le second effacerait celui, tout neuf, du premier. Le dossier de garde
//! (`<bail>.takeover`, créé de façon exclusive) sérialise les reprises, et le
//! **remplacement s'y déroule en entier** : juger, retirer puis recréer forment une
//! seule section critique, dont la création exclusive (`create_new`) est l'issue
//! décisive — le perdant la voit en « déjà pris » (`AlreadyExists`, ou
//! `PermissionDenied` : nom en cours de suppression sous Windows) sans rien effacer.

use super::acquire::try_create;
use super::holder::{self, Holder};
use crate::store::now_ms;
use std::collections::HashSet;
use std::io::ErrorKind;
use std::path::Path;
use std::time::Duration;

/// Une garde plus vieille que ça est celle d'un process mort : on la retire.
const GUARD_MAX_AGE: Duration = Duration::from_secs(10);
const RETRY_PAUSE: Duration = Duration::from_millis(50);

/// Le bail est-il reprenable ? Détenu vivant : non. Absent : oui, sauf s'il vient
/// d'être (re)créé et que son contenu n'est pas encore écrit.
fn reprenable(file: &Path) -> bool {
    match holder::read(file) {
        Some(h) => h.is_stale(now_ms()),
        None => !holder::is_young(file),
    }
}

/// Retire le bail `file` s'il est (toujours) périmé, sous la garde.
pub fn remove_if_stale(file: &Path) {
    let _ = under_guard(file, || {
        if reprenable(file) {
            let _ = std::fs::remove_file(file);
        }
        Ok(false)
    });
}

/// Variante groupée : les PID de ce passage ont déjà été vérifiés en un seul `tasklist`.
/// Si le fichier a changé de détenteur entre-temps, on revérifie ce nouveau détenteur.
pub fn remove_if_dead(file: &Path, seen: &Holder, alive: &HashSet<u32>) {
    let _ = under_guard(file, || {
        let reprenable = match holder::read(file) {
            Some(h) if h.run_id == seen.run_id && h.pid == seen.pid => {
                let now = now_ms();
                now.saturating_sub(h.ts) > holder::STALE_MS || !alive.contains(&h.pid)
            }
            Some(h) => h.is_stale(now_ms()),
            None => !holder::is_young(file),
        };
        if reprenable {
            let _ = std::fs::remove_file(file);
        }
        Ok(false)
    });
}

/// Reprise **atomique** du bail périmé `file` au profit de `holder` : sous la garde,
/// le périmé est retiré puis remplacé dans la même section critique. `Ok(true)` :
/// créé, le bail est nôtre. `Ok(false)` : défaite fiable — détenteur vivant, création
/// déjà faite par un autre (déjà pris), ou garde occupée — l'appelant réessaie.
pub fn take_over(file: &Path, holder: &Holder) -> std::io::Result<bool> {
    under_guard(file, || {
        if !reprenable(file) {
            return Ok(false);
        }
        let _ = std::fs::remove_file(file);
        match try_create(file, holder) {
            Ok(()) => Ok(true),
            // Déjà pris (os 80/183) ou nom en suppression (os 5) : l'autre a gagné.
            Err(e)
                if matches!(e.kind(), ErrorKind::AlreadyExists | ErrorKind::PermissionDenied) =>
            {
                Ok(false)
            }
            Err(e) => Err(e),
        }
    })
}

/// Occupe la garde de `file` le temps de `body` ; `Ok(false)` si un autre repreneur
/// la tient (après avoir tenté de retirer une garde orpheline).
fn under_guard(file: &Path, body: impl FnOnce() -> std::io::Result<bool>) -> std::io::Result<bool> {
    let guard = file.with_extension("takeover");
    if std::fs::create_dir(&guard).is_err() {
        drop_old_guard(&guard);
        std::thread::sleep(RETRY_PAUSE);
        return Ok(false);
    }
    let result = body();
    let _ = std::fs::remove_dir(&guard);
    result
}

fn drop_old_guard(guard: &Path) {
    let old = guard
        .metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age > GUARD_MAX_AGE);
    if old {
        let _ = std::fs::remove_dir(guard);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        GUARD_MAX_AGE, drop_old_guard, remove_if_dead, remove_if_stale, reprenable, take_over,
        under_guard,
    };
    use crate::owners::holder::{self, Holder, STALE_MS};
    use crate::store::now_ms;
    use std::collections::HashSet;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, Barrier};
    use std::time::{Duration, SystemTime};

    static SEQ: AtomicU32 = AtomicU32::new(0);

    /// Dossier temporaire unique, avec le dossier des baux déjà créé ; supprimé à la fin.
    struct Ws(PathBuf);

    impl Ws {
        fn nouveau(tag: &str) -> Self {
            let n = SEQ.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir()
                .join(format!("swarm-takeover-{tag}-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(dir.join("bridge").join("owners")).unwrap();
            Ws(dir)
        }

        /// Le bail de run de `claude`/`alice`.
        fn bail(&self) -> PathBuf {
            self.0.join("bridge").join("owners").join("claude-alice.json")
        }
    }

    impl Drop for Ws {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Le dossier de garde qui protège `bail`.
    fn garde(bail: &Path) -> PathBuf {
        bail.with_extension("takeover")
    }

    /// Détenteur vivant (ce process) qui vient de battre.
    fn vif() -> Holder {
        Holder {
            instance: "vivant".into(),
            pid: std::process::id(),
            run_id: "run-vif".into(),
            ts: now_ms(),
            ..Default::default()
        }
    }

    /// Détenteur d'un process mort (PID jamais allouable), bail frais.
    fn mort() -> Holder {
        Holder {
            instance: "mort".into(),
            pid: u32::MAX,
            run_id: "run-mort".into(),
            ts: now_ms(),
            ..Default::default()
        }
    }

    /// Détenteur vivant mais silencieux depuis plus que `STALE_MS`.
    fn silencieux() -> Holder {
        Holder {
            instance: "silencieux".into(),
            pid: std::process::id(),
            run_id: "run-silencieux".into(),
            ts: now_ms() - STALE_MS - 1_000,
            ..Default::default()
        }
    }

    fn ecrire(bail: &Path, holder: &Holder) {
        if let Some(dir) = bail.parent() {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(bail, serde_json::to_vec(holder).unwrap()).unwrap();
    }

    fn lire(bail: &Path) -> Holder {
        holder::read(bail).expect("bail lisible")
    }

    /// Recule la date de modification de `path` (fichier ou dossier) de `age`.
    fn vieillir(path: &Path, age: Duration) {
        let date = SystemTime::now() - age;
        ouvert_pour_les_dates(path)
            .and_then(|f| f.set_modified(date))
            .expect("date de modification repoussée");
    }

    #[cfg(windows)]
    fn ouvert_pour_les_dates(path: &Path) -> io::Result<std::fs::File> {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_READ_ATTRIBUTES: u32 = 0x80;
        const FILE_WRITE_ATTRIBUTES: u32 = 0x100;
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        // Un dossier refuse l'ouverture en écriture : on ne demande que le droit de modifier
        // ses attributs, et `FILE_FLAG_BACKUP_SEMANTICS` autorise `CreateFile` sur un dossier.
        std::fs::OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)
    }

    #[cfg(unix)]
    fn ouvert_pour_les_dates(path: &Path) -> io::Result<std::fs::File> {
        std::fs::File::open(path)
    }

    #[test]
    fn un_bail_frais_d_un_vivant_n_est_pas_reprenable() {
        let ws = Ws::nouveau("frais");
        let bail = ws.bail();
        ecrire(&bail, &vif());
        assert!(!reprenable(&bail));
    }

    #[test]
    fn un_bail_perime_par_l_age_ou_par_un_pid_mort_est_reprenable() {
        let ws = Ws::nouveau("perime");
        let bail = ws.bail();
        // Silencieux : seul l'âge le périme, son PID vit.
        ecrire(&bail, &silencieux());
        assert!(reprenable(&bail), "périmé par l'âge");
        // Frais, mais son process est mort.
        ecrire(&bail, &mort());
        assert!(reprenable(&bail), "périmé par un PID mort");
    }

    #[test]
    fn un_bail_absent_est_reprenable_mais_pas_un_fichier_jeune_en_cours_d_ecriture() {
        let ws = Ws::nouveau("absent");
        let bail = ws.bail();
        assert!(reprenable(&bail), "pas de bail : libre");
        // Fichier tout juste créé, contenu pas encore écrit : quelqu'un est en train de le prendre.
        std::fs::write(&bail, b"").unwrap();
        assert!(!reprenable(&bail), "jeune et vide : en cours d'écriture");
        std::fs::write(&bail, b"{pas du json").unwrap();
        assert!(!reprenable(&bail), "jeune et illisible : en cours d'écriture");
        // Même contenu illisible, mais vieux : c'est un débris, on le reprend.
        vieillir(&bail, Duration::from_secs(30));
        assert!(reprenable(&bail), "vieux et illisible : débris repris");
    }

    #[test]
    fn reprendre_un_bail_perime_le_remplace_et_le_rend_notre() {
        let ws = Ws::nouveau("remplace");
        let bail = ws.bail();
        ecrire(&bail, &silencieux());
        let moi = Holder::current("run-moi");
        assert!(take_over(&bail, &moi).unwrap(), "bail repris");
        let lu = lire(&bail);
        assert_eq!((lu.run_id.as_str(), lu.pid), ("run-moi", std::process::id()));
        assert!(!garde(&bail).exists(), "la garde est relâchée après la reprise");
        // Sans bail du tout : la reprise crée le nôtre.
        let autre = ws.0.join("bridge").join("owners").join("claude-bob.json");
        assert!(take_over(&autre, &moi).unwrap(), "créé");
        assert_eq!(lire(&autre).run_id, "run-moi");
    }

    #[test]
    fn reprendre_un_bail_frais_est_une_defaite_sans_effet() {
        let ws = Ws::nouveau("defaite");
        let bail = ws.bail();
        ecrire(&bail, &vif());
        assert!(!take_over(&bail, &Holder::current("run-moi")).unwrap(), "défaite");
        assert_eq!(lire(&bail).run_id, "run-vif", "le bail du vivant est intact");
        assert!(!garde(&bail).exists(), "aucune garde abandonnée");
    }

    #[test]
    fn une_garde_tenue_par_un_autre_est_une_defaite_sans_rien_effacer() {
        let ws = Ws::nouveau("garde-tenue");
        let bail = ws.bail();
        ecrire(&bail, &silencieux());
        // Un autre repreneur tient la garde (créée à l'instant : pas orpheline).
        std::fs::create_dir(garde(&bail)).unwrap();
        assert!(!take_over(&bail, &Holder::current("run-moi")).unwrap(), "garde occupée : défaite");
        assert_eq!(lire(&bail).run_id, "run-silencieux", "le bail périmé n'est pas effacé");
        assert!(garde(&bail).exists(), "la garde de l'autre n'est pas retirée");
        // Le nettoyage du balayeur passe par la même garde.
        remove_if_stale(&bail);
        assert!(bail.exists(), "toujours là");
        assert!(garde(&bail).exists());
    }

    #[test]
    fn une_garde_orpheline_est_retiree_et_la_reprise_passe_au_coup_suivant() {
        let ws = Ws::nouveau("garde-orpheline");
        let bail = ws.bail();
        ecrire(&bail, &silencieux());
        // Garde laissée par un process mort : plus vieille que `GUARD_MAX_AGE`.
        std::fs::create_dir(garde(&bail)).unwrap();
        vieillir(&garde(&bail), GUARD_MAX_AGE + Duration::from_secs(10));
        // Ce passage ne prend pas la garde (elle semblait occupée) : défaite, mais il
        // retire l'orpheline.
        assert!(!take_over(&bail, &Holder::current("run-moi")).unwrap(), "garde occupée : défaite");
        assert!(!garde(&bail).exists(), "garde orpheline retirée");
        assert!(bail.exists(), "rien d'effacé sans la garde");
        // Le suivant passe normalement.
        assert!(take_over(&bail, &Holder::current("run-moi")).unwrap(), "repris au coup suivant");
        assert_eq!(lire(&bail).run_id, "run-moi");
    }

    #[test]
    fn drop_old_guard_ne_retire_que_les_gardes_vieilles() {
        let ws = Ws::nouveau("age-garde");
        let g = ws.0.join("bridge").join("owners").join("claude-alice.takeover");
        std::fs::create_dir(&g).unwrap();
        drop_old_guard(&g);
        assert!(g.exists(), "fraîche : gardée");
        vieillir(&g, GUARD_MAX_AGE - Duration::from_secs(1));
        drop_old_guard(&g);
        assert!(g.exists(), "juste sous la limite : gardée");
        vieillir(&g, GUARD_MAX_AGE + Duration::from_secs(1));
        drop_old_guard(&g);
        assert!(!g.exists(), "au-delà de la limite : retirée");
    }

    #[test]
    fn la_garde_est_relassee_meme_quand_le_corps_echoue() {
        let ws = Ws::nouveau("relache");
        let bail = ws.bail();
        let raté = under_guard(&bail, || Err(io::Error::other("boum")));
        assert_eq!(raté.unwrap_err().to_string(), "boum", "l'erreur traverse");
        assert!(!garde(&bail).exists(), "pas de garde abandonnée sur erreur");
        assert!(under_guard(&bail, || Ok(true)).unwrap(), "l'appel suivant passe");
    }

    #[test]
    fn une_creation_refusee_est_une_defaite_pas_une_erreur() {
        let ws = Ws::nouveau("refusee");
        let bail = ws.bail();
        // Le nom du bail est occupé par un dossier âgé : le retrait échoue (ignoré) et la
        // création exclusive est refusée par le système — sous Windows c'est l'erreur 5
        // (`PermissionDenied`, celle d'un nom en cours de suppression), sous Unix « déjà
        // pris » (`AlreadyExists`). Dans les deux cas : réessai, jamais une erreur.
        std::fs::create_dir_all(&bail).unwrap();
        vieillir(&bail, Duration::from_secs(30));
        assert!(!take_over(&bail, &Holder::current("run-moi")).unwrap(), "défaite, pas une erreur");
        assert!(bail.is_dir(), "rien n'a été effacé");
        assert!(!garde(&bail).exists(), "la garde est relâchée");
    }

    #[test]
    fn un_seul_repreneur_gagne_la_course_et_les_perdants_n_effacent_rien() {
        const FILS: usize = 16;
        for present in [true, false] {
            let ws = Ws::nouveau(if present { "course" } else { "course-vide" });
            let bail = ws.bail();
            if present {
                ecrire(&bail, &silencieux());
            }
            let barriere = Arc::new(Barrier::new(FILS));
            let fils: Vec<_> = (0..FILS)
                .map(|i| {
                    let (bail, barriere) = (bail.clone(), Arc::clone(&barriere));
                    std::thread::spawn(move || {
                        barriere.wait();
                        take_over(&bail, &Holder::current(&format!("gagnant-{i}")))
                    })
                })
                .collect();
            let mut victoires = 0;
            for fil in fils {
                // Une défaite fiable n'est jamais une erreur : aucune ne doit remonter.
                victoires += usize::from(fil.join().unwrap().expect("pas d'erreur"));
            }
            assert_eq!(victoires, 1, "exactement un gagnant (bail initial : {present})");
            let lu = lire(&bail);
            assert!(lu.run_id.starts_with("gagnant-"), "le bail porte le gagnant : {}", lu.run_id);
            assert!(!garde(&bail).exists(), "aucune garde abandonnée après la course");
        }
    }

    #[test]
    fn remove_if_stale_retire_le_perime_et_garde_le_frais() {
        let ws = Ws::nouveau("retire");
        let bail = ws.bail();
        // Sans fichier : rien à faire, pas d'erreur.
        remove_if_stale(&bail);
        assert!(!bail.exists());
        ecrire(&bail, &vif());
        remove_if_stale(&bail);
        assert!(bail.exists(), "le bail d'un vivant reste");
        ecrire(&bail, &mort());
        remove_if_stale(&bail);
        assert!(!bail.exists(), "le bail d'un mort est retiré");
        assert!(!garde(&bail).exists());
    }

    #[test]
    fn remove_if_dead_verifie_le_detenteur_l_age_et_le_pid() {
        let ws = Ws::nouveau("morts");
        let bail = ws.bail();
        // Même détenteur que celui lu par le balayeur, frais, PID vivant : gardé.
        let vu = vif();
        ecrire(&bail, &vu);
        remove_if_dead(&bail, &vu, &HashSet::from([vu.pid]));
        assert!(bail.exists(), "vivant : gardé");
        // Même détenteur, PID mort au passage : retiré.
        remove_if_dead(&bail, &vu, &HashSet::new());
        assert!(!bail.exists(), "mort : retiré");
        // Même détenteur, périmé par l'âge même si son PID vit : retiré.
        let muet = silencieux();
        ecrire(&bail, &muet);
        remove_if_dead(&bail, &muet, &HashSet::from([muet.pid]));
        assert!(!bail.exists(), "silencieux : retiré malgré un PID vivant");
        // Le détenteur a changé depuis la lecture du balayeur : c'est le nouveau qu'on juge.
        ecrire(&bail, &vif());
        remove_if_dead(&bail, &muet, &HashSet::new());
        assert!(bail.exists(), "nouveau détenteur vivant : gardé");
        assert_eq!(lire(&bail).run_id, "run-vif");
        // Nouveau détenteur mort : retiré, même si l'ancien PID était « vu vivant ».
        let morte = Holder {
            instance: "x".into(),
            pid: u32::MAX,
            run_id: "run-x".into(),
            ts: now_ms(),
            ..Default::default()
        };
        ecrire(&bail, &morte);
        remove_if_dead(&bail, &muet, &HashSet::from([u32::MAX]));
        assert!(!bail.exists(), "nouveau détenteur mort : retiré");
        // Fichier disparu entre-temps : rien à faire.
        remove_if_dead(&bail, &muet, &HashSet::new());
        assert!(!garde(&bail).exists());
    }
}
