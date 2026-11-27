//! Lecture des variables : environnement système d'abord, puis fichiers `.env`.

use super::state;
use std::collections::HashMap;

/// Préfixes des variables propres à l'outil : le nouveau d'abord, l'ancien ensuite.
const APP_PREFIXES: [&str; 2] = ["SWARM_", "OVERMIND_"];

/// Valeur d'une variable (env système, sinon `.env` chargés).
pub fn var(name: &str) -> Option<String> {
    std::env::var(name).ok().or_else(|| state::dotenv_get(name))
}

/// Variable propre à l'outil : `SWARM_<clé>`, sinon `OVERMIND_<clé>`.
pub fn app_var(key: &str) -> Option<String> {
    APP_PREFIXES.iter().find_map(|p| var(&format!("{p}{key}")))
}

/// Comme [`var`], mais sur une table `.env` pas encore installée (démarrage).
pub fn lookup_in(dotenv: &HashMap<String, String>, name: &str) -> Option<String> {
    std::env::var(name).ok().or_else(|| dotenv.get(name).cloned())
}

/// Comme [`app_var`], sur une table `.env` pas encore installée.
pub fn app_lookup_in(dotenv: &HashMap<String, String>, key: &str) -> Option<String> {
    APP_PREFIXES.iter().find_map(|p| lookup_in(dotenv, &format!("{p}{key}")))
}
