//! Lecture des flux stdout / stderr dans des fils dédiés.

use super::utf8::take_utf8;
use crate::registry::LiveRun;
use std::io::Read;
use std::process::Child;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Plafond du tampon final par flux (fin de flux conservée).
const MAX_CAPTURE: usize = 10 * 1024 * 1024;
/// Un petit-enfant détaché peut garder un pipe ouvert : attente bornée.
const JOIN_GRACE: Duration = Duration::from_secs(3);

pub type Capture = Arc<Mutex<Vec<u8>>>;

pub fn new_capture() -> Capture {
    Arc::new(Mutex::new(Vec::new()))
}

/// Démarre les lecteurs des deux flux du process.
pub fn start(
    child: &mut Child,
    live: &Arc<LiveRun>,
    out: &Capture,
    err: &Capture,
) -> Vec<JoinHandle<()>> {
    [
        child.stdout.take().map(|s| spawn(s, Arc::clone(live), Arc::clone(out))),
        child.stderr.take().map(|s| spawn(s, Arc::clone(live), Arc::clone(err))),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// Attend la fin des lecteurs, au plus [`JOIN_GRACE`].
pub fn join_all(handles: Vec<JoinHandle<()>>) {
    let deadline = Instant::now() + JOIN_GRACE;
    for handle in handles {
        while !handle.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        if handle.is_finished() {
            let _ = handle.join();
        }
    }
}

pub fn text(capture: &Capture) -> String {
    let bytes = capture.lock().unwrap_or_else(|e| e.into_inner());
    String::from_utf8_lossy(&bytes).into_owned()
}

fn spawn<R: Read + Send + 'static>(
    mut src: R,
    live: Arc<LiveRun>,
    sink: Capture,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        let mut carry: Vec<u8> = Vec::new();
        loop {
            let n = match src.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            append_capped(&sink, &buf[..n]);
            carry.extend_from_slice(&buf[..n]);
            live.push_output(&take_utf8(&mut carry));
        }
        if !carry.is_empty() {
            live.push_output(&String::from_utf8_lossy(&carry));
        }
    })
}

