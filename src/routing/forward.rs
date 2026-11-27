//! Transmission d'un run à son hôte, par `ask`.

use super::eligible;
use crate::owners::Holder;
use crate::peers::{CallError, call_tool};
use crate::runners::{RunRequest, RunResult};
use serde_json::{Value, json};
use std::time::Duration;

/// Au-delà de l'attente par défaut de `ask` (1 h).
const WAIT: Duration = Duration::from_secs(3700);

pub struct Forwarded {
    pub result: RunResult,
    pub host: Holder,
}

/// Confie le run à son hôte. `None` = pas routable, ou hôte injoignable : lancer à la demande.
pub fn forward(req: &RunRequest) -> Option<Forwarded> {
    let host = eligible::host_of(req)?;
    let mut args = json!({ "prompt": req.prompt, "from": host_name() });
    if let Some(model) = &req.model {
        args["model"] = json!(model);
    }
    let result = match call_tool(host.url.as_deref()?, "ask", args, WAIT) {
        Ok(v) => reply(&v),
        Err(CallError::Unreachable(why)) => {
            eprintln!("hôte {} injoignable ({why}) : run à la demande", host.instance);
            return None;
        }
        Err(CallError::Tool(m) | CallError::Protocol(m)) => RunResult::err(m),
    };
    Some(Forwarded { result, host })
}

fn host_name() -> String {
    crate::owners::instance_name()
}

fn reply(v: &Value) -> RunResult {
    RunResult {
        result: v["reply"].as_str().unwrap_or_default().to_string(),
        session_id: v["sessionId"].as_str().map(String::from),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::reply;
    use serde_json::json;

    #[test]
    fn maps_an_ask_reply_to_a_run_result() {
        let r = reply(&json!({"reply": "salut", "sessionId": "s1"}));
        assert_eq!((r.result.as_str(), r.session_id.as_deref()), ("salut", Some("s1")));
        assert!(r.error.is_none());
    }
}
