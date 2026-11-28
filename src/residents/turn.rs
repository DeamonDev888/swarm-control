//! Un tour : une demande faite à l'agent, et sa réponse.

use crate::runners::RunResult;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Compteur processuel des tours : deux tours de la même nanoseconde restent distincts.
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub struct Turn {
    /// Identifiant stable du tour, unique dans le process et lisible dans une URL : c'est le
    /// `turnId` accusé par `POST /send {async:true}`, puis lu sur `GET /turn/<id>`.
    /// Aucun lecteur dans le crate pour l'instant (la route `/turn/<id>` est à venir, §4.2).
    #[allow(dead_code)]
    pub id: String,
    pub prompt: String,
    /// Qui demande (un pair), pour la traçabilité.
    pub from: Option<String>,
    /// Chaîne des agents déjà traversés (outil `ask`), journalisée avec le tour.
    pub via: Vec<String>,
    pub model: Option<String>,
    pub enqueued: Instant,
    /// Levé par l'appelant qui renonce : le tour est alors ignoré s'il n'a pas démarré.
    pub cancelled: Arc<AtomicBool>,
    reply: Sender<TurnOutcome>,
}

pub struct TurnOutcome {
    pub result: RunResult,
    pub queued_ms: u64,
    pub duration_ms: u64,
}

impl Turn {
    pub fn new(
        prompt: String,
        from: Option<String>,
        model: Option<String>,
    ) -> (Turn, Receiver<TurnOutcome>) {
        Self::with_via(prompt, from, model, Vec::new())
    }

    /// Comme [`Turn::new`], en conservant la chaîne `via` demandée par un pair.
    pub fn with_via(
        prompt: String,
        from: Option<String>,
        model: Option<String>,
        via: Vec<String>,
    ) -> (Turn, Receiver<TurnOutcome>) {
        let (reply, outcome) = mpsc::channel();
        let turn = Turn {
            id: Self::next_id(),
            prompt,
            from,
            via,
            model,
            enqueued: Instant::now(),
            cancelled: Arc::new(AtomicBool::new(false)),
            reply,
        };
        (turn, outcome)
    }

    pub fn queued_ms(&self) -> u64 {
        self.enqueued.elapsed().as_millis() as u64
    }

    /// Identifiant d'un nouveau tour : horodatage (nanosecondes, hexadécimal) et rang de
    /// création, séparés par un tiret — que des caractères sûrs dans un chemin d'URL.
    fn next_id() -> String {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let rank = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        format!("{nanos:x}-{rank:x}")
    }

    /// Rend la réponse à l'appelant (s'il est parti, personne ne la lira : sans importance).
    pub fn complete(self, result: RunResult, queued_ms: u64, duration_ms: u64) {
        let _ = self.reply.send(TurnOutcome { result, queued_ms, duration_ms });
    }

    /// Termine le tour sans l'exécuter.
    pub fn fail(self, message: &str) {
        let queued = self.queued_ms();
        self.complete(RunResult::err(message), queued, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::{Turn, TurnOutcome};
    use crate::runners::RunResult;
    use std::sync::Arc;
    use std::sync::atomic::Ordering;

    #[test]
    fn un_tour_neuf_garde_prompt_origine_et_chaine() {
        let (turn, outcome) = Turn::with_via(
            "résume".into(),
            Some("bob".into()),
            Some("sonnet".into()),
            vec!["alice".into()],
        );
        assert_eq!(turn.prompt, "résume");
        assert_eq!(turn.from.as_deref(), Some("bob"));
        assert_eq!(turn.via, ["alice"], "la chaîne du pair est conservée");
        assert_eq!(turn.model.as_deref(), Some("sonnet"));
        assert!(!turn.cancelled.load(Ordering::SeqCst), "neuf : non annulé");
        drop(outcome);
    }

    #[test]
    fn sans_origine_un_tour_est_anonyme_et_sans_chaine() {
        let (turn, _outcome) = Turn::new("salut".into(), None, None);
        assert_eq!((turn.from, turn.model), (None, None));
        assert!(turn.via.is_empty());
    }

    /// L'id est le même pendant toute la vie du tour, distinct d'un tour à l'autre, et ne
    /// contient que des caractères sûrs dans un chemin d'URL (`/turn/<id>`).
    #[test]
    fn un_tour_porte_un_id_stable_unique_et_sur_pour_une_url() {
        let (a, _x) = Turn::new("un".into(), None, None);
        let (b, _y) = Turn::with_via("deux".into(), None, None, Vec::new());
        assert!(!a.id.is_empty());
        assert_ne!(a.id, b.id, "deux tours, deux ids");
        for id in [&a.id, &b.id] {
            assert!(id.bytes().all(|c| c.is_ascii_hexdigit() || c == b'-'), "id {id}");
        }
        let id = a.id.clone();
        drop(a);
        assert_eq!(id.len(), id.trim().len(), "pas de blanc aux extrémités");
    }

    #[test]
    fn le_jeton_d_annulation_est_partage_avec_l_appelant() {
        let (turn, _outcome) = Turn::new("long".into(), None, None);
        let token = Arc::clone(&turn.cancelled);
        assert!(!token.load(Ordering::SeqCst));
        // L'appelant qui renonce lève le jeton : le tour le voit (ignoré s'il n'a pas démarré).
        token.store(true, Ordering::SeqCst);
        assert!(turn.cancelled.load(Ordering::SeqCst), "le tour voit l'appelant renoncer");
    }

    #[test]
    fn complete_rend_l_issue_prevue_a_l_appelant() {
        let (turn, outcome) = Turn::new("question".into(), None, None);
        let queued_ms = turn.queued_ms();
        turn.complete(
            RunResult {
                result: "réponse".into(),
                session_id: Some("s1".into()),
                ..Default::default()
            },
            42,
            7,
        );
        let done: TurnOutcome = outcome.recv().expect("issue reçue");
        assert_eq!(done.result.result, "réponse");
        assert_eq!(done.result.session_id.as_deref(), Some("s1"));
        assert_eq!((done.queued_ms, done.duration_ms), (42, 7));
        assert!(queued_ms <= done.queued_ms, "l'horloge de file ne recule pas");
    }

    #[test]
    fn fail_termine_le_tour_sans_l_executer() {
        let (turn, outcome) = Turn::new("bof".into(), None, None);
        turn.fail("refusé");
        let done = outcome.recv().expect("issue reçue");
        assert_eq!(done.result.error.as_deref(), Some("refusé"));
        assert_eq!(done.duration_ms, 0, "aucun run : durée nulle");
        assert!(done.queued_ms < 2000, "fail ne dort pas");
    }

    #[test]
    fn repondre_a_un_appelant_deja_parti_ne_panique_pas() {
        let (turn, outcome) = Turn::new("oublié".into(), None, None);
        drop(outcome);
        turn.complete(RunResult::default(), 0, 0);
    }
}