fn append_capped(sink: &Capture, chunk: &[u8]) {
    let mut s = sink.lock().unwrap_or_else(|e| e.into_inner());
    s.extend_from_slice(chunk);
    if s.len() > MAX_CAPTURE {
        let excess = s.len() - MAX_CAPTURE;
        s.drain(..excess);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        JOIN_GRACE, MAX_CAPTURE, append_capped, join_all, new_capture, spawn, start, text,
    };
    use crate::registry::LiveRun;
    use std::io::{ErrorKind, Read};
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::{Duration, Instant};

    /// Un run anonyme sur un chemin jamais écrit : rien ne touche au disque ici.
    fn live() -> Arc<LiveRun> {
        Arc::new(LiveRun::new("run-lecteurs".into(), "claude", None, Path::new("."), 1))
    }

    /// Lecteur qui rend ses morceaux l'un après l'autre : un caractère UTF-8 peut
    /// ainsi être coupé entre deux `read`, comme sur un vrai pipe.
    struct Morceaux(Vec<Vec<u8>>);

    impl Read for Morceaux {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let Some(chunk) = self.0.first_mut() else { return Ok(0) };
            let n = chunk.len().min(buf.len());
            buf[..n].copy_from_slice(&chunk[..n]);
            chunk.drain(..n);
            if chunk.is_empty() {
                self.0.remove(0);
            }
            Ok(n)
        }
    }

    /// Lecteur qui met `delai` à rendre la main, puis ferme le flux.
    struct Lent(Duration);

    impl Read for Lent {
        fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
            std::thread::sleep(self.0);
            Ok(0)
        }
    }

    /// Lecteur qui rend `acquis` une fois, puis une erreur (tube cassé) : jamais de fin
    /// de flux explicite.
    struct Casse(Option<Vec<u8>>);

    impl Read for Casse {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let Some(acquis) = self.0.take() else {
                return Err(ErrorKind::BrokenPipe.into());
            };
            buf[..acquis.len()].copy_from_slice(&acquis);
            Ok(acquis.len())
        }
    }

    /// Commande système inoffensive : écrit `out` sur stdout, `err` sur stderr, puis
    /// sort avec `code`.
    fn dit(out: &str, err: &str, code: i32) -> Command {
        let mut cmd = if cfg!(windows) {
            let script = format!("echo {out}& echo {err} 1>&2& exit {code}");
            let mut cmd = crate::process::command(Path::new("cmd"));
            cmd.args(["/C", &script]);
            cmd
        } else {
            let script = format!("echo {out}; echo {err} 1>&2; exit {code}");
            let mut cmd = crate::process::command(Path::new("sh"));
            cmd.args(["-c", &script]);
            cmd
        };
        cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        cmd
    }

    static RANG: AtomicU32 = AtomicU32::new(0);

    /// Dossier temporaire propre à un test (pid + rang), supprimé à la fin.
    struct Dossier(PathBuf);

    impl Dossier {
        fn new(tag: &str) -> Self {
            let n = RANG.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir()
                .join(format!("swarm-lecteurs-{tag}-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Dossier(dir)
        }

        fn chemin(&self, nom: &str) -> PathBuf {
            self.0.join(nom)
        }
    }

    impl Drop for Dossier {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Commande système inoffensive : écrit `nom` (dans `dossier`) sur stdout, octet pour
    /// octet — une ligne de la longueur voulue, ou des octets invalides en UTF-8.
    fn affiche(dossier: &Dossier, nom: &str) -> Command {
        let mut cmd = if cfg!(windows) {
            let mut cmd = crate::process::command(Path::new("cmd"));
            cmd.args(["/C", "type", nom]);
            cmd
        } else {
            let mut cmd = crate::process::command(Path::new("cat"));
            cmd.arg(nom);
            cmd
        };
        cmd.current_dir(&dossier.0)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        cmd
    }

    /// Commande système inoffensive : 300 lignes d'environ 600 octets sur chaque flux, en
    /// alternance — près de 200 Kio par flux, bien au-delà du tampon d'un tube. Sans
    /// lecteur sur un flux, le process resterait bloqué dessus.
    fn gros_volume() -> Command {
        let bourre = "x".repeat(600);
        let mut cmd = if cfg!(windows) {
            let script = format!(
                "for /L %i in (1,1,300) do @(echo O-%i-{bourre} & echo E-%i-{bourre} 1>&2)"
            );
            let mut cmd = crate::process::command(Path::new("cmd"));
            cmd.args(["/C", &script]);
            cmd
        } else {
            let script = format!(
                "i=1; while [ $i -le 300 ]; do echo \"O-$i-{bourre}\"; echo \"E-$i-{bourre}\" 1>&2; i=$((i+1)); done"
            );
            let mut cmd = crate::process::command(Path::new("sh"));
            cmd.args(["-c", &script]);
            cmd
        };
        cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        cmd
    }

    /// Attend la fin du process au plus `delai`, par sondage ; au-delà, le tue pour qu'un
    /// test en échec ne laisse rien tourner.
    fn attendre_fin(child: &mut Child, delai: Duration) -> bool {
        let deadline = Instant::now() + delai;
        loop {
            match child.try_wait() {
                Ok(Some(_)) => return true,
                Ok(None) => {}
                Err(_) => return false,
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn le_tampon_rend_son_texte_et_remplace_l_invalide() {
        let capture = new_capture();
        assert_eq!(text(&capture), "");
        capture.lock().unwrap().extend_from_slice("déjà".as_bytes());
        assert_eq!(text(&capture), "déjà");
        capture.lock().unwrap().extend_from_slice(&[0xFF]);
        assert_eq!(text(&capture), "déjà\u{FFFD}");
    }

    #[test]
    fn au_dela_du_plafond_le_tampon_garde_la_fin_du_flux() {
        let capture = new_capture();
        append_capped(&capture, &vec![b'a'; MAX_CAPTURE]);
        append_capped(&capture, b"queue");
        let bytes = capture.lock().unwrap().clone();
        assert_eq!(bytes.len(), MAX_CAPTURE, "plafond par flux");
        assert!(bytes.ends_with(b"queue"), "la fin du flux est conservée");
        assert!(bytes.starts_with(b"aaa"), "le début jeté, la suite gardée");
        // Sous le plafond, rien n'est jeté.
        let petit = new_capture();
        append_capped(&petit, b"abc");
        assert_eq!(text(&petit), "abc");
    }

    #[test]
    fn un_lecteur_journalise_le_flux_et_recolle_un_caractere_coupe() {
        let live = live();
        let sink = new_capture();
        let é = "é".as_bytes();
        let lecteur = Morceaux(vec![vec![b'a', é[0]], vec![é[1], b'b']]);
        spawn(lecteur, Arc::clone(&live), Arc::clone(&sink)).join().unwrap();
        assert_eq!(text(&sink), "aéb");
        // Le journal en direct ne contient aucun caractère de remplacement.
        assert_eq!(live.output_since(None), "aéb");
    }

    #[test]
    fn un_lecteur_qui_ne_rend_rien_ne_journalise_rien() {
        let live = live();
        let sink = new_capture();
        spawn(Morceaux(Vec::new()), Arc::clone(&live), Arc::clone(&sink)).join().unwrap();
        assert_eq!(text(&sink), "");
        assert_eq!(live.output_since(None), "");
    }

    #[test]
    fn les_deux_flux_d_un_process_sont_lus_jusqu_a_la_fin() {
        let mut child = dit("sortie-standard", "erreur-standard", 0)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let live = live();
        let (out, err) = (new_capture(), new_capture());
        let handles = start(&mut child, &live, &out, &err);
        assert_eq!(handles.len(), 2, "un fil par flux");
        child.wait().unwrap();
        join_all(handles);
        assert!(text(&out).contains("sortie-standard"), "{}", text(&out));
        assert!(!text(&out).contains("erreur-standard"), "{}", text(&out));
        assert!(text(&err).contains("erreur-standard"), "{}", text(&err));
        let journal = live.output_since(None);
        assert!(
            journal.contains("sortie-standard") && journal.contains("erreur-standard"),
            "{journal}"
        );
    }

    #[test]
    fn sans_flux_le_demarrage_ne_cree_aucun_lecteur() {
        let mut child = dit("sans-tube", "rien", 0).spawn().unwrap();
        let live = live();
        let (out, err) = (new_capture(), new_capture());
        assert!(start(&mut child, &live, &out, &err).is_empty());
        child.wait().unwrap();
        assert_eq!(text(&out), "");
        assert_eq!(text(&err), "");
    }

    #[test]
    fn join_all_abandonne_un_lecteur_qui_ne_finit_pas() {
        let live = live();
        let sink = new_capture();
        let handles = vec![spawn(Lent(Duration::from_secs(10)), live, Arc::clone(&sink))];
        let started = Instant::now();
        join_all(handles);
        let took = started.elapsed();
        assert!(
            took >= JOIN_GRACE - Duration::from_millis(100)
                && took < JOIN_GRACE + Duration::from_secs(5),
            "attente bornée à {JOIN_GRACE:?}, prise {took:?}"
        );
        assert_eq!(text(&sink), "");
    }

    #[test]
    fn un_volume_superieur_au_tampon_du_tube_sur_les_deux_flux_ne_bloque_pas() {
        let mut child =
            gros_volume().stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        let live = live();
        let (out, err) = (new_capture(), new_capture());
        let handles = start(&mut child, &live, &out, &err);
        assert_eq!(handles.len(), 2, "un fil par flux");
        // Le process est attendu pendant que les fils vident les tubes : sans lecteur sur
        // l'un des deux, il resterait bloqué sur le tube plein.
        assert!(
            attendre_fin(&mut child, Duration::from_secs(30)),
            "le process a bloqué sur un tube plein"
        );
        join_all(handles);
        for (flux, marque) in [(&out, "O"), (&err, "E")] {
            let texte = text(flux);
            let lignes: Vec<&str> = texte.lines().collect();
            assert_eq!(lignes.len(), 300, "flux {marque} : {} lignes sur 300", lignes.len());
            assert!(
                lignes[0].starts_with(&format!("{marque}-1-"))
                    && lignes[299].starts_with(&format!("{marque}-300-")),
                "flux {marque} : début « {} », fin « {} »",
                lignes[0],
                lignes[299]
            );
        }
    }

    #[test]
    fn une_ligne_plus_longue_que_les_tampons_arrive_entiere() {
        let dossier = Dossier::new("ligne-longue");
        // Cent mille octets sans saut de ligne : plus de douze lectures de 8192 octets, et
        // plus que le tampon d'un tube.
        let attendu = format!("debut-{}-fin\nsuite\n", "x".repeat(100_000));
        std::fs::write(dossier.chemin("ligne.txt"), &attendu).unwrap();
        let mut child = affiche(&dossier, "ligne.txt").stdout(Stdio::piped()).spawn().unwrap();
        let live = live();
        let out = new_capture();
        let handles = start(&mut child, &live, &out, &new_capture());
        assert_eq!(handles.len(), 1, "seul stdout est pipé : un seul lecteur");
        assert!(attendre_fin(&mut child, Duration::from_secs(30)));
        join_all(handles);
        assert_eq!(text(&out), attendu, "la ligne doit arriver entière");
        assert_eq!(live.output_since(None), attendu, "idem dans le journal en direct");
    }

    #[test]
    fn une_sortie_non_utf8_signale_la_perte_par_des_caracteres_de_remplacement() {
        let dossier = Dossier::new("non-utf8");
        // 0xFF n'appartient à aucune séquence UTF-8 ; 0xC3, tout en fin de flux, est le début
        // d'une séquence de deux octets que la fermeture du tube coupe.
        let brut: Vec<u8> = [b"avant".as_slice(), &[0xFF], b"apres", &[0xC3]].concat();
        std::fs::write(dossier.chemin("bruit.bin"), &brut).unwrap();
        let mut child = affiche(&dossier, "bruit.bin").stdout(Stdio::piped()).spawn().unwrap();
        let live = live();
        let out = new_capture();
        let handles = start(&mut child, &live, &out, &new_capture());
        assert!(attendre_fin(&mut child, Duration::from_secs(30)));
        join_all(handles);
        // Les octets d'origine restent dans le tampon ; texte et journal signalent chaque
        // perte par U+FFFD.
        assert_eq!(*out.lock().unwrap(), brut, "le tampon garde les octets d'origine");
        assert_eq!(text(&out), "avant\u{FFFD}apres\u{FFFD}");
        assert_eq!(live.output_since(None), "avant\u{FFFD}apres\u{FFFD}");
    }

    #[test]
    fn une_erreur_de_lecture_arrete_le_lecteur_et_garde_l_acquis() {
        let live = live();
        let sink = new_capture();
        let lecteur = spawn(Casse(Some(b"acquis".to_vec())), Arc::clone(&live), Arc::clone(&sink));
        // Attente bornée : un lecteur qui bouclerait sur l'erreur ne finirait jamais.
        let deadline = Instant::now() + Duration::from_secs(10);
        while !lecteur.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(lecteur.is_finished(), "le lecteur doit s'arrêter sur une erreur de lecture");
        lecteur.join().unwrap();
        assert_eq!(text(&sink), "acquis");
        assert_eq!(live.output_since(None), "acquis");
    }
}
