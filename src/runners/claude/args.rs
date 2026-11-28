//! Arguments de la ligne de commande `claude`.

use super::settings::AgentSetup;
use crate::config;
use crate::runners::RunRequest;
use crate::tools::run_args::current_archetype;

/// Limite de ligne de commande Windows (~32 Kio) : une persona plus longue
/// est ajoutée au début du prompt (stdin) à la place.
const MAX_PERSONA_ARG: usize = 20_000;
const DEFAULT_PERMISSIONS: &str = "--dangerously-skip-permissions";

/// Archétypes en lecture seule (PLAN §4.3) : ils cherchent ou relisent, sans écrire.
const RESTRICTED_ARCHETYPES: &[&str] = &["explorer", "verifier"];
/// Outils de lecture autorisés sans invite pour ces archétypes (`--allowedTools`).
const READ_ONLY_TOOLS: &str = "Read,Grep,Glob,LS,WebFetch,WebSearch";

/// Arguments de permissions d'un tour. Un archétype restreint passe toujours en
/// lecture seule, même si `CLAUDE_PERMISSIONS` demande le contraire : la restriction
/// d'archétype est un plancher de sécurité (PLAN §4.3), pas un simple défaut.
fn permission_args(archetype: Option<&str>, configured: Option<&str>) -> Vec<String> {
    if archetype.is_some_and(|a| RESTRICTED_ARCHETYPES.contains(&a)) {
        vec!["--allowedTools".into(), READ_ONLY_TOOLS.into()]
    } else {
        configured.unwrap_or(DEFAULT_PERMISSIONS).split_whitespace().map(String::from).collect()
    }
}

/// `(arguments, prompt envoyé sur stdin)`.
pub fn build(
    req: &RunRequest,
    setup: &AgentSetup,
    model: Option<&str>,
    resume: Option<&str>,
) -> (Vec<String>, String) {
    let mut args: Vec<String> = ["-p", "--output-format", "json"].map(String::from).to_vec();
    let configured = config::app_var("CLAUDE_PERMISSIONS");
    args.extend(permission_args(current_archetype().as_deref(), configured.as_deref()));
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
    use super::AgentSetup;
    use super::{DEFAULT_PERMISSIONS, apply_persona, build, permission_args};
    use crate::agents::Runner;
    use crate::config;
    use crate::runners::RunRequest;
    use crate::tools::run_args::with_archetype;
    use std::path::PathBuf;

    fn req() -> RunRequest {
        RunRequest {
            runner: Runner::Claude,
            prompt: "salut".into(),
            agent: None,
            session_id: None,
            auto_resume: false,
            model: None,
            provider: None,
            cwd: None,
            workspace: PathBuf::from("."),
        }
    }

    #[test]
    fn restricted_archetypes_get_read_only_tools_instead_of_skip() {
        for archetype in ["explorer", "verifier"] {
            let args = permission_args(Some(archetype), Some(DEFAULT_PERMISSIONS));
            assert_eq!(args, vec!["--allowedTools", "Read,Grep,Glob,LS,WebFetch,WebSearch"]);
        }
    }

    #[test]
    fn unrestricted_archetypes_keep_the_configured_permissions() {
        assert_eq!(
            permission_args(Some("coder"), Some("--dangerously-skip-permissions")),
            vec!["--dangerously-skip-permissions"]
        );
        assert_eq!(permission_args(None, None), vec!["--dangerously-skip-permissions"]);
        assert_eq!(
            permission_args(Some("coordinator"), Some("--allowedTools Read")),
            vec!["--allowedTools", "Read"]
        );
    }

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

    #[test]
    fn restricted_archetype_run_gets_allowed_tools_and_no_skip() {
        let setup = AgentSetup::default();
        let (args, _) = with_archetype(Some("explorer"), || build(&req(), &setup, None, None));
        let joined = args.join(" ");
        assert!(joined.contains("--allowedTools"), "{joined}");
        assert!(!joined.contains("dangerously-skip-permissions"), "{joined}");
    }

    #[test]
    fn plain_run_keeps_the_effective_permissions() {
        let setup = AgentSetup::default();
        let configured = config::app_var("CLAUDE_PERMISSIONS");
        let expected = permission_args(None, configured.as_deref());
        let (args, _) = with_archetype(Some("coder"), || build(&req(), &setup, None, None));
        let mut head = vec!["-p".into(), "--output-format".into(), "json".into()];
        head.extend(expected);
        assert!(args.starts_with(&head), "{args:?}");
    }
}
