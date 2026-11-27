//! Agents Hermes : profils natifs pilotés par le CLI `hermes`.
//!
//! Un profil vit dans `…/hermes/profiles/<nom>/` (config.yaml, SOUL.md, .env,
//! auth.json, state.db…). Ses credentials lui sont **propres**.

mod cli;
pub mod configs;
pub mod create;
mod credentials;
pub mod delete;
mod env_edit;
mod exec;
mod file_rewrite;
pub(crate) mod mcp;
mod provider;
pub mod update;
pub(crate) mod yaml;

pub use yaml::{yaml_mcp_servers, yaml_model};
