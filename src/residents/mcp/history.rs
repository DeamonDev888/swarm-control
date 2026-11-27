//! Outil `history` : les derniers tours de l'agent, lus dans le journal.
//!
//! Le journal est celui de [`crate::history`] (`bridge/history/<runner>-<agent>.jsonl`) :
//! une entrée par tour, rendues du plus ancien au plus récent (l'ordre du fichier). `before`
//! (ms) exclut les tours à cet horodatage ou après, ce qui permet de remonter page par page.

use crate::history::{self, Entry};
use crate::residents::ctx::Ctx;
use litemcp::prelude::*;
use litemcp::zod;
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Arc;

const DESCRIPTION: &str = "Les derniers tours de cet agent, lus dans son journal, du plus ancien \
    au plus récent. limit borne la page (défaut 50, max 500) ; before (ms) ne garde que les tours \
    antérieurs, pour remonter page par page.";

/// Taille de page par défaut.
const DEFAULT_LIMIT: usize = 50;
/// Plafond de `limit`, imposé aussi par le schéma.
const MAX_LIMIT: u64 = 500;

#[derive(Deserialize)]
struct Args {
    limit: Option<u64>,
    before: Option<u64>,
}

pub fn register(builder: ServerBuilder, ctx: Arc<Ctx>) -> ServerBuilder {
    let schema = zod::object()
        .opt(
            "limit",
            zod::describe(
                "Nombre de tours rendus (défaut 50, max 500)",
                zod::uint().min(1).max(MAX_LIMIT),
            ),
        )
        .opt(
            "before",
            zod::describe("Ne garder que les tours antérieurs à cet horodatage (ms)", zod::uint()),
        )
        .strict();
    builder
        .tool_parse("history", DESCRIPTION, schema, move |args: Args| handle(&ctx, args))
        .annotate("history", ToolAnnotations::new().title("Historique des tours").read_only(true))
}

fn handle(ctx: &Ctx, args: Args) -> ToolResult {
    let limit = args.limit.map_or(DEFAULT_LIMIT, |n| n as usize);
    let entries =
        history::read(&ctx.workspace, ctx.runner.as_str(), &ctx.agent, limit, args.before)
            .map_err(|e| ToolError::internal(format!("journal illisible : {e}")))?;
    let path = history::path(&ctx.workspace, ctx.runner.as_str(), &ctx.agent);
    Ok(page(&ctx.agent, &path, entries))
}

/// Réponse de l'outil : les entrées dans l'ordre du journal, et le total de la page.
fn page(agent: &str, path: &Path, entries: Vec<Entry>) -> Value {
    let total = entries.len();
    json!({
        "agent": agent,
        "entries": entries,
        "total": total,
        "path": path.display().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::{Args, DEFAULT_LIMIT, MAX_LIMIT, handle, page};
    use crate::agents::Runner;
    use crate::history::{self, Entry};
    use crate::residents::ctx::Ctx;
    use serde_json::json;
    use std::path::Path;
    use std::sync::Arc;

    fn entry(ts: u64, prompt: &str) -> Entry {
        Entry {
            ts,
            prompt: prompt.into(),
            reply: format!("echo:{prompt}"),
            session_id: Some("s1".into()),
            duration_ms: 12,
            queued_ms: 3,
            runner: "claude".into(),
            agent: "alice".into(),
            ..Entry::default()
        }
    }

    fn temp_ctx(name: &str) -> Arc<Ctx> {
        let ws =
            std::env::temp_dir().join(format!("swarm-mcp-history-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&ws);
        Arc::new(Ctx::new("alice", Runner::Claude, ws, "test".into()))
    }

    #[test]
    fn page_keeps_the_journal_order_and_counts_it() {
        let out = page(
            "mina",
            Path::new("bridge/history/claude-mina.jsonl"),
            vec![entry(1, "un"), entry(2, "deux")],
        );
        assert_eq!(out["agent"], json!("mina"));
        assert_eq!(out["total"], json!(2));
        assert_eq!(out["entries"][0]["ts"], json!(1));
        assert_eq!(out["entries"][1]["ts"], json!(2));
        assert!(out["path"].as_str().unwrap().ends_with("claude-mina.jsonl"));
    }

    #[test]
    fn an_entry_serialises_with_camel_case_fields() {
        let out = page("mina", Path::new("j.jsonl"), vec![entry(7, "sept")]);
        let first = &out["entries"][0];
        assert_eq!(first["sessionId"], json!("s1"));
        assert_eq!(first["durationMs"], json!(12));
        assert_eq!(first["queuedMs"], json!(3));
        assert!(first.get("error").is_none(), "un tour réussi n'a pas de champ error");
    }

    #[test]
    fn an_empty_page_is_well_formed() {
        let out = page("mina", Path::new("j.jsonl"), Vec::new());
        assert_eq!(out["total"], json!(0));
        assert_eq!(out["entries"], json!([]));
    }

    #[test]
    fn limit_and_before_page_through_the_journal() {
        let ctx = temp_ctx("pagination");
        for i in 1..=5u64 {
            let e = entry(i * 10, &format!("t{i}"));
            history::append(&ctx.workspace, "claude", "alice", &e).unwrap();
        }
        let last2 = handle(&ctx, Args { limit: Some(2), before: None }).unwrap();
        let prompts: Vec<&str> = last2["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["prompt"].as_str().unwrap())
            .collect();
        assert_eq!(prompts, ["t4", "t5"]);
        assert_eq!(last2["total"], json!(2));
        let older = handle(&ctx, Args { limit: Some(10), before: Some(40) }).unwrap();
        let prompts: Vec<&str> = older["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["prompt"].as_str().unwrap())
            .collect();
        assert_eq!(prompts, ["t1", "t2", "t3"]);
        let all = handle(&ctx, Args { limit: None, before: None }).unwrap();
        assert_eq!(all["total"], json!(5), "la limite par défaut couvre la page");
        let _ = std::fs::remove_dir_all(&ctx.workspace);
    }

    #[test]
    fn the_limits_fit_the_schema_range() {
        assert!((1..=MAX_LIMIT).contains(&(DEFAULT_LIMIT as u64)));
    }
}
