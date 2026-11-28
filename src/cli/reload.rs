//! `reload` : relit à chaud les `.env` du workspace d'un agent résident (PLAN §4.8).
//!
//! Trois cas, comme `status` :
//! 1. l'agent est hébergé par ce process → `config::reload_env` (même table que ses tours) ;
//! 2. un pair vivant est annoncé → l'outil `reload` de son serveur ;
//! 3. sinon → erreur : l'agent n'est pas résident (un agent non résident relit déjà ses
//!    fichiers à chaque tour, il n'y a rien à recharger).
//!
//! Affiche `{agent, reloaded}` (`--json`) ou une ligne.

use super::args::Options;
use super::output;
use super::target;
use crate::config;
use crate::peers::{self, CallError};
use serde_json::{Value, json};
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(30);

pub fn run(options: &Options, agent: &str) -> Result<(), String> {
    target::validate(agent)?;
    let reloaded = if target::hosted(agent, options.runner).is_some() {
        config::reload_env()
    } else if let Some(peer) = target::peer(agent) {
        call(&peer.url, agent)?
    } else {
        return Err(format!(
            "agent '{agent}' non résident : ni hébergé par ce process, ni annoncé vivant. Rien à \
             recharger — un agent non résident relit ses .env à chaque tour ; démarre-le \
             (`serve --agent {agent}`) pour le recharger à chaud."
        ));
    };
    if options.json {
        output::json(&json!({ "agent": agent, "reloaded": reloaded }));
    } else if reloaded {
        println!(
            "{agent} : .env rechargés (variables modifiées), pris en compte dès le prochain tour."
        );
    } else {
        println!("{agent} : .env inchangés.");
    }
    Ok(())
}

/// L'outil `reload` d'un pair : `{reloaded: bool}` ; absent ou illisible → faux.
fn call(url: &str, agent: &str) -> Result<bool, String> {
    match peers::call_tool(url, "reload", json!({}), WAIT) {
        Ok(value) => Ok(reloaded_of(&value)),
        Err(CallError::Tool(message) | CallError::Protocol(message)) => {
            Err(format!("agent '{agent}' ({url}): {message}"))
        }
        Err(CallError::Unreachable(why)) => {
            Err(format!("agent '{agent}' annoncé mais injoignable ({why})."))
        }
    }
}

/// `reloaded` d'un document `reload` : faux s'il manque ou n'est pas un booléen.
fn reloaded_of(value: &Value) -> bool {
    value.get("reloaded").and_then(Value::as_bool).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{reloaded_of, run};
    use crate::cli::args::Options;
    use serde_json::json;

    #[test]
    fn reads_the_reloaded_flag_of_a_peer_document() {
        assert!(reloaded_of(&json!({ "reloaded": true })));
        assert!(!reloaded_of(&json!({ "reloaded": false })));
        assert!(!reloaded_of(&json!({})), "absent : rien à affirmer");
        assert!(!reloaded_of(&json!({ "reloaded": "oui" })));
    }

    /// Ni hébergé ici, ni annoncé (nom unique dans le registre) : erreur claire, avant tout
    /// appel ; un nom invalide est refusé le premier.
    #[test]
    fn un_agent_non_resident_est_une_erreur_claire() {
        let options = Options::default();
        let err = run(&options, &format!("fantome-{}", std::process::id())).unwrap_err();
        assert!(err.contains("non résident"), "{err}");
        assert!(err.contains("serve --agent"), "{err}");
        let err = run(&options, "../évasion").unwrap_err();
        assert!(err.starts_with("agent:"), "{err}");
    }
}
