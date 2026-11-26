//! Faux CLI pour les tests d'intégration : imite `claude -p` et `hermes`.
//!
//! - Claude (`-p`) : lit le prompt sur stdin, rend l'enveloppe JSON
//!   `{"result", "session_id", "is_error"}` ; `--resume X` conserve la session.
//! - Hermes (`chat`) : prompt sur stdin (`--query-file -`), réponse sur stdout,
//!   `session_id: …` sur stderr ; gère aussi `profile create|delete` et `config set`.
//!
//! Déclencheurs dans le prompt : `SLEEP` (dort 60 s), `DEADSESSION` (échoue si
//! une session est reprise, comme un `--resume` périmé).

use std::io::{Read, Write};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let home = std::env::var("HERMES_HOME").unwrap_or_default();
    let arg_after = |flag: &str| {
        args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned()
    };

    // ── Hermes : gestion des profils ──
    if args.iter().any(|a| a == "profile") {
        let name = args.iter().find(|a| !a.starts_with('-') && *a != "profile" && *a != "create" && *a != "delete" && *a != "show").cloned().unwrap_or_default();
        let dir = std::path::Path::new(&home).join("profiles").join(&name);
        if args.iter().any(|a| a == "create") {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("config.yaml"), "model:\n  default: none\n").unwrap();
            // --clone-from SRC : copie le .env (là où vivent les clés d'un profil).
            if let Some(src) = arg_after("--clone-from") {
                let from = std::path::Path::new(&home).join("profiles").join(src).join(".env");
                if from.is_file() {
                    std::fs::copy(from, dir.join(".env")).unwrap();
                }
            }
        } else if args.iter().any(|a| a == "delete") {
            let _ = std::fs::remove_dir_all(&dir);
        }
        return;
    }
    if args.iter().any(|a| a == "config") && args.iter().any(|a| a == "set") {
        let pos = args.iter().position(|a| a == "set").unwrap();
        let (key, value) = (&args[pos + 1], &args[pos + 2]);
        let name = arg_after("-p").unwrap_or_default();
        let path = std::path::Path::new(&home).join("profiles").join(name).join("config.yaml");
        let mut content = std::fs::read_to_string(&path).unwrap_or_default();
        content.push_str(&format!("# {key}={value}\n"));
        if key == "model.default" {
            content = format!("model:\n  default: {value}\n");
        }
        std::fs::write(path, content).unwrap();
        return;
    }

    let mut prompt = String::new();
    let _ = std::io::stdin().read_to_string(&mut prompt);
    let resume = arg_after("--resume");
    let model = arg_after("--model").unwrap_or_else(|| "-".into());
    let agent = std::env::var("OVERMIND_AGENT_NAME").unwrap_or_default();
    let token = std::env::var("ANTHROPIC_AUTH_TOKEN").unwrap_or_default();
    let is_hermes = args.iter().any(|a| a == "chat");

    if prompt.contains("SLEEP") {
        std::thread::sleep(std::time::Duration::from_secs(60));
    }
    if let (true, Some(sid)) = (prompt.contains("DEADSESSION"), resume.as_ref()) {
        eprintln!("No conversation found with session ID: {sid}");
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
        println!("{{\"type\":\"result\",\"is_error\":true,\"subtype\":\"error\",\"api_error_status\":401,\"result\":\"unauthorized\",\"session_id\":\"s-fail\"}}");
        std::process::exit(1);
    }

    let session = resume.clone().unwrap_or_else(|| format!("sess-{}-{}", std::process::id(), prompt.len()));
    let text = format!(
        "echo:{}|resumed:{}|model:{}|agent:{}|token:{}",
        prompt.trim(),
        resume.as_deref().unwrap_or("none"),
        model,
        agent,
        token
    );
    if is_hermes {
        print!("{text}");
        eprintln!("session_id: {session}");
    } else {
        let env = serde_json::json!({
            "type": "result", "subtype": "success", "is_error": false,
            "result": text, "session_id": session,
        });
        println!("{env}");
    }
    let _ = std::io::stdout().flush();
}
