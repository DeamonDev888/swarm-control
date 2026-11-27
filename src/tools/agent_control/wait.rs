//! Action `wait` : bloque jusqu'à la fin d'un run.

use super::args::ControlArgs;
use super::snapshot::to_json;
use crate::registry::{LiveRun, Status};
use litemcp::cancel::CancelToken;
use litemcp::prelude::{ToolError, ToolResult};
use std::time::{Duration, Instant};

const DEFAULT_MS: u64 = 900_000;

pub fn wait(run: &LiveRun, args: &ControlArgs) -> ToolResult {
    let timeout =
        Duration::from_millis(args.timeout_ms.unwrap_or(DEFAULT_MS).clamp(1_000, 3_600_000));
    let deadline = Instant::now() + timeout;
    let cancel = litemcp::cancel::token();
    while run.snapshot().status == Status::Running {
        if Instant::now() >= deadline {
            return Err(ToolError::timeout(format!(
                "délai {}ms atteint : l'agent tourne toujours",
                timeout.as_millis()
            )));
        }
        if cancel.as_ref().is_some_and(CancelToken::is_cancelled) {
            return Err(ToolError::timeout("annulé par le client"));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Ok(to_json(&run.snapshot()))
}
