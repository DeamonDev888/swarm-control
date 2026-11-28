//! Un PID désigne-t-il un process vivant ?
//!
//! Sous Windows, chaque `tasklist` coûte 50–100 ms. Choix retenu, en std uniquement :
//! un **cache court par PID** (`CACHE_TTL`, une seconde) — il profite à tous les appelants
//! épars (reprise de bail, inspection, découverte de pairs, `kill`) sans changer leurs
//! signatures — complété par la requête **groupée** `pids_alive`, un seul `tasklist` pour
//! les PID sans entrée fraîche d'un même passage du balayeur (les autres sont servis du
//! cache, zéro `tasklist` tant que rien n'a expiré). Une entrée est invalidée par le temps
//! (TTL) : une erreur éventuelle de `tasklist` ou la mort d'un process ne peuvent tromper
//! personne plus d'une seconde. La lecture du système est injectable (`pids_alive_with`) :
//! les tests comptent les `tasklist` lancés.

use std::process::Stdio;

#[cfg(windows)]
use std::collections::{HashMap, HashSet};
#[cfg(windows)]
use std::path::Path;
#[cfg(windows)]
use std::sync::Mutex;
#[cfg(windows)]
use std::time::{Duration, Instant};

/// Sous Windows, `tasklist` coûte 50–100 ms : un résultat est gardé une seconde.
#[cfg(windows)]
const CACHE_TTL: Duration = Duration::from_secs(1);
#[cfg(windows)]
const CACHE_MAX: usize = 256;

#[cfg(windows)]
static CACHE: Mutex<Option<HashMap<u32, (bool, Instant)>>> = Mutex::new(None);

#[cfg(windows)]
fn cached(pid: u32) -> Option<bool> {
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let cache = guard.as_mut()?;
    let (alive, at) = cache.get(&pid)?;
    if at.elapsed() <= CACHE_TTL {
        return Some(*alive);
    }
    cache.remove(&pid);
    None
}

#[cfg(windows)]
fn remember(pid: u32, alive: bool) {
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let cache = guard.get_or_insert_with(HashMap::new);
    if cache.len() >= CACHE_MAX {
        cache.retain(|_, (_, at)| at.elapsed() <= CACHE_TTL);
    }
    cache.insert(pid, (alive, Instant::now()));
}

#[cfg(windows)]
pub fn pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    if pid == std::process::id() {
        // Vivant par définition : aucune sonde externe ne doit pouvoir juger notre
        // propre PID mort (une erreur `tasklist` sous charge le « tuerait » une
        // seconde, et ferait retirer nos baux tout neufs par un repreneur).
        return true;
    }
    if let Some(alive) = cached(pid) {
        return alive;
    }
    let filter = format!("PID eq {pid}");
    let alive = super::command(std::path::Path::new("tasklist"))
        .args(["/FI", &filter, "/FO", "CSV", "/NH"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains(&format!("\"{pid}\"")))
        .unwrap_or(false);
    remember(pid, alive);
    alive
}

/// Les PID vivants parmi `pids` : le cache d'abord, **un seul** `tasklist` sous Windows
/// pour les PID sans entrée fraîche (premier passage ou TTL dépassé) ; aucun quand tout
/// est en cache valide.
#[cfg(windows)]
pub fn pids_alive(pids: &[u32]) -> HashSet<u32> {
    pids_alive_with(pids, tasklist_pids)
}

/// Tous les PID vivants au sens de `tasklist` (un appel, CSV sans en-tête). Un lancement
/// raté rend un ensemble vide : les PID demandés passent pour morts, au plus une seconde
/// (TTL du cache).
#[cfg(windows)]
fn tasklist_pids() -> HashSet<u32> {
    super::command(Path::new("tasklist"))
        .args(["/FO", "CSV", "/NH"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).lines().filter_map(csv_pid).collect())
        .unwrap_or_default()
}

