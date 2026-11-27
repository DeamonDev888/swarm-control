//! Outil `logs` : fin de la sortie du dernier tour (ou du tour en cours).
//!
//! Si un run tourne (`registry::find` + `Status::Running`), la sortie vient du run vivant ;
//! sinon de [`Ctx::last_output`], gardée par le fil de travail. Dans les deux cas on ne rend
//! que la fin, coupée sur une frontière de caractère pour ne jamais casser un caractère UTF-8.

use crate::registry::{self, Status};
use crate::residents::ctx::Ctx;
use litemcp::prelude::*;
use litemcp::zod;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

const DESCRIPTION: &str = "Fin de la sortie brute : celle du tour en cours s'il tourne, sinon celle \
    du dernier tour. tail borne la taille rendue en octets (défaut 8192) ; la coupe garde la fin \
    et ne casse jamais un caractère. live dit si un tour tourne.";

/// Taille rendue par défaut (8 Kio).
const DEFAULT_TAIL: usize = 8 * 1024;
const MIN_TAIL: u64 = 1;
const MAX_TAIL: u64 = 65_536;

#[derive(Deserialize)]
struct Args {
    tail: Option<u64>,
}

pub fn register(builder: ServerBuilder, ctx: Arc<Ctx>) -> ServerBuilder {
    let schema = zod::object()
        .opt(
            "tail",
            zod::describe(
                "Octets rendus, les derniers (défaut 8192, max 65536)",
                zod::uint().min(MIN_TAIL).max(MAX_TAIL),
            ),
        )
        .strict();
    builder
        .tool_parse("logs", DESCRIPTION, schema, move |args: Args| handle(&ctx, args))
        .annotate("logs", ToolAnnotations::new().title("Sortie du tour").read_only(true))
}

fn handle(ctx: &Ctx, args: Args) -> ToolResult {
    let running = registry::find(&ctx.agent, Some(ctx.runner.as_str()))
        .filter(|run| run.snapshot().status == Status::Running);
    let output = match &running {
        Some(run) => run.output_since(None),
        None => ctx.last_output(),
    };
    let tail = args.tail.map_or(DEFAULT_TAIL, |n| n as usize);
    Ok(rendered(&output, tail, running.is_some()))
}

/// Réponse de l'outil : `bytes` est la taille réellement rendue, pas celle de la source.
fn rendered(output: &str, tail: usize, live: bool) -> Value {
    let output = keep_end(output, tail);
    json!({ "output": output, "bytes": output.len(), "live": live })
}

/// Les `max` derniers octets, ramenés à une frontière de caractère.
///
/// Le résultat fait au plus `max` octets : un caractère multi-octets à cheval sur la coupe est
/// écarté (au plus 3 octets perdus), jamais rendu incomplet.
fn keep_end(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut start = text.len() - max;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}

#[cfg(test)]
mod tests {
    use super::{Args, DEFAULT_TAIL, MAX_TAIL, MIN_TAIL, handle, keep_end, rendered};
    use crate::agents::Runner;
    use crate::residents::ctx::Ctx;
    use serde_json::json;
    use std::sync::Arc;

    fn temp_ctx(name: &str) -> Arc<Ctx> {
        let ws = std::env::temp_dir().join(format!("swarm-mcp-logs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&ws);
        Arc::new(Ctx::new("alice", Runner::Claude, ws, "test".into()))
    }

    #[test]
    fn a_short_output_is_kept_whole() {
        assert_eq!(keep_end("abc", 8), "abc");
        assert_eq!(keep_end("", 8), "");
    }

    #[test]
    fn a_long_output_keeps_its_end() {
        assert_eq!(keep_end("abcdef", 3), "def");
        assert_eq!(keep_end("abcdef", 6), "abcdef");
    }

    #[test]
    fn the_cut_never_splits_a_character() {
        // « a » (1 octet) + « é » (2 octets) : une coupe à 1 octet tomberait dans « é ».
        assert_eq!(keep_end("aé", 2), "é");
        assert_eq!(keep_end("aé", 1), "", "le caractère coupé est écarté, pas tronqué");
        // « x » + emoji (4 octets) + « y » : la coupe avance jusqu'à la frontière suivante.
        assert_eq!(keep_end("x😀y", 3), "y");
        assert_eq!(keep_end("x😀y", 5), "😀y");
        assert!(keep_end("x😀y", 3).len() <= 3);
    }

    #[test]
    fn rendered_reports_the_returned_size_and_live_flag() {
        let out = rendered("abcdef", 3, true);
        assert_eq!(out["output"], json!("def"));
        assert_eq!(out["bytes"], json!(3));
        assert_eq!(out["live"], json!(true));
    }

    #[test]
    fn the_handler_serves_the_end_of_the_last_turn() {
        let ctx = temp_ctx("dernier-tour");
        assert_eq!(handle(&ctx, Args { tail: None }).unwrap()["output"], json!(""));
        ctx.set_output("une sortie quelconque".into());
        let out = handle(&ctx, Args { tail: Some(4) }).unwrap();
        assert_eq!(out["output"], json!("nque"));
        assert_eq!(out["bytes"], json!(4));
        assert_eq!(out["live"], json!(false), "aucun run en mémoire dans ce test");
        assert_eq!(
            handle(&ctx, Args { tail: None }).unwrap()["output"],
            json!("une sortie quelconque")
        );
        let _ = std::fs::remove_dir_all(&ctx.workspace);
    }

    #[test]
    fn the_defaults_fit_the_schema_range() {
        assert!((MIN_TAIL..=MAX_TAIL).contains(&(DEFAULT_TAIL as u64)));
    }
}
