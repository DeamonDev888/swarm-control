//! Interprétation de la sortie de `hermes chat`.

use super::hints::hint_for;
use super::session_id::{find_session_id, strip_session_line};
use crate::process::ProcOutput;
use crate::runners::end::end_error;
use crate::runners::text::{join_raw, tail};
use crate::runners::{RunRequest, RunResult};
use std::time::Duration;

pub fn from_output(
    req: &RunRequest,
    out: &ProcOutput,
    timeout: Duration,
    resume: Option<&str>,
) -> RunResult {
    let raw = join_raw(out);
    // L'identifiant de session sort sur stderr (ou, selon la version, stdout).
    let session_id = find_session_id(&out.stderr)
        .or_else(|| find_session_id(&out.stdout))
        .or_else(|| resume.map(String::from));
    if let Some(error) = end_error(out, timeout) {
        return RunResult { error: Some(error), raw, session_id, ..Default::default() };
    }
    let text = strip_session_line(&out.stdout).trim().to_string();
    if out.code == Some(0) {
        return RunResult { result: text, session_id, raw, ..Default::default() };
    }
    RunResult {
        error: Some(failure(req, out, &text)),
        result: text,
        raw,
        session_id,
        ..Default::default()
    }
}

/// Code non nul = échec, même avec du texte sur stdout : Hermes y met alors le
/// message d'erreur du fournisseur (crédits épuisés, 401…).
fn failure(req: &RunRequest, out: &ProcOutput, text: &str) -> String {
    let code = out.code.map_or("?".to_string(), |c| c.to_string());
    let detail = if text.is_empty() { tail(out.stderr.trim(), 800) } else { tail(text, 800) };
    let mut error = format!("EXIT_CODE_{code}: {detail}");
    if let Some(hint) = hint_for(&format!("{text}\n{}", out.stderr), req.agent.as_deref()) {
        error.push_str(&format!("\n→ {hint}"));
    }
    error
}
