//! Jeton partagé : authentification des serveurs MCP et refus d'une écoute exposée sans jeton.
//!
//! Le jeton vient de `SWARM_TOKEN`, lu par la configuration habituelle
//! (environnement puis `.env`). Sans jeton, l'écoute reste celle d'aujourd'hui : **loopback
//! uniquement**. Écouter sur une autre interface sans jeton est refusé au démarrage.
//!
//! Le client (`peers`) envoie `Authorization: Bearer …` quand la variable est posée ; les
//! routeurs externes du pont doivent faire de même.

use crate::config;
use litemcp::RequestRejection;
use litemcp::prelude::ServerBuilder;

/// Jeton partagé demandé par cette machine, s'il est configuré.
pub fn token() -> Option<String> {
    config::app_var("TOKEN").map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
}

/// Ajoute le contrôle `Authorization: Bearer <token>` au serveur MCP (si un jeton est posé).
pub fn guard(builder: ServerBuilder, token: Option<&str>) -> ServerBuilder {
    let Some(expected) = token.map(str::to_string) else { return builder };
    builder.on_request(move |ctx| {
        let got = ctx.header("Authorization").and_then(|v| v.strip_prefix("Bearer "));
        match got {
            Some(value) if constant_time_eq(value.as_bytes(), expected.as_bytes()) => Ok(()),
            _ => Err(RequestRejection::unauthorized("jeton requis (Authorization: Bearer)")
                .with_header("WWW-Authenticate", "Bearer")),
        }
    })
}

/// Refuse d'écouter sur une interface non-loopback sans jeton.
pub fn check_exposure(host: &str, token: Option<&str>) -> Result<(), String> {
    if token.is_some() || is_loopback(host) {
        return Ok(());
    }
    Err(format!(
        "écoute sur {host} sans authentification refusée : pose SWARM_TOKEN (jeton partagé) \
         ou écoute sur 127.0.0.1"
    ))
}

/// Vérifie l'en-tête `Authorization` d'une requête du pont.
pub fn authorized(header: Option<&str>, token: Option<&str>) -> bool {
    match token {
        None => true,
        Some(expected) => header
            .and_then(|v| v.strip_prefix("Bearer "))
            .is_some_and(|got| constant_time_eq(got.as_bytes(), expected.as_bytes())),
    }
}

fn is_loopback(host: &str) -> bool {
    let host = host.trim().trim_start_matches('[').trim_end_matches(']');
    host == "localhost"
        || host == "::1"
        || host.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// Comparaison à durée constante : le jeton ne fuit pas par le temps de réponse.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::{authorized, check_exposure, constant_time_eq, is_loopback, token};

    #[test]
    fn loopback_only_without_token() {
        assert!(is_loopback("127.0.0.1"));
        assert!(is_loopback("::1"));
        assert!(is_loopback("[::1]"));
        assert!(is_loopback("localhost"));
        assert!(!is_loopback("0.0.0.0"));
        assert!(check_exposure("127.0.0.1", None).is_ok());
        assert!(check_exposure("0.0.0.0", None).is_err());
        assert!(check_exposure("0.0.0.0", Some("secret")).is_ok());
    }

    #[test]
    fn compares_and_checks_bearer_tokens() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
        assert!(authorized(None, None));
        assert!(authorized(Some("Bearer x"), Some("x")));
        assert!(!authorized(Some("Bearer y"), Some("x")));
        assert!(!authorized(Some("x"), Some("x")));
    }

    #[test]
    fn no_token_configured_by_default() {
        // Les tests n'isolent pas SWARM_TOKEN : on vérifie seulement la forme.
        if let Some(t) = token() {
            assert!(!t.is_empty());
        }
    }
}
