//! Sessions persistantes d'un agent.
//!
//! La session vit dans le fichier d'état de l'agent (`state/`).

mod delete;
mod entry;
mod get;
mod save;

#[cfg(test)]
mod tests;

pub use delete::delete;
pub use get::get;
pub use save::save;
