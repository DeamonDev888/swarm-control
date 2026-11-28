//! Attente de la fin d'un process, avec délai, arrêt et annulation.

use super::{End, kill_tree};
use crate::registry::LiveRun;
use litemcp::cancel::CancelToken;
use std::process::Child;
use std::sync::Arc;
use std::time::{Duration, Instant};

const POLL: Duration = Duration::from_millis(100);
const PROGRESS_EVERY: Duration = Duration::from_secs(10);

/// Bloque jusqu'à la fin du process ; le tue si besoin. Renvoie code et cause.
pub fn until_done(
    child: &mut Child,
    live: &Arc<LiveRun>,
    timeout: Duration,
    cancel: Option<&CancelToken>,
) -> (Option<i32>, End) {
    let pid = child.id();
    let started = Instant::now();
    let mut last_progress = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return (status.code(), End::Exited),
            Ok(None) => {}
            Err(_) => return (None, End::Exited),
        }
        if let Some(reason) = stop_reason(live, cancel, started, timeout) {
            kill_tree(pid);
            let _ = child.kill();
            let status = child.wait().ok();
            return (status.and_then(|s| s.code()), reason);
        }
        if last_progress.elapsed() >= PROGRESS_EVERY {
            last_progress = Instant::now();
            report_progress(started);
        }
        std::thread::sleep(POLL);
    }
}

fn stop_reason(
    live: &LiveRun,
    cancel: Option<&CancelToken>,
    started: Instant,
    timeout: Duration,
) -> Option<End> {
    if live.kill_requested() {
        Some(End::Killed)
    } else if cancel.is_some_and(CancelToken::is_cancelled) {
        Some(End::Cancelled)
    } else if started.elapsed() >= timeout {
        Some(End::TimedOut)
    } else {
        None
    }
}

/// Sans jeton de progression côté client, c'est une non-action.
fn report_progress(started: Instant) {
    let secs = started.elapsed().as_secs();
    litemcp::progress::report(secs as f64, None, Some(&format!("agent en cours depuis {secs}s")));
}

#[cfg(test)]
mod tests {
    use super::{report_progress, stop_reason, until_done};
    use crate::process::{End, command, kill_tree, pid_alive};
    use crate::registry::LiveRun;
    use litemcp::cancel::CancelToken;
    use std::io::{BufRead, BufReader};
    use std::path::Path;
    use std::process::{Command, Stdio};
    use std::sync::Arc;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    /// Un run anonyme sur un chemin jamais écrit : rien ne touche au disque ici.
    fn live() -> Arc<LiveRun> {
        Arc::new(LiveRun::new("run-attente".into(), "claude", None, Path::new("."), 1))
    }

    /// Commande système inoffensive (jamais un CLI d'agent) : dort `secs` secondes.
    fn dort(secs: u64) -> Command {
        let mut cmd = if cfg!(windows) {
            let script = format!("ping -n {} 127.0.0.1 >nul", secs + 1);
            let mut cmd = command(Path::new("cmd"));
            cmd.args(["/C", &script]);
            cmd
        } else {
            let script = format!("sleep {secs}");
            let mut cmd = command(Path::new("sh"));
            cmd.args(["-c", &script]);
            cmd
        };
        cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        cmd
    }

    /// Commande système inoffensive : sort avec `code`.
    fn sort_avec(code: i32) -> Command {
        let mut cmd = if cfg!(windows) {
            let script = format!("exit {code}");
            let mut cmd = command(Path::new("cmd"));
            cmd.args(["/C", &script]);
            cmd
        } else {
            let script = format!("exit {code}");
            let mut cmd = command(Path::new("sh"));
            cmd.args(["-c", &script]);
            cmd
        };
        cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        cmd
    }

