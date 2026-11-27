//! Registre des agents sur disque — mêmes fichiers que l'OverMind TS.
//!
//! - `claude/` : `.claude/agents/<n>.md`, `settings_<n>.json`, `.mcp.<n>.json`
//! - `hermes/` : profils natifs `…/hermes/profiles/<n>/` pilotés par le CLI
//!
//! Les fichiers de ce niveau sont communs aux deux runners ; les opérations
//! (`create`, `update`, `delete`, `configs`) y aiguillent vers le bon runner.

mod claude;
mod configs;
mod create;
mod delete;
mod files;
mod hermes;
mod list;
mod lookup;
mod mcp;
mod name;
mod paths;
mod runner;
mod secrets;
mod update;

pub use configs::get_configs;
pub use create::{CreateArgs, Created, create};
pub use delete::{Deleted, delete};
pub use list::{AgentInfo, list};
pub use lookup::{exists_for, peek_runner, resolve};
pub use name::validate_name;
pub use paths::{claude_mcp_path, claude_prompt_path, claude_settings_path, global_mcp_path};
pub use runner::Runner;
pub use update::{Updates, update};
