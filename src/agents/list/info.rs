//! Fiche résumée d'un agent.

use crate::agents::Runner;

pub struct AgentInfo {
    pub name: String,
    pub runner: Runner,
    pub model: String,
    pub mcp_servers: Vec<String>,
    pub prompt_bytes: u64,
    pub missing_config: bool,
}
