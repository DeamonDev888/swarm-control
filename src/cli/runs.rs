//! `runs` : le dernier run de chaque agent, lu dans les fichiers d'état du workspace.
//!
//! Source : `state::list` (`bridge/state/<runner>-<agent>.json`), qui porte `lastRun`
//! (`status`, `pid`, `ts`, `exitCode`…) — utile quand le serveur qui a lancé le run n'est plus là.

use super::args::Options;
use super::output;
use crate::state;
use serde_json::Value;
use std::path::Path;

pub fn run(options: &Options, ws: &Path) -> Result<(), String> {
    let mut docs = state::list(ws);
    docs.sort_by_key(key);
    if options.json {
        output::json(&Value::Array(docs));
        return Ok(());
    }
    if docs.is_empty() {
        println!("aucun run enregistré dans {}.", ws.display());
        return Ok(());
    }
    let rows: Vec<Vec<String>> = docs.iter().map(cells).collect();
    print!("{}", output::table(&["AGENT", "RUNNER", "STATUT", "PID", "FIN"], &rows));
    Ok(())
}

fn key(doc: &Value) -> (String, String) {
    (text(doc, "runner").to_string(), text(doc, "agent").to_string())
}

fn cells(doc: &Value) -> Vec<String> {
    let run = doc.get("lastRun");
    let status = run.and_then(|run| run.get("status")).and_then(Value::as_str).unwrap_or("-");
    let pid = run
        .and_then(|run| run.get("pid"))
        .and_then(Value::as_u64)
        .map_or_else(|| "-".to_string(), |pid| pid.to_string());
    let ts = run
        .and_then(|run| run.get("ts"))
        .and_then(Value::as_u64)
        .map_or_else(|| "-".to_string(), output::stamp);
    vec![text(doc, "agent").to_string(), text(doc, "runner").to_string(), status.into(), pid, ts]
}

fn text<'a>(doc: &'a Value, key: &str) -> &'a str {
    doc.get(key).and_then(Value::as_str).unwrap_or("?")
}

#[cfg(test)]
mod tests {
    use super::{cells, key};
    use serde_json::json;

    #[test]
    fn a_run_row_shows_status_pid_and_end() {
        let doc = json!({
            "agent": "sniper",
            "runner": "claude",
            "lastRun": {"status": "done", "pid": 4212, "ts": 1_600_000_000_000u64},
        });
        assert_eq!(cells(&doc), ["sniper", "claude", "done", "4212", "2020-09-13 12:26:40Z"]);
    }

    #[test]
    fn a_document_without_a_run_stays_readable() {
        let doc = json!({"agent": "bob", "runner": "hermes"});
        assert_eq!(cells(&doc), ["bob", "hermes", "-", "-", "-"]);
        assert_eq!(key(&doc), ("hermes".to_string(), "bob".to_string()));
    }
}
