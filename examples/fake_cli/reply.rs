//! Réponses du faux CLI, selon les déclencheurs du prompt.

use crate::args::Args;
use std::io::Write;

pub fn answer(args: &Args, prompt: &str) {
    let resume = args.after("--resume").or_else(|| args.after("--conversation"));
    let token = std::env::var("ANTHROPIC_AUTH_TOKEN").unwrap_or_default();
    let is_hermes = args.is_hermes();
    let is_antigravity = args.is_antigravity();

    if prompt.contains("SLEEP") {
        std::thread::sleep(std::time::Duration::from_secs(60));
    }
    // Tour lent mais fini : pour observer une file d'attente.
    if prompt.contains("SLOW") {
        std::thread::sleep(std::time::Duration::from_millis(1500));
    }
    if let (true, Some(sid)) = (prompt.contains("DEADSESSION"), resume.as_ref()) {
        if is_antigravity {
            eprintln!("Conversation not found: {sid}");
        } else {
            eprintln!("No conversation found with session ID: {sid}");
        }
        std::process::exit(1);
    }
    if is_antigravity && prompt.contains("AGYFAIL") {
        println!("{{\"is_error\":true,\"error\":{{\"message\":\"Unknown model\"}}}}");
        std::process::exit(1);
    }
    // Hermes réel : une erreur de fournisseur sort le message sur stdout ET code 1.
    if is_hermes && prompt.contains("PROVIDERFAIL") {
        print!("Billing or credits exhausted: HTTP 429: Insufficient balance");
        eprintln!("session_id: local-failed-session");
        std::process::exit(1);
    }
    if is_hermes && prompt.contains("NOCRED") {
        eprintln!("No usable credentials found for provider 'deepseek'. Set DEEPSEEK_API_KEY.");
        std::process::exit(1);
    }
    if prompt.contains("FAIL401") && token != "backup-token" {
        println!(
            "{{\"type\":\"result\",\"is_error\":true,\"subtype\":\"error\",\"api_error_status\":401,\"result\":\"unauthorized\",\"session_id\":\"s-fail\"}}"
        );
        std::process::exit(1);
    }

    let session =
        resume.clone().unwrap_or_else(|| format!("sess-{}-{}", std::process::id(), prompt.len()));
    let text = echo_text(args, prompt, resume.as_deref(), &token);
    if is_hermes {
        print!("{text}");
        eprintln!("session_id: {session}");
    } else if is_antigravity {
        let envelope = serde_json::json!({
            "type": "result", "is_error": false,
            "result": text, "conversation_id": session,
        });
        println!("{envelope}");
    } else {
        let envelope = serde_json::json!({
            "type": "result", "subtype": "success", "is_error": false,
            "result": text, "session_id": session,
        });
        println!("{envelope}");
    }
    let _ = std::io::stdout().flush();
}

/// Réponse « écho » : permet aux tests de vérifier ce que le serveur a transmis.
fn echo_text(args: &Args, prompt: &str, resume: Option<&str>, token: &str) -> String {
    format!(
        "echo:{}|resumed:{}|model:{}|agent:{}|token:{}",
        prompt.trim(),
        resume.unwrap_or("none"),
        args.after("--model").unwrap_or_else(|| "-".into()),
        std::env::var("SWARM_AGENT_NAME").unwrap_or_default(),
        token
    )
}
