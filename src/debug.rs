//! Journal de diagnostic : `--debug` ou `SWARM_DEBUG=1` (PLAN §4.2).
//!
//! Une ligne sur stderr par événement notable :
//! - requête MCP (client de la CLI, et serveur d'un agent via son crochet `on_call`) ;
//! - commande lancée (`claude`, `hermes`, `agy`), secrets masqués ;
//! - session reprise, ou oubliée avec retente à neuf ;
//! - bail de run pris, ou bail périmé repris.
//!
//! Le diagnostic est silencieux par défaut : chaque appel est un `if` réduit à
//! une lecture atomique quand il est inactif.

use std::fmt::Display;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

static ON: AtomicBool = AtomicBool::new(false);

/// Active le journal depuis `--debug` (ou `SWARM_DEBUG=1`), une fois au démarrage.
pub fn init(flag: bool) {
    if flag || env_on() {
        enable();
    }
}

fn env_on() -> bool {
    matches!(std::env::var("SWARM_DEBUG").as_deref(), Ok("1" | "true" | "TRUE" | "True"))
}

/// Active le diagnostic (option `--debug`).
pub fn enable() {
    ON.store(true, Ordering::SeqCst);
}

/// Diagnostic actif ? L'option `--debug`, ou la variable `SWARM_DEBUG=1`.
pub fn enabled() -> bool {
    ON.load(Ordering::SeqCst) || matches!(std::env::var("SWARM_DEBUG").as_deref(), Ok("1"))
}

/// Une ligne `[debug] <quoi>: <detail>` sur stderr ; sans effet si inactif.
pub fn line(quoi: &str, detail: impl AsRef<str>) {
    if enabled() {
        eprintln!("[debug] {quoi}: {}", detail.as_ref());
    }
}

/// Une ligne de diagnostic brute, préfixée `[debug] ` ; sans effet si inactif.
pub fn log(args: impl Display) {
    if enabled() {
        eprintln!("[debug] {args}");
    }
}

/// Écrit une ligne de diagnostic si le mode est actif (`--debug`, `SWARM_DEBUG=1`).
///
/// ```ignore
/// crate::debug!("bail pris {}", file.display());
/// ```
#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {
        $crate::debug::log(::std::format_args!($($arg)*))
    };
}

/// Une valeur de ligne de commande, masquée si elle sent le secret :
/// `AUTH_TOKEN=sk-…` → `AUTH_TOKEN=***`, un jeton nu (`sk-…`) → `***`.
/// Même heuristique que le masquage des fichiers d'agent (`agents::secrets`) :
/// TOKEN, KEY, SECRET ou PASSWORD dans le nom.
pub fn mask(value: &str) -> String {
    match value.split_once('=') {
        Some((name, secret)) if is_secret_name(name) && !secret.is_empty() => {
            format!("{name}=***")
        }
        _ if value.starts_with("sk-") => "***".to_string(),
        _ => value.to_string(),
    }
}

fn is_secret_name(name: &str) -> bool {
    let upper = name.to_uppercase();
    ["TOKEN", "KEY", "SECRET", "PASSWORD"].iter().any(|word| upper.contains(word))
}

/// Ligne de commande masquée : `programme arg1 arg2…`. L'environnement n'est
/// jamais imprimé (les jetons y vivent) ; seuls les arguments sont filtrés.
pub fn command_line(program: &Path, args: &[String]) -> String {
    let mut parts = vec![program.display().to_string()];
    parts.extend(args.iter().map(|a| mask(a)));
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::{command_line, enable, enabled, mask};
    use std::path::Path;

    #[test]
    fn assignments_with_secret_names_are_masked() {
        assert_eq!(mask("ANTHROPIC_AUTH_TOKEN=sk-vrai"), "ANTHROPIC_AUTH_TOKEN=***");
        assert_eq!(mask("api_key=valeur"), "api_key=***");
        assert_eq!(mask("PASSWORD=chose"), "PASSWORD=***");
    }

    #[test]
    fn bare_tokens_are_masked_and_plain_values_kept() {
        assert_eq!(mask("sk-vrai-jeton"), "***");
        assert_eq!(mask("bonjour"), "bonjour");
        assert_eq!(mask("--resume"), "--resume");
        assert_eq!(mask("CHEMIN=A/B"), "CHEMIN=A/B");
    }

    #[test]
    fn the_command_line_keeps_args_but_masks_secrets() {
        let args = vec!["-p".to_string(), "settings_a.json".to_string()];
        assert_eq!(command_line(Path::new("claude"), &args), "claude -p settings_a.json");
        let secret = vec!["AUTH_TOKEN=sk-vrai".to_string()];
        assert_eq!(command_line(Path::new("x"), &secret), "x AUTH_TOKEN=***");
    }

    #[test]
    fn enabling_is_visible_through_enabled() {
        // La variable `SWARM_DEBUG` n'est pas posée par les tests (`set_var` est
        // unsafe) : on vérifie seulement l'option `--debug`.
        enable();
        assert!(enabled());
    }
}
