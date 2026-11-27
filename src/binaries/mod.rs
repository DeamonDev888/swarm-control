//! Localisation des CLI externes pilotés (`claude`, `hermes`, `agy`).

mod antigravity;
mod claude;
mod hermes;
mod path_lookup;

pub use antigravity::antigravity_binary;
pub use claude::claude_binary;
pub use hermes::hermes_binary;
