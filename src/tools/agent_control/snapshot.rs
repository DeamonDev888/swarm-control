//! Un run au format JSON de l'outil.

use crate::registry::Snapshot;
use crate::store::now_ms;
use serde_json::{Value, json};

pub fn to_json(s: &Snapshot) -> Value {
    let mut v = json!({
        "runId": s.run_id,
        "runner": s.runner,
        "agentName": s.agent,
        "pid": s.pid,
        "status": s.status.as_str(),
        "exitCode": s.exit_code,
        "sessionId": s.session_id,
        "startedAt": s.started_at,
        "lastOutputAt": s.last_output_at,
        "elapsedMs": now_ms().saturating_sub(s.started_at),
    });
    if let Some(o) = &s.outcome {
        v["result"] = json!(o.result);
        if let Some(e) = &o.error {
            v["error"] = json!(e);
        }
    }
    v
}
