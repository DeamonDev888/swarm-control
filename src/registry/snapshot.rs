//! Vue figée d'un run, pour l'affichage.

use super::{Outcome, Status};

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub run_id: String,
    pub runner: &'static str,
    pub agent: Option<String>,
    pub pid: Option<u32>,
    pub status: Status,
    pub exit_code: Option<i32>,
    pub session_id: Option<String>,
    pub started_at: u64,
    pub last_output_at: u64,
    pub outcome: Option<Outcome>,
}