/// [`pids_alive`] avec la lecture du système injectée : `list` n'est appelée que s'il
/// reste des PID sans entrée fraîche — les tests comptent ses appels.
#[cfg(windows)]
fn pids_alive_with(pids: &[u32], list: impl Fn() -> HashSet<u32>) -> HashSet<u32> {
    let wanted: HashSet<u32> = pids.iter().copied().filter(|p| *p != 0).collect();
    if wanted.is_empty() {
        return HashSet::new();
    }
    let mut alive: HashSet<u32> = HashSet::new();
    let mut missing: Vec<u32> = Vec::new();
    for &pid in &wanted {
        if pid == std::process::id() {
            alive.insert(pid);
            continue;
        }
        match cached(pid) {
            Some(true) => {
                alive.insert(pid);
            }
            Some(false) => {}
            None => missing.push(pid),
        }
    }
    if missing.is_empty() {
        return alive;
    }
    let listed = list();
    for pid in missing {
        let live = listed.contains(&pid);
        if live {
            alive.insert(pid);
        }
        remember(pid, live);
    }
    alive
}

/// Deuxième champ d'une ligne `"image.exe","1234",…` de `tasklist /FO CSV`. Tous les champs
/// sont entre guillemets : une virgule dans un nom d'image (`"a,b.exe","1234"`) ne sépare
/// rien.
#[cfg(windows)]
fn csv_pid(line: &str) -> Option<u32> {
    let mut quoted = false;
    line.split(|c| match c {
        '"' => {
            quoted = !quoted;
            false
        }
        ',' => !quoted,
        _ => false,
    })
    .nth(1)?
    .trim()
    .trim_matches('"')
    .parse()
    .ok()
}

#[cfg(unix)]
pub fn pids_alive(pids: &[u32]) -> std::collections::HashSet<u32> {
    pids.iter().copied().filter(|pid| pid_alive(*pid)).collect()
}

/// PID utilisable avec `kill` : `0` vise notre propre groupe, et au-delà de
/// `i32::MAX` la valeur repasse en négatif (`u32::MAX` devient `-1` : tous les process).
#[cfg(unix)]
pub(super) fn valid_unix_pid(pid: u32) -> bool {
    pid != 0 && i32::try_from(pid).is_ok()
}

