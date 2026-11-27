//! Arguments de la ligne de commande `claude`.

use super::settings::AgentSetup;
use crate::config;
use crate::runners::RunRequest;

/// Limite de ligne de commande Windows (~32 Kio) : une persona plus longue
/// est ajoutée au début du prompt (stdin) à la place.
const MAX_PERSONA_ARG: usize = 20_000;
const DEFAULT_PERMISSIONS: &str = "--dangerously-skip-permissions";

/// `(arguments, prompt envoyé sur stdin)`.
pub fn build(
    req: &RunRequest,
    setup: &AgentSetup,
    model: Option<&str>,
    resume: Option<&str>,
) -> (Vec<String>, String) {
    let mut args: Vec<String> = ["-p", "--output-format", "json"].map(String::from).to_vec();
    let permissions =
        config::app_var("CLAUDE_PERMISSIONS").unwrap_or_else(|| DEFAULT_PERMISSIONS.into());
    args.extend(permissions.split_whitespace().map(String::from));
    push_files(&mut args, setup);
    push_pair(&mut args, "--resume", resume);
    push_pair(&mut args, "--model", model);
    push_pair(&mut args, "--name", req.agent.as_deref());
    let stdin_prompt = apply_persona(&mut args, setup.persona.as_deref(), &req.prompt, resume);
    (args, stdin_prompt)
}

fn push_pair(args: &mut Vec<String>, flag: &str, value: Option<&str>) {
    if let Some(v) = value {
        args.push(flag.into());
        args.push(v.into());
    }
}

fn push_files(args: &mut Vec<String>, setup: &AgentSetup) {
    if let Some(p) = &setup.settings_file {
        push_pair(args, "--settings", Some(&p.display().to_string()));
    }
    if let Some(p) = &setup.mcp_file {
        push_pair(args, "--mcp-config", Some(&p.display().to_string()));
        if setup.strict_mcp {
            args.push("--strict-mcp-config".into());
        }
    }
}

/// Persona courte : `--append-system-prompt`. Trop longue : préfixée au prompt
/// de la première session seulement (une session reprise la connaît déjà).
fn apply_persona(
    args: &mut Vec<String>,
    persona: Option<&str>,
    prompt: &str,
    resume: Option<&str>,
) -> String {
    match persona {
        Some(p) if p.len() <= MAX_PERSONA_ARG => {
            push_pair(args, "--append-system-prompt", Some(p));
            prompt.to_string()
        }
        Some(p) if resume.is_none() => format!("{p}\n\n---\n\n{prompt}"),
        _ => prompt.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::apply_persona;

    #[test]
    fn short_persona_goes_to_the_system_prompt() {
        let mut args = Vec::new();
        let stdin = apply_persona(&mut args, Some("Tu es X"), "salut", None);
        assert_eq!(args, vec!["--append-system-prompt", "Tu es X"]);
        assert_eq!(stdin, "salut");
    }

    #[test]
    fn long_persona_prefixes_only_a_fresh_session() {
        let long = "x".repeat(20_001);
        let mut args = Vec::new();
        assert!(apply_persona(&mut args, Some(&long), "salut", None).ends_with("salut"));
        assert!(args.is_empty());
        assert_eq!(apply_persona(&mut args, Some(&long), "salut", Some("sid")), "salut");
    }
}
