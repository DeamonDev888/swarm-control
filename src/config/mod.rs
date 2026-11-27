//! Configuration globale : workspace, variables d'environnement, chemins.
//!
//! Résolution du workspace : `--workspace` → `SWARM_WORKSPACE` (ou l'ancien
//! `OVERMIND_WORKSPACE`) → dossier contenant `.mcp.json` en remontant →
//! `~/.swarm-control` (ou `~/.overmind` s'il existe déjà).

mod dotenv;
mod env;
mod hermes_home;
mod interpolate;
mod paths;
mod state;
mod timeouts;
mod workspace;

pub use env::{app_var, var};
pub use hermes_home::native_hermes_home;
pub use interpolate::interpolate_json;
pub use paths::home_dir;
pub use state::{get, init, workspace_for};
pub use timeouts::default_timeout;
