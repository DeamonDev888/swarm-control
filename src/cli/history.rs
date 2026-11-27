//! `history` : les derniers tours d'un agent, lus dans son journal.
//!
//! Le journal (`bridge/history/<runner>-<agent>.jsonl`, voir [`crate::history`]) est à nous : il
//! ne dépend pas du format de session du CLI. On lit les `-n` derniers tours, éventuellement
//! avant un horodatage (`--before`, pour remonter page par page).

use super::args::Options;
use super::output;
use super::target;
use crate::history::{self, Entry};
use serde_json::json;
use std::path::Path;

pub fn run(
    options: &Options,
    ws: &Path,
    agent: &str,
    limit: usize,
    before: Option<u64>,
) -> Result<(), String> {
    target::validate(agent)?;
    let runner = match target::runner(ws, agent, options.runner) {
        Ok(runner) => runner,
        // Agent supprimé mais journal encore là : on lit quand même.
        Err(error) => target::RUNNERS
            .into_iter()
            .find(|runner| history::path(ws, runner.as_str(), agent).is_file())
            .ok_or(error)?,
    };
    let entries = history::read(ws, runner.as_str(), agent, limit, before)
        .map_err(|e| format!("journal de {agent} ({}) illisible: {e}", runner.as_str()))?;
    crate::debug!(
        "history {agent} ({}): {} tour(s) dans {}",
        runner.as_str(),
        entries.len(),
        history::path(ws, runner.as_str(), agent).display()
    );
    if options.json {
        output::json(&json!({"agent": agent, "runner": runner.as_str(), "turns": entries}));
        return Ok(());
    }
    if entries.is_empty() {
        println!("aucun tour pour {agent} ({}).", runner.as_str());
        return Ok(());
    }
    for entry in &entries {
        println!("{}", line(entry));
    }
    println!("\n{} tour(s) · {}", entries.len(), runner.as_str());
    Ok(())
}

/// Une ligne lisible par tour : date, issue, durée, demandeur, prompt, réponse (tronqués).
fn line(entry: &Entry) -> String {
    let status = if entry.ok() { "ok" } else { "échec" };
    let who = entry.from.as_deref().unwrap_or("-");
    let body = match &entry.error {
        Some(error) => error.as_str(),
        None => entry.reply.as_str(),
    };
    format!(
        "{}  {:<5}  {:>8}  {:<16}  {}  →  {}",
        output::stamp(entry.ts),
        status,
        output::duration(entry.duration_ms),
        output::clip(who, 16),
        output::clip(&entry.prompt, 80),
        output::clip(body, 120),
    )
}

#[cfg(test)]
mod tests {
    use super::line;
    use crate::history::Entry;

    #[test]
    fn a_turn_line_shows_date_status_and_both_texts() {
        let entry = Entry {
            ts: 1_600_000_000_000,
            from: Some("swarm-control-1".into()),
            prompt: "état du marché ?".into(),
            reply: "calme".into(),
            duration_ms: 1_200,
            runner: "claude".into(),
            agent: "sniper".into(),
            ..Default::default()
        };
        let text = line(&entry);
        assert!(text.starts_with("2020-09-13 12:26:40Z  ok"), "{text}");
        assert!(text.contains("1,2 s"), "{text}");
        assert!(text.contains("swarm-control-1"), "{text}");
        assert!(text.contains("état du marché ?  →  calme"), "{text}");
    }

    #[test]
    fn a_failed_turn_shows_the_error_instead_of_the_reply() {
        let entry = Entry {
            ts: 0,
            prompt: "x".into(),
            reply: "partiel".into(),
            error: Some("STOPPED: arrêté".into()),
            duration_ms: 900,
            ..Default::default()
        };
        let text = line(&entry);
        assert!(text.contains("échec"), "{text}");
        assert!(text.contains("STOPPED: arrêté"), "{text}");
        assert!(!text.contains("partiel"), "{text}");
    }
}
