//! Description d'un agent résident, rendue par `agent_start`.

use serde_json::{Value, json};

#[derive(Clone, Debug)]
pub struct HostInfo {
    pub agent: String,
    pub runner: String,
    /// Nom annoncé dans le registre LiteMCP (`swarm-<agent>`).
    pub name: String,
    pub url: String,
    pub instance: String,
}

impl HostInfo {
    pub fn to_json(&self) -> Value {
        json!({
            "agent": self.agent,
            "runner": self.runner,
            "name": self.name,
            "url": self.url,
            "instance": self.instance,
        })
    }
}
