//! Enveloppe JSON de `claude -p --output-format json`.

use crate::process::ProcOutput;
use crate::runners::RunResult;
use crate::runners::end::end_error;
use crate::runners::text::{join_raw, tail};
use serde_json::Value;
use std::time::Duration;

/// Transforme la sortie du CLI en résultat de run.
pub fn parse(out: &ProcOutput, timeout: Duration, model: Option<String>) -> RunResult {
    let raw = join_raw(out);
    let envelope = last_object(&out.stdout);
    let session_id = envelope.as_ref().and_then(session_id);
    if let Some(error) = end_error(out, timeout) {
        return RunResult { error: Some(error), raw, session_id, model, ..Default::default() };
    }
    match envelope {
        Some(v) => from_envelope(&v, out, raw, session_id, model),
        None => from_plain_output(out, raw, model),
    }
}

fn from_envelope(
    v: &Value,
    out: &ProcOutput,
    raw: String,
    session_id: Option<String>,
    model: Option<String>,
) -> RunResult {
    let text = v
        .get("result")
        .or_else(|| v.get("reply"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let is_error = v.get("is_error").and_then(Value::as_bool).unwrap_or(false);
    if !is_error && out.code.is_none_or(|c| c == 0) {
        return RunResult { result: text, session_id, raw, model, error: None };
    }
    let subtype = v.get("subtype").and_then(Value::as_str).unwrap_or("error");
    let code = match v.get("api_error_status").and_then(Value::as_u64) {
        Some(status) => format!("CLAUDE_ERROR[{subtype}, HTTP {status}]"),
        None => format!("CLAUDE_ERROR[{subtype}]"),
    };
    let detail = if text.is_empty() { tail(&out.stderr, 800) } else { tail(&text, 800) };
    RunResult { error: Some(format!("{code}: {detail}")), result: text, raw, session_id, model }
}

/// Pas d'enveloppe : stdout brut si le code est 0, sinon l'erreur du CLI.
fn from_plain_output(out: &ProcOutput, raw: String, model: Option<String>) -> RunResult {
    if out.code == Some(0) && !out.stdout.trim().is_empty() {
        return RunResult {
            result: out.stdout.trim().to_string(),
            raw,
            model,
            ..Default::default()
        };
    }
    let code = out.code.map_or("?".to_string(), |c| c.to_string());
    let hint = tail(out.stderr.trim(), 800);
    RunResult { error: Some(format!("EXIT_CODE_{code}: {hint}")), raw, model, ..Default::default() }
}

/// Dernier objet JSON de la sortie (tolère du bruit avant/après).
pub fn last_object(stdout: &str) -> Option<Value> {
    let t = stdout.trim();
    if let Ok(v) = serde_json::from_str::<Value>(t) {
        return v.is_object().then_some(v);
    }
    let end = t.rfind('}')?;
    let mut start = t[..end].rfind('{')?;
    loop {
        if let Ok(v) = serde_json::from_str::<Value>(&t[start..=end]) {
            if v.is_object() {
                return Some(v);
            }
        }
        start = t[..start].rfind('{')?;
    }
}

fn session_id(v: &Value) -> Option<String> {
    v.get("session_id").and_then(Value::as_str).filter(|s| !s.is_empty()).map(String::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::End;

    fn out(stdout: &str, stderr: &str, code: i32) -> ProcOutput {
        ProcOutput {
            stdout: stdout.into(),
            stderr: stderr.into(),
            code: Some(code),
            end: End::Exited,
        }
    }

    #[test]
    fn success_envelope() {
        let o =
            out(r#"{"type":"result","is_error":false,"result":"ok","session_id":"abc"}"#, "", 0);
        let r = parse(&o, Duration::from_secs(1), None);
        assert_eq!(r.result, "ok");
        assert_eq!(r.session_id.as_deref(), Some("abc"));
        assert!(r.error.is_none());
    }

    #[test]
    fn error_envelope_reports_http_status() {
        let o = out(
            r#"{"is_error":true,"subtype":"error","api_error_status":429,"result":"rate"}"#,
            "",
            1,
        );
        assert!(parse(&o, Duration::from_secs(1), None).error.unwrap().contains("HTTP 429"));
    }

    #[test]
    fn missing_envelope_reports_the_exit_code() {
        let r = parse(&out("", "No conversation found", 1), Duration::from_secs(1), None);
        assert!(r.error.unwrap().starts_with("EXIT_CODE_1"));
    }

    #[test]
    fn tolerates_noise_around_the_json() {
        let v = last_object("warning {not json\n{\"session_id\":\"s\"}\ntrailer").unwrap();
        assert_eq!(v["session_id"], "s");
    }
}
