//! Localisation des CLI externes pilotés (`claude`, `hermes`).

mod claude;
mod hermes;
mod path_lookup;

pub use claude::claude_binary;
pub use hermes::hermes_binary;
