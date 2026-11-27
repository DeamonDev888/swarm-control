//! Synthèse d'un lot de runs parallèles.

use super::Indexed;
use serde_json::{Value, json};
use std::time::Duration;

/// `results` est trié par index de tâche ; `still_running` = tâches non attendues.
pub fn build(
    total: usize,
    mut results: Vec<Indexed>,
    still_running: usize,
    took: Duration,
) -> Value {
    results.sort_by_key(|(i, _)| *i);
    let failed = results.iter().filter(|(_, v)| v.get("error").is_some()).count();
    json!({
        "total": total,
        "completed": results.len(),
        "failed": failed,
        "stillRunning": still_running,
        "durationMs": took.as_millis() as u64,
        "results": results.into_iter().map(|(_, v)| v).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::build;
    use serde_json::json;
    use std::time::Duration;

    #[test]
    fn sorts_by_task_and_counts_failures() {
        let results = vec![(1, json!({"error": "x"})), (0, json!({"result": "ok"}))];
        let v = build(3, results, 1, Duration::from_millis(5));
        assert_eq!(v["results"][0]["result"], "ok");
        assert_eq!(v["failed"], 1);
        assert_eq!(v["completed"], 2);
        assert_eq!(v["stillRunning"], 1);
    }
}
