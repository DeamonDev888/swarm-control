//! Numéro d'instance : `swarm-control-N`, premier numéro libre ou `--instance N`.
//!
//! Le numéro est réservé par un bail (`%TEMP%/swarm-control-instances/N.json`), renouvelé
//! tant que le process vit : deux instances ne portent jamais le même nom.

mod claim;

pub use claim::claim;

/// Nom d'une instance numérotée.
pub fn name(number: u32) -> String {
    format!("swarm-control-{number}")
}