    /// Lève `action` depuis un fil, 200 ms plus tard (le temps d'entrer dans l'attente).
    fn apres_un_moment(action: impl FnOnce() + Send + 'static) {
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            action();
        });
    }

    /// Commande système inoffensive : travaille `secs` secondes puis sort avec `code`.
    fn travaille_puis_sort(secs: u64, code: i32) -> Command {
        let mut cmd = if cfg!(windows) {
            let script = format!("ping -n {} 127.0.0.1 >nul & exit {code}", secs + 1);
            let mut cmd = command(Path::new("cmd"));
            cmd.args(["/C", &script]);
            cmd
        } else {
            let script = format!("sleep {secs}; exit {code}");
            let mut cmd = command(Path::new("sh"));
            cmd.args(["-c", &script]);
            cmd
        };
        cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        cmd
    }

    /// Arbre à deux étages qui dort 30 s, sans rien écrire sur disque : le parent écrit
    /// `PID=<pid du petit-fils>` sur stdout puis attend. Ce petit-fils est le descendant
    /// qu'un arrêt du seul parent laisserait tourner. Jamais un CLI d'agent.
    fn arbre_dormant() -> Command {
        let mut cmd = if cfg!(windows) {
            let script = "$g = Start-Process -PassThru -NoNewWindow cmd.exe \
                -ArgumentList '/C ping -n 31 127.0.0.1 >nul'; \
                [Console]::Out.WriteLine('PID=' + $g.Id); [Console]::Out.Flush(); \
                Start-Sleep -Seconds 31";
            let mut cmd = command(Path::new("powershell"));
            cmd.args(["-NoProfile", "-NonInteractive", "-Command", script]);
            cmd
        } else {
            let script = "sleep 31 & echo \"PID=$!\"; sleep 31";
            let mut cmd = command(Path::new("sh"));
            cmd.args(["-c", script]);
            cmd
        };
        cmd.current_dir(std::env::temp_dir())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        cmd
    }

    /// PID annoncé par la ligne `PID=<n>` du parent.
    fn pid_annonce(ligne: &str) -> Option<u32> {
        ligne.trim().strip_prefix("PID=")?.parse().ok()
    }

    #[test]
    fn un_process_termine_rend_son_code() {
        let mut child = sort_avec(3).spawn().unwrap();
        let live = live();
        let (code, end) = until_done(&mut child, &live, Duration::from_secs(60), None);
        assert_eq!((code, end), (Some(3), End::Exited));
        // Un process déjà attendu se relit sans erreur.
        let (code, end) = until_done(&mut child, &live, Duration::from_secs(60), None);
        assert_eq!((code, end), (Some(3), End::Exited));
    }

    #[test]
    fn un_process_qui_ne_finit_pas_est_arrete_au_delai() {
        let mut child = dort(30).spawn().unwrap();
        let live = live();
        let started = Instant::now();
        let (_, end) = until_done(&mut child, &live, Duration::from_millis(300), None);
        assert_eq!(end, End::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(20), "l'arrêt a traîné");
        // Le process a été attendu après l'arrêt : il est terminé, pas orphelin.
        assert!(child.try_wait().unwrap().is_some(), "le process dort encore");
    }

    #[test]
    fn une_demande_d_arret_pendant_l_attente_est_suivie() {
        let mut child = dort(30).spawn().unwrap();
        let live = live();
        let porteuse = Arc::clone(&live);
        apres_un_moment(move || porteuse.request_kill());
        let (_, end) = until_done(&mut child, &live, Duration::from_secs(60), None);
        assert_eq!(end, End::Killed);
        assert!(child.try_wait().unwrap().is_some(), "le process dort encore");
    }

    #[test]
    fn une_annulation_du_client_arrete_le_process() {
        let mut child = dort(30).spawn().unwrap();
        let live = live();
        let jeton = CancelToken::new();
        let clone = jeton.clone();
        apres_un_moment(move || clone.cancel());
        let (_, end) = until_done(&mut child, &live, Duration::from_secs(60), Some(&jeton));
        assert_eq!(end, End::Cancelled);
        assert!(child.try_wait().unwrap().is_some(), "le process dort encore");
    }

    #[test]
    fn l_arret_demande_passe_avant_l_annulation_et_le_delai() {
        let live = live();
        let jeton = CancelToken::new();
        // Délai nul : franchi dès le premier sondage ; dix secondes : jamais franchi ici.
        let (franchi, restant) = (Duration::ZERO, Duration::from_secs(10));
        // Jeton vierge, dans le délai : on attend.
        assert_eq!(stop_reason(&live, Some(&jeton), Instant::now(), restant), None);
        assert_eq!(stop_reason(&live, None, Instant::now(), restant), None);
        // Délai dépassé.
        assert_eq!(stop_reason(&live, None, Instant::now(), franchi), Some(End::TimedOut));
        // L'annulation passe avant le délai.
        jeton.cancel();
        assert_eq!(stop_reason(&live, Some(&jeton), Instant::now(), franchi), Some(End::Cancelled));
        assert_eq!(stop_reason(&live, Some(&jeton), Instant::now(), restant), Some(End::Cancelled));
        // L'arrêt demandé passe avant tout.
        live.request_kill();
        assert_eq!(stop_reason(&live, Some(&jeton), Instant::now(), franchi), Some(End::Killed));
        assert_eq!(stop_reason(&live, None, Instant::now(), restant), Some(End::Killed));
    }

    #[test]
    fn le_point_d_etape_sans_jeton_ne_fait_rien() {
        report_progress(Instant::now());
        if let Some(vieux) = Instant::now().checked_sub(Duration::from_secs(12)) {
            report_progress(vieux);
        }
    }

    #[test]
    fn un_process_qui_finit_avant_le_delai_rend_son_code_sans_attendre_le_delai() {
        // Une seconde de travail, soixante de délai : la fin est vue au sondage suivant.
        let mut child = travaille_puis_sort(1, 7).spawn().unwrap();
        let live = live();
        let started = Instant::now();
        let (code, end) = until_done(&mut child, &live, Duration::from_secs(60), None);
        assert_eq!((code, end), (Some(7), End::Exited));
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "l'attente a duré le délai entier : {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn a_l_expiration_tout_l_arbre_est_tue_pas_seulement_le_parent() {
        let mut child = arbre_dormant().stdout(Stdio::piped()).spawn().unwrap();
        // Le PID du petit-fils arrive sur stdout : lu dans un fil, attendu ici avec bornage.
        let (tx, rx) = mpsc::channel();
        let mut sortie = BufReader::new(child.stdout.take().unwrap());
        std::thread::spawn(move || {
            let mut ligne = String::new();
            let pid = if sortie.read_line(&mut ligne).is_ok_and(|n| n > 0) {
                pid_annonce(&ligne)
            } else {
                None
            };
            let _ = tx.send(pid);
        });
        let petit_fils = rx
            .recv_timeout(Duration::from_secs(60))
            .ok()
            .flatten()
            .expect("PID du petit-fils introuvable sur stdout");
        // L'arbre tourne bien avant l'arrêt (sinon le test ne prouverait rien).
        assert!(pid_alive(petit_fils), "le petit-fils devait tourner avant l'arrêt");
        let live = live();
        let (_, end) = until_done(&mut child, &live, Duration::from_millis(500), None);
        assert_eq!(end, End::TimedOut);
        // Recherche bornée : le « vivant » de tout à l'heure peut venir du cache (une seconde).
        let deadline = Instant::now() + Duration::from_secs(10);
        while pid_alive(petit_fils) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        let survivant = pid_alive(petit_fils);
        if survivant {
            // Ne rien laisser tourner 30 s si le test échoue.
            kill_tree(petit_fils);
        }
        assert!(!survivant, "le petit-fils {petit_fils} survit à l'arrêt de l'arbre");
    }
}
