//! État mutable d'un run vivant.

use super::output_buffer::OutputBuffer;
use super::{Outcome, Status};

pub struct State {
    pub pid: Option<u32>,
    pub status: Status,
    pub exit_code: Option<i32>,
    pub session_id: Option<String>,
    pub output: OutputBuffer,
    pub last_output_at: u64,
    pub finished_at: Option<u64>,
    pub outcome: Option<Outcome>,
}

impl State {
    pub fn new(now: u64) -> Self {
        State {
            pid: None,
            status: Status::Running,
            exit_code: None,
            session_id: None,
            output: OutputBuffer::default(),
            last_output_at: now,
            finished_at: None,
            outcome: None,
        }
    }
}
