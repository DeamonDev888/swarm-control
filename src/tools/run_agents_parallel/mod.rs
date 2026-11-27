//! Outil `run_agents_parallel` : jusqu'à 10 agents en parallèle.

mod launch;
mod routed;
mod summary;

use super::errors::invalid;
use super::run_args::RunArgs;
use super::schema::run_object;
use litemcp::prelude::*;
use litemcp::zod;
use serde::Deserialize;
use serde_json::Value;
use std::sync::mpsc;
use std::time::Instant;

const DESCRIPTION: &str = "Lance jusqu'à 10 agents en parallèle (runners mélangeables). waitAll:false rend \
    la main dès le premier succès ; les autres continuent et restent suivis par agent_control.";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ParallelArgs {
    agents: Vec<RunArgs>,
    wait_all: Option<bool>,
}

pub fn register(builder: ServerBuilder) -> ServerBuilder {
    let task = run_object().opt("taskId", zod::string().min(1)).strict();
    let schema = zod::object()
        .field("agents", zod::array(task).min(1).max(10))
        .opt("waitAll", zod::describe("Attendre tous les agents (défaut: true)", zod::boolean()))
        .strict();
    builder.tool_parse("run_agents_parallel", DESCRIPTION, schema, handle)
}

fn handle(args: ParallelArgs) -> ToolResult {
    if args.agents.is_empty() || args.agents.len() > 10 {
        return Err(invalid("entre 1 et 10 agents"));
    }
    let wait_all = args.wait_all.unwrap_or(true);
    let total = args.agents.len();
    let started = Instant::now();
    let (tx, rx) = mpsc::channel::<launch::Finished>();

    let launched = launch::all(args.agents, &tx);
    drop(tx);

    let mut results = launched.immediate;
    let mut received = 0usize;
    for _ in 0..launched.count {
        let Ok(finished) = rx.recv() else { break };
        received += 1;
        let stop_early = finished.ok && !wait_all;
        results.push((finished.index, finished.json));
        if stop_early {
            break;
        }
    }
    Ok(summary::build(total, results, launched.count - received, started.elapsed()))
}

pub(super) type Indexed = (usize, Value);
