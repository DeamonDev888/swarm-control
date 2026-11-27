//! Arguments de `agent_control`.

use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlArgs {
    pub action: String,
    pub agent_name: Option<String>,
    pub run_id: Option<String>,
    pub runner: Option<String>,
    pub config: Option<String>,
    pub timeout_ms: Option<u64>,
    pub since_timestamp: Option<u64>,
}
