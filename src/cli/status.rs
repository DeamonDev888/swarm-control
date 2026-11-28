//! `status` : état d'un agent — hébergé ici, annoncé ailleurs, ou connu par le disque.
//!
//! Trois sources, dans cet ordre :
//! 1. l'agent est hébergé par ce process → `Ctx::status_json` (le même document que l'outil
//!    `status` de son serveur) ;
//! 2. un pair vivant est annoncé → l'outil `status` de son serveur ;
//! 3. sinon → le disque : `session` (`sessions::get`) et `lastRun` (`registry::disk_entry`),
//!    avec `resident: false`.
//!
//! `rebuildNeeded` (§4.8) est dans les trois documents : `null` tant que l'agent n'a jamais
//! été compilé, sinon vrai quand son binaire dédié n'est pas à la version de l'instance ;
//! le résumé le signale par une ligne « binaire : à recompiler ».

use super::args::Options;
use super::output;
use super::target;
use crate::agents::compile;
use crate::peers::{self, CallError};
use crate::registry;
use crate::sessions;
use serde_json::{Value, json};
use std::path::Path;
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(30);

pub fn run(options: &Options, ws: &Path, agent: &str) -> Result<(), String> {
    target::validate(agent)?;
    if let Some(ctx) = target::hosted(agent, options.runner) {
        return print(options, ctx.status_json());
    }
    if let Some(peer) = target::peer(agent) {
        return match peers::call_tool(&peer.url, "status", json!({}), WAIT) {
            Ok(value) => print(options, value),
            Err(CallError::Tool(message) | CallError::Protocol(message)) => {
                Err(format!("agent '{agent}' ({}): {message}", peer.url))
            }
            Err(CallError::Unreachable(why)) => {
                Err(format!("agent '{agent}' annoncé mais injoignable ({why})."))
            }
        };
    }
    disk(options, ws, agent)
}

fn disk(options: &Options, ws: &Path, agent: &str) -> Result<(), String> {
    let entry = registry::disk_entry(ws, agent, options.runner.map(|r| r.as_str()));
    let resolved = target::known_runner(ws, agent, options.runner);
    let runner =
        entry.as_ref().and_then(|(runner, _)| crate::agents::Runner::parse(runner)).or(resolved);
    let Some(runner) = runner else {
        return Err(format!(
            "agent '{agent}' introuvable : ni hébergé ici, ni annoncé, ni connu du disque."
        ));
    };
    let session = sessions::get(ws, runner.as_str(), agent);
    let last_run = entry.map(|(_, run)| run).unwrap_or(Value::Null);
    print(
        options,
        json!({
            "agent": agent,
            "runner": runner.as_str(),
            "resident": false,
            "session": session,
            "lastRun": last_run,
            "rebuildNeeded": compile::rebuild_needed_value(ws, agent),
        }),
    )
}

fn print(options: &Options, value: Value) -> Result<(), String> {
    if options.json {
        output::json(&value);
    } else {
        print!("{}", summary(&value));
    }
    Ok(())
}

/// Résumé lisible d'un document `status` (hébergé, pair ou disque).
pub fn summary(value: &Value) -> String {
    let mut lines =
        vec![format!("agent    : {} ({})", text(value, "agent"), text(value, "runner"))];
    let name = value.get("name").and_then(Value::as_str);
    let url = value.get("url").and_then(Value::as_str);
    if let (Some(name), Some(url)) = (name, url) {
        lines.push(format!("serveur  : {url} · {name} · instance {}", text(value, "instance")));
    } else {
        let resident = value.get("resident").and_then(Value::as_bool).unwrap_or(false);
        lines.push(format!("résident : {}", if resident { "oui" } else { "non" }));
    }
    if let Some(state) = value.get("state").and_then(Value::as_str) {
        let label = if state == "busy" { "occupé" } else { "inactif" };
        let queue = value.get("queueLength").and_then(Value::as_u64).unwrap_or(0);
        let mut line = format!("état     : {label} · file {queue}");
        if let Some(turns) = value.get("turnsDone").and_then(Value::as_u64) {
            line.push_str(&format!(" · {turns} tour(s)"));
        }
        lines.push(line);
        if let Some(current) = value.get("current").filter(|current| !current.is_null()) {
            let who = current.get("from").and_then(Value::as_str).unwrap_or("?");
            let elapsed = current.get("elapsedMs").and_then(Value::as_u64).unwrap_or(0);
            lines.push(format!("en cours : demandé par {who} · {}", output::duration(elapsed)));
        }
    }
    lines.push(format!(
        "session  : {}",
        value.get("session").and_then(Value::as_str).unwrap_or("(aucune)")
    ));
    if let Some(last) = value.get("lastTurn").filter(|last| !last.is_null()) {
        let ok = last.get("ok").and_then(Value::as_bool).unwrap_or(false);
        let mut line = String::from(if ok { "dernier  : ok" } else { "dernier  : échec" });
        if let Some(error) = last.get("error").and_then(Value::as_str) {
            line.push_str(&format!(" · {}", output::clip(error, 80)));
        }
        if let Some(ms) = last.get("durationMs").and_then(Value::as_u64) {
            line.push_str(&format!(" · {}", output::duration(ms)));
        }
        if let Some(at) = last.get("at").and_then(Value::as_u64) {
            line.push_str(&format!(" · {}", output::stamp(at)));
        }
        lines.push(line);
    } else if let Some(run) = value.get("lastRun").filter(|run| !run.is_null()) {
        let status = run.get("status").and_then(Value::as_str).unwrap_or("?");
        let mut line = format!("dernier  : {status}");
        if let Some(pid) = run.get("pid").and_then(Value::as_u64) {
            line.push_str(&format!(" · pid {pid}"));
        }
        if let Some(ts) = run.get("ts").and_then(Value::as_u64) {
            line.push_str(&format!(" · {}", output::stamp(ts)));
        }
        lines.push(line);
    }
    if value.get("rebuildNeeded").and_then(Value::as_bool).unwrap_or(false) {
        lines.push(String::from("binaire  : à recompiler"));
    }
    lines.join("\n") + "\n"
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("?")
}

