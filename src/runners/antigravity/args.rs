//! Arguments de la ligne de commande `agy`.

use crate::config;
use crate::runners::RunRequest;

const DEFAULT_PERMISSIONS: &str = "--dangerously-skip-permissions";

/// Arguments passés à `agy` en mode headless (`-p <prompt> --output-format json`).
pub fn build(
    req: &RunRequest,
    persona: Option<&str>,
    model: Option<&str>,
    resume: Option<&str>,
) -> Vec<String> {
    let prompt = with_persona(persona, &req.prompt, resume);
    let mut args = vec!["-p".into(), prompt, "--output-format".into(), "json".into()];
    let permissions =
        config::app_var("ANTIGRAVITY_PERMISSIONS").unwrap_or_else(|| DEFAULT_PERMISSIONS.into());
    args.extend(permissions.split_whitespace().map(String::from));
    push_pair(&mut args, "--conversation", resume);
    push_pair(&mut args, "--model", model);
    args
}

fn push_pair(args: &mut Vec<String>, flag: &str, value: Option<&str>) {
    if let Some(v) = value {
        args.push(flag.into());
        args.push(v.into());
    }
}

/// Préfixe la persona uniquement à la première session : une conversation reprise
/// la possède déjà dans son historique.
fn with_persona(persona: Option<&str>, prompt: &str, resume: Option<&str>) -> String {
    match persona {
        Some(p) if resume.is_none() => format!("{p}\n\n---\n\n{prompt}"),
        _ => prompt.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{build, with_persona};
    use crate::agents::Runner;
    use crate::runners::RunRequest;
    use std::path::PathBuf;

    fn req(prompt: &str) -> RunRequest {
        RunRequest {
            runner: Runner::Antigravity,
            prompt: prompt.into(),
            agent: Some("nova".into()),
            session_id: None,
            auto_resume: true,
            model: None,
            provider: None,
            cwd: None,
            workspace: PathBuf::from("."),
        }
    }

    #[test]
    fn persona_prefixes_only_fresh_conversations() {
        assert_eq!(
            with_persona(Some("Tu es Nova"), "bonjour", None),
            "Tu es Nova\n\n---\n\nbonjour"
        );
        assert_eq!(with_persona(Some("Tu es Nova"), "encore", Some("conv-1")), "encore");
        assert_eq!(with_persona(None, "bonjour", None), "bonjour");
    }

    #[test]
    fn build_includes_conversation_and_model_when_set() {
        let args = build(&req("salut"), Some("Persona"), Some("gemini-4-neon-medium"), Some("c-9"));
        assert_eq!(args[0], "-p");
        assert_eq!(args[1], "salut");
        assert!(args.windows(2).any(|w| w == ["--output-format", "json"]));
        assert!(args.windows(2).any(|w| w == ["--conversation", "c-9"]));
        assert!(args.windows(2).any(|w| w == ["--model", "gemini-4-neon-medium"]));
    }
}
