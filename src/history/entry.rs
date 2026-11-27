//! Une ligne du journal : un tour, réussi ou non.

use serde::{Deserialize, Serialize};

/// Un tour d'agent.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// Horodatage de fin de tour (ms depuis l'époque).
    pub ts: u64,
    /// Qui a demandé (`discord:…`, `swarm-control-1`, un pair…).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    /// Chaîne des agents traversés (outil `ask`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub via: Vec<String>,
    pub prompt: String,
    #[serde(default)]
    pub reply: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default)]
    pub duration_ms: u64,
    #[serde(default)]
    pub queued_ms: u64,
    #[serde(default)]
    pub runner: String,
    #[serde(default)]
    pub agent: String,
}

impl Entry {
    /// Le tour a-t-il réussi ?
    pub fn ok(&self) -> bool {
        self.error.is_none()
    }
}
