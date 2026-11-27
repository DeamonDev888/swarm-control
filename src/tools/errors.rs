//! Erreurs d'outil partagées.

use crate::agents::Runner;
use litemcp::prelude::ToolError;

pub fn invalid(msg: impl Into<String>) -> ToolError {
    ToolError::invalid_input(msg)
}

pub fn parse_runner(s: &str) -> Result<Runner, ToolError> {
    Runner::parse(s).ok_or_else(|| invalid(format!("runner inconnu '{s}' (claude | hermes)")))
}

pub fn opt_runner(s: &Option<String>) -> Result<Option<Runner>, ToolError> {
    s.as_deref().map(parse_runner).transpose()
}
