//! Arguments de la ligne de commande `hermes`.

use crate::runners::RunRequest;

/// Options globales avant `chat`, options de `chat` après.
const CHAT: [&str; 8] =
    ["chat", "--query-file", "-", "-Q", "--yolo", "--pass-session-id", "--source", "tool"];

pub fn build(req: &RunRequest, resume: Option<&str>) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    push_pair(&mut args, "-p", req.agent.as_deref());
    args.extend(CHAT.map(String::from));
    push_pair(&mut args, "--model", req.model.as_deref());
    push_pair(&mut args, "--provider", req.provider.as_deref());
    push_pair(&mut args, "--resume", resume);
    args
}

fn push_pair(args: &mut Vec<String>, flag: &str, value: Option<&str>) {
    if let Some(v) = value {
        args.push(flag.into());
        args.push(v.into());
    }
}

#[cfg(test)]
mod tests {
    use super::build;
    use crate::agents::Runner;
    use crate::runners::RunRequest;

    fn request(agent: Option<&str>) -> RunRequest {
        RunRequest {
            runner: Runner::Hermes,
            prompt: "p".into(),
            agent: agent.map(String::from),
            session_id: None,
            auto_resume: false,
            model: Some("glm-5.3-flash".into()),
            provider: None,
            cwd: None,
            workspace: ".".into(),
        }
    }

    #[test]
    fn profile_comes_before_chat_and_resume_last() {
        let args = build(&request(Some("hera")), Some("local-abc"));
        assert_eq!(&args[..3], ["-p", "hera", "chat"]);
        assert!(args.windows(2).any(|w| w == ["--model", "glm-5.3-flash"]));
        assert_eq!(&args[args.len() - 2..], ["--resume", "local-abc"]);
    }

    #[test]
    fn anonymous_run_has_no_profile() {
        assert_eq!(build(&request(None), None)[0], "chat");
    }
}