#[cfg(unix)]
pub fn pid_alive(pid: u32) -> bool {
    if !valid_unix_pid(pid) {
        return false;
    }
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// Fait passer l'entrée « vivante » de `pid` pour plus vieille que le TTL, sans attendre.
    fn perime_vivant(pid: u32) {
        let ancien = Instant::now()
            .checked_sub(CACHE_TTL + Duration::from_millis(50))
            .expect("horodatage passé calculable");
        CACHE
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_mut()
            .expect("cache initialisé")
            .insert(pid, (true, ancien));
    }

    /// Copie de la table du cache, lue sans le seuil de fraîcheur de `cached`.
    fn entrees() -> HashMap<u32, (bool, Instant)> {
        CACHE.lock().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_default()
    }

    #[test]
    fn csv_pid_lit_le_deuxieme_champ() {
        assert_eq!(csv_pid(r#""image.exe","1234","Console","1","8 000 K""#), Some(1234));
        assert_eq!(csv_pid("swarm-control.exe,42,Console,1,1 024"), Some(42));
        assert_eq!(csv_pid(r#""image.exe","7","Console""#), Some(7));
        assert_eq!(csv_pid(""), None);
        assert_eq!(csv_pid(r#""seul.csv""#), None);
        assert_eq!(csv_pid(r#""image.exe","abc","Console""#), None);
    }

    #[test]
    fn csv_pid_ignore_une_virgule_dans_le_nom_et_les_lignes_hors_tableau() {
        assert_eq!(csv_pid(r#""a,b.exe","1234","Console","1","8 000 K""#), Some(1234));
        assert_eq!(csv_pid("\"image.exe\",\"4294967295\",\"Console\"\r"), Some(u32::MAX));
        assert_eq!(csv_pid(r#""image.exe","99999999999","Console""#), None);
        // En-tête (appel sans /NH) et message « aucune tâche » : jamais un PID.
        assert_eq!(csv_pid("\"Image Name\",\"PID\",\"Session Name\""), None);
        assert_eq!(csv_pid("INFO: No tasks are running which match the specified criteria."), None);
    }

    #[test]
    fn une_entree_de_cache_expire_apres_le_ttl() {
        // PID fantôme propre à ce test (impair, jamais allouable sous Windows) pour ne
        // pas gêner les autres tests qui partagent le même cache statique.
        let ghost = 3_000_000_103_u32;
        remember(ghost, true);
        assert_eq!(cached(ghost), Some(true));
        // L'horodatage est repoussé au-delà du TTL : l'entrée ne peut plus servir et
        // disparaît à la première consultation.
        let ancien = Instant::now()
            .checked_sub(CACHE_TTL + Duration::from_millis(50))
            .expect("horodatage passé calculable");
        let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        cache.as_mut().expect("cache initialisé").insert(ghost, (true, ancien));
        drop(cache);
        assert_eq!(cached(ghost), None, "l'entrée au-delà du TTL doit expirer");
        assert_eq!(cached(ghost), None, "l'entrée expirée doit avoir été retirée");
    }

    #[test]
    fn pids_alive_sert_le_cache_au_second_passage() {
        // Trois PID fantômes (impairs, jamais allouables), n'employés par aucun autre test
        // du binaire : le cache est statique et commun à tous les fils. `3_000_000_105`,
        // d'abord choisi ici, l'est aussi par `cli::ask`, qui y écrit « mort ».
        let ghosts: Vec<u32> = (3_000_000_205..=3_000_000_209).step_by(2).collect();
        for &ghost in &ghosts {
            assert!(
                !entrees().contains_key(&ghost),
                "PID fantôme déjà en cache : {ghost} est partagé avec un autre test"
            );
        }
        // Premier passage : le vrai `tasklist` les ignore, et son verdict (mort) est écrit
        // pour chacun.
        assert!(pids_alive(&ghosts).is_empty());
        for &ghost in &ghosts {
            assert_eq!(
                entrees().get(&ghost).map(|(vivant, _)| *vivant),
                Some(false),
                "le premier passage écrit le verdict de tasklist"
            );
        }
        // Second passage servi du cache, sans `tasklist`. Preuve sans chronomètre (sous
        // charge, un fil peut être préempté bien au-delà de 15 ms sans rien lancer) : une
        // entrée fraîche « vivante » ressort vivante, alors qu'un `tasklist` la dirait
        // morte et remplacerait son verdict.
        CACHE
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_mut()
            .expect("cache initialisé")
            .insert(ghosts[1], (true, Instant::now()));
        assert_eq!(pids_alive(&ghosts), HashSet::from([ghosts[1]]));
    }

    #[test]
    fn aucun_tasklist_tant_que_tout_est_en_cache_valide() {
        // Cinq PID fantômes propres à ce test (impairs, jamais allouables sous Windows).
        let ghosts: Vec<u32> = (3_000_000_111..=3_000_000_119).step_by(2).collect();
        let appels = Cell::new(0u32);
        let list = || {
            appels.set(appels.get() + 1);
            HashSet::new()
        };
        // Premier passage : rien en cache, une seule lecture pour les cinq.
        assert!(pids_alive_with(&ghosts, list).is_empty());
        assert_eq!(appels.get(), 1, "un seul tasklist pour les PID absents du cache");
        // Second passage : tout est en cache frais, aucune lecture.
        assert!(pids_alive_with(&ghosts, list).is_empty());
        assert_eq!(appels.get(), 1, "aucun tasklist quand tout est en cache valide");
        // Un seul PID périmé : une lecture pour lui seul, les autres restent servis du cache.
        let ancien = Instant::now()
            .checked_sub(CACHE_TTL + Duration::from_millis(50))
            .expect("horodatage passé calculable");
        CACHE
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_mut()
            .expect("cache initialisé")
            .insert(ghosts[0], (true, ancien));
        let alive = pids_alive_with(&ghosts, || {
            appels.set(appels.get() + 1);
            HashSet::from([ghosts[0]])
        });
        assert_eq!(appels.get(), 2, "une lecture pour le seul PID périmé");
        assert_eq!(alive, HashSet::from([ghosts[0]]), "seul le PID relu peut changer d'avis");
    }

    #[test]
    fn notre_propre_pid_est_vivant_sans_lire_le_systeme() {
        let alive =
            pids_alive_with(&[0, std::process::id()], || panic!("tasklist pour notre propre PID"));
        assert_eq!(alive, HashSet::from([std::process::id()]), "0 ignoré, le nôtre vivant");
    }

    #[test]
    fn un_process_mort_apres_le_ttl_ne_sort_plus_vivant() {
        // PID fantôme propre à ce test (impair, jamais allouable sous Windows).
        let ghost = 3_000_000_121_u32;
        let appels = Cell::new(0u32);
        let mort = || {
            appels.set(appels.get() + 1);
            HashSet::new()
        };
        // Verdict « vivant » encore frais : servi sans sonder.
        remember(ghost, true);
        assert_eq!(pids_alive_with(&[ghost], mort), HashSet::from([ghost]));
        assert_eq!(appels.get(), 0, "une entrée fraîche est servie sans tasklist");
        // Même verdict, périmé : tasklist est consulté et son avis (mort) l'emporte.
        perime_vivant(ghost);
        let alive = pids_alive_with(&[ghost], mort);
        assert!(alive.is_empty(), "un PID absent de tasklist ne doit plus sortir vivant");
        assert_eq!(appels.get(), 1, "une relecture pour le PID périmé");
        // Le nouveau verdict est en cache : le passage suivant ne relit pas.
        assert!(pids_alive_with(&[ghost], mort).is_empty());
        assert_eq!(appels.get(), 1, "le verdict frais est servi du cache");
        assert_eq!(cached(ghost), Some(false));
    }

    #[test]
    fn une_entree_fraiche_vivante_n_est_pas_rejugee_par_un_tasklist_lance_pour_un_autre() {
        // Deux PID fantômes propres à ce test (impairs, jamais allouables sous Windows).
        let (frais, absent) = (3_000_000_123_u32, 3_000_000_125_u32);
        let appels = Cell::new(0u32);
        let vide = || {
            appels.set(appels.get() + 1);
            HashSet::new()
        };
        remember(frais, true);
        // Seul `absent` manque au cache : tasklist tourne pour lui et ne connaît pas `frais`
        // non plus — qui reste servi de son entrée fraîche.
        assert_eq!(pids_alive_with(&[frais, absent], vide), HashSet::from([frais]));
        assert_eq!(appels.get(), 1, "une lecture, pour le seul PID absent du cache");
        assert_eq!(cached(absent), Some(false), "le verdict de tasklist est mémorisé pour lui");
    }

    #[test]
    fn pid_alive_sert_une_entree_fraiche_et_resonde_une_entree_perimee() {
        // Deux PID fantômes propres à ce test : sans cache, `tasklist` les dirait morts.
        let (frais, perime) = (3_000_000_127_u32, 3_000_000_129_u32);
        remember(frais, true);
        remember(perime, true);
        assert!(pid_alive(frais), "une entrée fraîche est servie sans sonder");
        perime_vivant(perime);
        assert!(!pid_alive(perime), "une entrée périmée est re-sondée : tasklist la dit morte");
        assert_eq!(cached(perime), Some(false), "le nouveau verdict remplace l'ancien");
    }
}

#[cfg(all(test, unix))]
mod tests_unix {
    use super::*;

    #[test]
    fn valid_unix_pid_bornes() {
        assert!(!valid_unix_pid(0));
        assert!(valid_unix_pid(1));
        assert!(valid_unix_pid(i32::MAX as u32));
        assert!(!valid_unix_pid(i32::MAX as u32 + 1));
        assert!(!valid_unix_pid(u32::MAX));
    }
}

/// Ce qui vaut sur les deux familles de systèmes.
#[cfg(test)]
mod tests_communs {
    use super::pid_alive;

    #[test]
    fn pid_alive_nous_trouve_vivant_et_juge_morts_les_pid_impossibles() {
        assert!(pid_alive(std::process::id()), "notre propre PID est vivant, sans sondage");
        assert!(!pid_alive(0), "0 ne désigne aucun process");
        assert!(!pid_alive(u32::MAX), "4294967295 n'est pas un PID utilisable");
    }
}
