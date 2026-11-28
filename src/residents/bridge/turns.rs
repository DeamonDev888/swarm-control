//! Registre des tours accusés d'un pont : `POST /send {async:true}` rend un `turnId` tout de
//! suite, `GET /turn/<id>` lit l'état ici (PLAN §4.2).
//!
//! Le client async ne tient pas le [`Receiver`] du tour : un fil de suivi par tour l'attend et
//! range le résultat ici, que le client HTTP soit encore là ou non — le tour continue. Les
//! tours terminés sont bornés (les plus anciens oubliés) ; un tour en cours ne l'est jamais.

use crate::residents::turn::TurnOutcome;
use serde_json::{Value, json};
use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, MutexGuard};

/// Tours terminés gardés par agent : au-delà, les plus anciens sont oubliés.
const MAX_DONE: usize = 256;

#[derive(Default)]
struct Inner {
    /// `None` = en cours ; `Some(corps)` = terminé (le corps de `GET /turn/<id>`, sans `"state"`).
    turns: HashMap<String, Option<Value>>,
    /// Identifiants terminés, du plus ancien au plus récent.
    done: VecDeque<String>,
}

/// Les tours accusés d'un pont (donc d'un agent), un par [`super::Bridge`].
#[derive(Default)]
pub struct Registry {
    inner: Arc<Mutex<Inner>>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// L'état d'un tour : `None` s'il est inconnu, `Some(None)` s'il court, `Some(Some(corps))`
    /// s'il est terminé.
    pub fn get(&self, id: &str) -> Option<Option<Value>> {
        self.lock().turns.get(id).cloned()
    }

    /// Suit le tour `id` : en cours jusqu'à ce que `outcome` livre son issue, que `shape` met en
    /// forme (le corps de la réponse, un objet JSON). Le tour est connu dès le retour : jamais
    /// de `404` pour un client qui relit tout de suite.
    pub fn track(
        &self,
        id: String,
        outcome: Receiver<TurnOutcome>,
        shape: impl FnOnce(TurnOutcome) -> Value + Send + 'static,
    ) {
        self.lock().turns.insert(id.clone(), None);
        let inner = Arc::clone(&self.inner);
        let suivi = std::thread::Builder::new()
            .name("swarm-bridge-turn".into())
            .spawn(move || {
                let body = match outcome.recv() {
                    Ok(done) => shape(done),
                    // Tour lâché sans réponse (agent arrêté) : même erreur que le client synchrone.
                    Err(_) => json!({"error": "l'agent s'est arrêté avant de répondre"}),
                };
                finish(&inner, &id, body);
            });
        if let Err(e) = suivi {
            finish(&self.inner, &id, json!({"error": format!("suivi du tour impossible: {e}")}));
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Range le résultat d'un tour, puis oublie les plus anciens au-delà de [`MAX_DONE`].
fn finish(inner: &Mutex<Inner>, id: &str, body: Value) {
    let mut inner = inner.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(entry) = inner.turns.get_mut(id) {
        *entry = Some(body);
        inner.done.push_back(id.to_string());
    }
    while inner.done.len() > MAX_DONE {
        if let Some(oldest) = inner.done.pop_front() {
            inner.turns.remove(&oldest);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_DONE, Registry, finish};
    use crate::residents::turn::Turn;
    use crate::runners::RunResult;
    use serde_json::json;
    use std::time::{Duration, Instant};

    /// Attend que le tour `id` soit terminé, et rend son corps.
    fn done(registry: &Registry, id: &str) -> serde_json::Value {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(Some(body)) = registry.get(id) {
                return body;
            }
            assert!(Instant::now() < deadline, "le tour {id} ne se termine pas");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn un_tour_suivi_passe_de_en_cours_a_termine() {
        let registry = Registry::new();
        let (turn, outcome) = Turn::new("salut".into(), None, None);
        let id = turn.id.clone();
        assert_eq!(registry.get(&id), None, "inconnu avant le suivi");
        registry.track(id.clone(), outcome, |done| {
            json!({"result": done.result.result, "session_id": done.result.session_id})
        });
        assert_eq!(registry.get(&id), Some(None), "en cours tant que le tour n'est pas fini");
        turn.complete(
            RunResult { result: "réponse".into(), session_id: Some("s1".into()), ..Default::default() },
            3,
            7,
        );
        let body = done(&registry, &id);
        assert_eq!(body, json!({"result": "réponse", "session_id": "s1"}));
    }

    /// Un tour lâché sans réponse (agent arrêté, tour jamais exécuté) ne reste pas « en cours ».
    #[test]
    fn un_tour_lache_sans_reponse_termine_avec_une_erreur() {
        let registry = Registry::new();
        let (turn, outcome) = Turn::new("oublié".into(), None, None);
        let id = turn.id.clone();
        registry.track(id.clone(), outcome, |_| json!({}));
        drop(turn);
        let body = done(&registry, &id);
        assert_eq!(body["error"], json!("l'agent s'est arrêté avant de répondre"), "{body}");
    }

    #[test]
    fn un_id_inconnu_est_absent() {
        assert_eq!(Registry::new().get("personne"), None);
    }

    /// Les tours terminés sont bornés : les plus anciens disparaissent, les récents restent,
    /// et un tour en cours n'est jamais oublié.
    #[test]
    fn les_tours_termines_sont_bornes_aux_plus_recents() {
        let registry = Registry::new();
        let (turn, outcome) = Turn::new("vivant".into(), None, None);
        registry.track(turn.id.clone(), outcome, |_| json!({}));
        assert_eq!(registry.get(&turn.id), Some(None), "en cours");
        {
            let mut inner = registry.lock();
            for n in 0..MAX_DONE + 5 {
                inner.turns.insert(format!("vieux-{n}"), None);
            }
        }
        for n in 0..MAX_DONE + 5 {
            finish(&registry.inner, &format!("vieux-{n}"), json!({"n": n}));
        }
        let inner = registry.lock();
        // Les 5 plus anciens sont oubliés, les `MAX_DONE` derniers restent.
        assert_eq!(inner.done.len(), MAX_DONE);
        assert_eq!(inner.turns.get("vieux-0"), None);
        assert_eq!(inner.turns.get("vieux-4"), None);
        assert_eq!(inner.turns.get("vieux-5"), Some(&Some(json!({"n": 5}))));
        assert_eq!(inner.turns["vieux-5"], Some(json!({"n": 5})));
        let dernier = format!("vieux-{}", MAX_DONE + 4);
        assert_eq!(inner.turns[&dernier], Some(json!({"n": MAX_DONE + 4})));
        assert_eq!(inner.turns.len(), MAX_DONE + 1, "les terminés + le tour en cours");
        assert_eq!(inner.turns.get(&turn.id), Some(&None), "un tour en cours n'est jamais oublié");
    }
}
