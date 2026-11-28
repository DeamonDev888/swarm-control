//! Délais d'exécution.

use super::env;
use std::time::Duration;

/// Délai par défaut d'un run d'agent : 15 min.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// `SWARM_TIMEOUT_MS` si défini, sinon 15 min.
pub fn default_timeout() -> Duration {
    env::app_var("TIMEOUT_MS")
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or(DEFAULT_TIMEOUT)
}
