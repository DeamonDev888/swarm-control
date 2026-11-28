//! Outil `reload` : relit à chaud les `.env` du workspace (PLAN §4.8).
//!
//! Même chemin que le début de chaque tour ([`crate::config::reload_env`]) et que la commande
//! `reload` : le `.env` du workspace et `EXTERNAL_ENV_PATH` sont relus si leur date de
//! modification a changé, l'env système garde la priorité. `reloaded` vaut false si rien n'a
//! changé.

use crate::config;
use litemcp::prelude::*;
use litemcp::zod;
use serde_json::{Value, json};

const DESCRIPTION: &str = "Relit à chaud les .env du workspace et EXTERNAL_ENV_PATH (jetons, modèle, \
    variables de l'agent…) : pris en compte dès le prochain tour, sans redémarrer. L'environnement \
    système garde la priorité. reloaded vaut false si rien n'a changé.";

pub fn register(builder: ServerBuilder) -> ServerBuilder {
    builder
        .tool_parse("reload", DESCRIPTION, zod::object().strict(), handle)
        .annotate("reload", ToolAnnotations::new().title("Recharger les .env").read_only(false))
}

fn handle(_: Value) -> ToolResult {
    Ok(json!({ "reloaded": config::reload_env() }))
}

#[cfg(test)]
mod tests {
    use super::handle;
    use serde_json::json;

    /// Le handler rend exactement `{reloaded: bool}` — la valeur suit l'état des fichiers
    /// chauds du process (config globale), le contrat est sa forme.
    #[test]
    fn handle_returns_a_single_reloaded_flag() {
        let first = handle(json!({})).unwrap();
        let second = handle(json!({})).unwrap();
        assert_eq!(first.as_object().map(|o| o.len()), Some(1), "une seule clé : {first}");
        assert!(first["reloaded"].is_boolean(), "{first}");
        // Sans écriture entre les deux appels : le second ne recharge rien.
        assert_eq!(second, json!({ "reloaded": false }), "rien n'a changé : {second}");
    }
}