#[cfg(test)]
mod tests {
    use super::summary;
    use serde_json::json;

    #[test]
    fn summarises_a_hosted_agent() {
        let text = summary(&json!({
            "agent": "discord",
            "runner": "claude",
            "name": "swarm-discord",
            "url": "http://127.0.0.1:23082/mcp",
            "instance": "swarm-control-1",
            "state": "busy",
            "queueLength": 2,
            "current": {"from": "swarm-control-1", "runId": "r1", "elapsedMs": 1500},
            "session": "s1",
            "turnsDone": 7,
            "lastTurn": {"ok": true, "error": null, "durationMs": 1200, "at": 1_600_000_000_000u64},
        }));
        assert!(text.contains("agent    : discord (claude)"), "{text}");
        assert!(text.contains("serveur  : http://127.0.0.1:23082/mcp"), "{text}");
        assert!(text.contains("état     : occupé · file 2 · 7 tour(s)"), "{text}");
        assert!(text.contains("en cours : demandé par swarm-control-1 · 1,5 s"), "{text}");
        assert!(text.contains("session  : s1"), "{text}");
        assert!(text.contains("dernier  : ok · 1,2 s · 2020-09-13 12:26:40Z"), "{text}");
    }

    #[test]
    fn summarises_the_disk_after_a_failure() {
        let text = summary(&json!({
            "agent": "bob",
            "runner": "hermes",
            "resident": false,
            "session": null,
            "lastRun": {"status": "failed", "pid": 4212, "ts": 1_600_000_000_000u64},
        }));
        assert!(text.contains("agent    : bob (hermes)"), "{text}");
        assert!(text.contains("résident : non"), "{text}");
        assert!(text.contains("session  : (aucune)"), "{text}");
        assert!(text.contains("dernier  : failed · pid 4212 · 2020-09-13 12:26:40Z"), "{text}");
        assert!(!text.contains("serveur"), "{text}");
    }

    /// La ligne « binaire » n'apparaît que si `rebuildNeeded` vaut vrai : ni pour `false`, ni
    /// pour `null` (jamais compilé), ni s'il est absent.
    #[test]
    fn summarises_a_binary_to_rebuild_only_when_needed() {
        let doc = |rebuild| json!({ "agent": "bob", "runner": "claude", "rebuildNeeded": rebuild });
        let text = summary(&doc(json!(true)));
        assert!(text.contains("agent    : bob (claude)"), "{text}");
        assert!(text.lines().any(|line| line == "binaire  : à recompiler"), "{text}");
        for quiet in [json!(false), serde_json::Value::Null] {
            let text = summary(&doc(quiet));
            assert!(!text.contains("recompiler"), "{text}");
        }
        let text = summary(&json!({ "agent": "bob", "runner": "claude" }));
        assert!(!text.contains("recompiler"), "{text}");
    }

    #[test]
    fn tolerates_a_minimal_document() {
        let text = summary(&json!({"agent": "x", "runner": "claude", "state": "idle"}));
        assert!(text.contains("agent    : x (claude)"), "{text}");
        assert!(text.contains("état     : inactif · file 0"), "{text}");
        assert!(text.contains("session  : (aucune)"), "{text}");
    }
}
