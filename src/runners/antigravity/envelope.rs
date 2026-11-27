//! Enveloppe JSON de `agy -p <prompt> --output-format json`.

use crate::process::ProcOutput;
use crate::runners::RunResult;
use crate::runners::claude::envelope::last_object;
use crate::runners::end::end_error;
use crate::runners::text::{join_raw, tail};
use serde_json::Value;
use std::time::Duration;

/// Transforme la sortie d'`agy` en résultat de run.
pub fn parse(out: &ProcOutput, timeout: Duration, model: Option<String>) -> RunResult {
    let raw = join_raw(out);
    let envelope = last_object(&out.stdout);
    let session_id = envelope.as_ref().and_then(conversation_id);
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
        .or_else(|| v.get("response"))
        .or_else(|| v.get("text"))
        .or_else(|| v.get("reply"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let is_error = v.get("is_error").and_then(Value::as_bool).unwrap_or(false)
        || v.get("status").and_then(Value::as_str) == Some("ERROR")
        || v.get("error").is_some_and(|e| !e.is_null() && e != &Value::Bool(false) && e != "");
    if !is_error && out.code.is_none_or(|c| c == 0) {
        return RunResult { result: text, session_id, raw, model, error: None };
    }
    let detail = error_detail(v, &text, &out.stderr);
    RunResult {
        error: Some(format!("ANTIGRAVITY_ERROR: {detail}")),
        result: text,
        raw,
        session_id,
        model,
    }
}

fn error_detail(v: &Value, text: &str, stderr: &str) -> String {
    if let Some(msg) =
        v.get("error").and_then(|e| e.as_str().or_else(|| e.get("message").and_then(Value::as_str)))
    {
        return tail(msg, 800);
    }
    if !text.is_empty() {
        return tail(text, 800);
    }
    tail(stderr.trim(), 800)
}

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

fn conversation_id(v: &Value) -> Option<String> {
    ["conversation_id", "conversationId", "session_id", "sessionId"]
        .iter()
        .find_map(|k| v.get(*k).and_then(Value::as_str))
        .filter(|s| !s.is_empty())
        .map(String::from)
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
    fn parses_conversation_id_and_result() {
        let o = out(
            r#"{"type":"result","is_error":false,"result":"salut","conversation_id":"conv-42"}"#,
            "",
            0,
        );
        let r = parse(&o, Duration::from_secs(1), Some("gemini-pro".into()));
        assert_eq!(r.result, "salut");
        assert_eq!(r.session_id.as_deref(), Some("conv-42"));
        assert_eq!(r.model.as_deref(), Some("gemini-pro"));
        assert!(r.error.is_none());
    }

    #[test]
    fn parses_error_envelope() {
        let o = out(r#"{"is_error":true,"error":{"message":"Unknown model"}}"#, "", 1);
        let r = parse(&o, Duration::from_secs(1), None);
        assert!(r.error.unwrap().contains("Unknown model"));
    }

    #[test]
    fn missing_envelope_reports_exit_code() {
        let r = parse(&out("", "Conversation not found", 1), Duration::from_secs(1), None);
        assert!(r.error.unwrap().starts_with("EXIT_CODE_1"));
    }

    #[test]
    fn parses_real_agy_response_and_status_error() {
        let ok_out = out(
            r#"{"conversation_id":"c-1","status":"OK","response":"bonjour","error":""}"#,
            "",
            0,
        );
        let r = parse(&ok_out, Duration::from_secs(1), None);
        assert_eq!(r.result, "bonjour");
        assert_eq!(r.session_id.as_deref(), Some("c-1"));
        assert!(r.error.is_none());

        let err_out = out(
            r#"{"conversation_id":"c-2","status":"ERROR","response":"","error":"print timeout"}"#,
            "",
            0,
        );
        let r_err = parse(&err_out, Duration::from_secs(1), None);
        assert!(r_err.error.unwrap().contains("print timeout"));
        assert_eq!(r_err.session_id.as_deref(), Some("c-2"));
    }
}
