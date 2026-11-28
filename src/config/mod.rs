//! Configuration globale : workspace, variables d'environnement, chemins.
//!
//! Résolution du workspace : `--workspace` → `SWARM_WORKSPACE` → dossier contenant
//! `.mcp.json` en remontant → `~/.swarm-control`.

mod dotenv;
mod env;
mod hermes_home;
mod interpolate;
mod paths;
/// Garde-fou de publication des agents x et google : écrit et testé, **sans appelant**
/// pour l'instant (le marqueur de tour est à venir, PLAN §5 « Robustesse »).
#[allow(dead_code)]
pub(crate) mod publish;
mod state;
mod timeouts;
mod workspace;

pub use env::{app_var, var};
pub use hermes_home::native_hermes_home;
pub use interpolate::interpolate_json;
pub use paths::home_dir;
pub use state::{get, init, workspace_for};
pub use timeouts::default_timeout;
