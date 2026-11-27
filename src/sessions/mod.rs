//! Sessions persistantes d'un agent.
//!
//! La session vit dans le fichier d'état de l'agent (`state/`). L'ancien fichier
//! partagé `bridge/agents.json` de l'OverMind TS reste **lu** (repli), jamais écrit.

mod delete;
mod entry;
mod get;
mod legacy;
mod save;

#[cfg(test)]
mod tests;

pub use delete::delete;
pub use get::get;
pub use save::save;
