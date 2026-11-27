//! Appel d'un outil MCP (`tools/call`) sur un serveur distant.

use super::http::{self, PostError};
use super::url;
use serde_json::{Value, json};
use std::time::Duration;

const PROTOCOL: &str = "2026-07-28";

#[derive(Debug)]
pub enum CallError {
    /// Serveur injoignable avant tout envoi : sans effet, on peut se rabattre.
    Unreachable(String),
    /// L'outil a répondu `isError` : le message de l'outil.
    Tool(String),
    /// Réponse illisible ou coupée : la demande a pu être traitée.
    Protocol(String),
}

/// Appelle `tool` sur le serveur `url` ; rend `structuredContent` (ou le texte).
pub fn call_tool(url: &str, tool: &str, args: Value, wait: Duration) -> Result<Value, CallError> {
    let target =
        url::parse(url).ok_or_else(|| CallError::Unreachable(format!("URL invalide: {url}")))?;
    let body = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {
            "name": tool, "arguments": args,
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": PROTOCOL,
                "io.modelcontextprotocol/clientCapabilities": {}
            }
        }
    })
    .to_string();
    let headers =
        [("MCP-Protocol-Version", PROTOCOL), ("Mcp-Method", "tools/call"), ("Mcp-Name", tool)];
    let text = http::post(&target, &headers, &body, wait).map_err(|e| match e {
        PostError::Unreachable(m) => CallError::Unreachable(m),
        PostError::Failed(m) => CallError::Protocol(m),
    })?;
    decode(&text)
}

fn decode(text: &str) -> Result<Value, CallError> {
    let resp: Value = serde_json::from_str(text.trim())
        .map_err(|e| CallError::Protocol(format!("JSON invalide ({e}): {text}")))?;
    if let Some(error) = resp.get("error") {
        return Err(CallError::Tool(error["message"].as_str().unwrap_or("erreur MCP").to_string()));
    }
    let result =
        resp.get("result").ok_or_else(|| CallError::Protocol(format!("sans result: {resp}")))?;
    let payload = result
        .get("structuredContent")
        .cloned()
        .unwrap_or_else(|| result["content"][0]["text"].clone());
    if result.get("isError").and_then(Value::as_bool).unwrap_or(false) {
        let message = payload.as_str().map_or_else(|| payload.to_string(), String::from);
        return Err(CallError::Tool(message));
    }
    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_success_and_tool_errors() {
        let ok = r#"{"result":{"structuredContent":{"a":1}}}"#;
        assert_eq!(decode(ok).unwrap(), json!({"a": 1}));
        let err = r#"{"result":{"isError":true,"content":[{"text":"boum"}]}}"#;
        assert!(matches!(decode(err), Err(CallError::Tool(m)) if m == "boum"));
        assert!(matches!(decode("pas du json"), Err(CallError::Protocol(_))));
    }

    #[test]
    fn closed_port_is_unreachable() {
        let r = call_tool("http://127.0.0.1:1/mcp", "ask", json!({}), Duration::from_secs(1));
        assert!(matches!(r, Err(CallError::Unreachable(_))));
    }
}
