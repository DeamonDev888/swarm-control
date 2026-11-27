//! Recherche du run ciblé.

use super::args::ControlArgs;
use crate::agents::validate_name;
use crate::registry::{self, LiveRun};
use crate::tools::errors::{invalid, parse_runner};
use litemcp::prelude::ToolError;
use std::sync::Arc;

/// Run visé par `runId`, sinon par `agentName` (+ `runner`). `None` si inconnu en mémoire.
pub fn find_run(args: &ControlArgs) -> Result<Option<Arc<LiveRun>>, ToolError> {
    if let Some(id) = &args.run_id {
        return Ok(registry::find_by_run_id(id));
    }
    let name = args.agent_name.as_deref().ok_or_else(|| invalid("agentName ou runId requis"))?;
    validate_name(name).map_err(invalid)?;
    if let Some(r) = args.runner.as_deref() {
        parse_runner(r)?;
    }
    Ok(registry::find(name, args.runner.as_deref()))
}
