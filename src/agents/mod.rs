//! Registre des agents sur disque.
//!
//! - `claude/` : `.claude/agents/<n>.md`, `settings_<n>.json`, `.mcp.<n>.json`
//! - `hermes/` : profils natifs `…/hermes/profiles/<n>/` pilotés par le CLI
//! - `antigravity/` : `.antigravity/agents/<n>.md`, `settings_<n>.json`
//!
//! Les fichiers de ce niveau sont communs aux runners ; les opérations
//! (`create`, `update`, `delete`, `configs`) y aiguillent vers le bon runner.

mod antigravity;
mod claude;
pub mod compile;
mod configs;
mod create;
mod delete;
pub mod executable;
mod files;
mod hermes;
mod list;
pub mod logo;
mod lookup;
mod mcp;
mod name;
mod paths;
pub(crate) mod peers;
mod runner;
mod secrets;
pub mod templates;
mod update;

pub use configs::get_configs;
pub use create::{CreateArgs, Created, create};
pub use delete::{Deleted, delete};
pub use list::{AgentInfo, list};
pub use lookup::{exists_for, peek_runner, resolve};
pub use name::validate_name;
pub use paths::{
    antigravity_prompt_path, antigravity_settings_path, claude_mcp_path, claude_prompt_path,
    claude_settings_path, global_mcp_path,
};
pub use runner::Runner;
pub use templates::class_of;
pub use update::{Updates, update};
