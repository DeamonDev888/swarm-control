//! Garde-fou de publication : rien ne sort au nom de l'utilisateur tant que
//! `SWARM_ALLOW_PUBLISH=1` n'est pas posé (PLAN §5 « Robustesse et sécurité »).
//!
//! Destiné aux agents **x** et **google** (pas encore implémentés) : tout ce qui publie ou
//! envoie — post sur X, réponse à une mention, mail Gmail, événement d'agenda — appelle
//! [`guard`] **avant** d'agir, avec l'action nommée en clair :
//!
//! ```ignore
//! // Dans un agent x ou google, juste avant la publication :
//! crate::config::publish::guard("post X")?; // Err tant que la variable n'est pas posée
//! // … puis seulement : publier.
//! ```
//!
//! - Variable absente (défaut) : refus. L'agent travaille en **brouillon** — il prépare,
//!   l'humain relit et publie lui-même.
//! - Seule la valeur exactement `1` autorise. `0`, `true`, `yes`, ` 1` refusent : une faute
//!   de frappe ou un `true` recopié d'ailleurs ne doit pas ouvrir la porte.
//! - La variable est lue par [`super::var`] : environnement du process d'abord, puis
//!   `.env` déjà chargés. Elle se pose dans l'environnement du résident (unit, service) ou
//!   dans le `.env` du workspace — jamais dans la définition d'un agent : un modèle ne doit
//!   pas pouvoir s'autoriser à publier.
//! - Les tests ne posent pas la variable (`set_var` est `unsafe`, interdit par la crate) :
//!   ils passent par [`allowed_from`] et [`guard_with`], formes pures.

use super::var;

/// La variable qui lève le garde-fou.
pub const ALLOW_VAR: &str = "SWARM_ALLOW_PUBLISH";

/// Publication autorisée ? Vrai seulement si `SWARM_ALLOW_PUBLISH` vaut exactement `1`.
pub fn allowed() -> bool {
    allowed_from(var(ALLOW_VAR).as_deref())
}

/// [`allowed`] sur une valeur déjà lue : l'environnement est injecté, les tests le fixent.
fn allowed_from(value: Option<&str>) -> bool {
    value == Some("1")
}

/// Passe le garde-fou pour l'action `what` (« post X », « envoi Gmail »…) : `Ok` si la
/// publication est autorisée, sinon `Err` avec un message qui dit quoi poser.
pub fn guard(what: &str) -> Result<(), String> {
    guard_with(what, allowed())
}

/// [`guard`] sur une autorisation déjà calculée (les tests la fixent sans variable).
fn guard_with(what: &str, allowed: bool) -> Result<(), String> {
    if allowed {
        return Ok(());
    }
    Err(format!(
        "refus de « {what} » : publication verrouillée (brouillon par défaut) ; \
         pose {ALLOW_VAR}=1 dans l'environnement pour autoriser."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variable_absente_refuse() {
        assert!(!allowed_from(None));
        let err = guard_with("post X", allowed_from(None)).unwrap_err();
        assert!(err.contains("« post X »"), "{err}");
        assert!(err.contains("SWARM_ALLOW_PUBLISH=1"), "{err}");
        assert!(err.contains("brouillon"), "{err}");
    }

    #[test]
    fn la_valeur_un_autorise() {
        assert!(allowed_from(Some("1")));
        assert_eq!(guard_with("envoi Gmail", allowed_from(Some("1"))), Ok(()));
    }

    #[test]
    fn zero_true_ou_faute_de_frappe_refusent() {
        for valeur in ["", "0", "01", "2", "true", "TRUE", "True", "yes", "on", " 1", "1 "] {
            assert!(!allowed_from(Some(valeur)), "{valeur:?} ne doit pas autoriser");
            assert!(guard_with("événement Calendar", allowed_from(Some(valeur))).is_err());
        }
    }

    #[test]
    fn les_deux_portes_publiques_sont_coherentes_sans_poser_la_variable() {
        // Comme `SWARM_TOKEN` (server/auth.rs) : les tests n'isolent pas l'environnement,
        // ils vérifient seulement que `guard` suit `allowed`.
        let garde = guard("post X");
        if allowed() {
            assert_eq!(garde, Ok(()));
        } else {
            assert!(garde.is_err());
        }
    }
}
