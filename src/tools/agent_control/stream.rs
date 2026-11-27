//! Action `stream` : sortie en direct d'un run.

use super::args::ControlArgs;
use crate::registry::{LiveRun, Status};
use crate::store::now_ms;
use serde_json::{Value, json};

pub fn stream(run: &LiveRun, args: &ControlArgs) -> Value {
    let snap = run.snapshot();
    json!({
        "runId": snap.run_id,
        "status": snap.status.as_str(),
        "isComplete": snap.status != Status::Running,
        "output": run.output_since(args.since_timestamp),
        "timestamp": now_ms(),
        "sessionId": snap.session_id,
    })
}
