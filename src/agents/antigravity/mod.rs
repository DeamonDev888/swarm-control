//! Agents Antigravity (`agy`) sur disque : `.antigravity/agents/<n>.md`,
//! `.antigravity/settings_<n>.json`.

pub mod configs;
pub mod create;
pub mod delete;
pub mod update;

#[cfg(test)]
mod tests;
