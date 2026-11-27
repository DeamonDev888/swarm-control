//! Action `kill` : arrêt d'un run vivant.

use super::snapshot::to_json;
use crate::registry::{LiveRun, Status};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

const GRACE: Duration = Duration::from_secs(10);

pub fn kill(run: &LiveRun) -> Value {
    if run.snapshot().status != Status::Running {
        return json!({ "killed": false, "reason": "le run n'est pas en cours", "run": to_json(&run.snapshot()) });
    }
    run.request_kill();
    let deadline = Instant::now() + GRACE;
    while run.snapshot().status == Status::Running && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
    let snap = run.snapshot();
    json!({ "killed": snap.status != Status::Running, "run": to_json(&snap) })
}
