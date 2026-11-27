//! `reset` : oublie la session d'un agent (le prochain tour repart à neuf).
//!
//! Hébergé ici → `Ctx::reset_session` (qui gère aussi le tour en cours : la session sera oubliée
//! à sa fin, sinon le tour la réécrirait) ; sinon → `sessions::delete`. Affiche
//! `{agent, hadSession}`.

use super::args::Options;
use super::output;
use super::target;
use crate::sessions;
use serde_json::json;
use std::path::Path;

pub fn run(options: &Options, ws: &Path, agent: &str) -> Result<(), String> {
    target::validate(agent)?;
    let had_session = match target::hosted(agent, options.runner) {
        Some(ctx) => ctx.reset_session(),
        None => {
            let runner = target::runner(ws, agent, options.runner)?;
            sessions::delete(ws, runner.as_str(), agent)
        }
    };
    output::json(&json!({"agent": agent, "hadSession": had_session}));
    Ok(())
}
