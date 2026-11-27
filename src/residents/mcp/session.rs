//! Outil `session` : voir, oublier ou forcer la session de l'agent.
//!
//! `reset` passe par [`Ctx::reset_session`] (le même chemin que le pont HTTP) : demandé
//! pendant un tour, il est appliqué **à la fin** de ce tour — le tour enregistre sa session
//! en se terminant et annulerait sinon l'oubli.

use crate::residents::ctx::Ctx;
use crate::sessions;
use litemcp::prelude::*;
use litemcp::zod;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

const DESCRIPTION: &str = "Gère la session de cet agent : show (la session reprise), reset \
    (l'oublie — le prochain tour repart à neuf), set (force l'identifiant de session). Un reset \
    demandé pendant un tour est appliqué à la fin de ce tour.";

#[derive(Deserialize)]
struct Args {
    action: String,
    id: Option<String>,
}

pub fn register(builder: ServerBuilder, ctx: Arc<Ctx>) -> ServerBuilder {
    let schema = zod::object()
        .field(
            "action",
            zod::describe("show | reset | set", zod::enumeration(&["show", "reset", "set"])),
        )
        .opt("id", zod::describe("Identifiant de session (obligatoire pour set)", zod::string()))
        .strict();
    builder
        .tool_parse("session", DESCRIPTION, schema, move |args: Args| handle(&ctx, args))
        .annotate("session", ToolAnnotations::new().title("Session de l'agent").read_only(false))
}

fn handle(ctx: &Ctx, args: Args) -> ToolResult {
    match args.action.as_str() {
        "show" => {
            let session = sessions::get(&ctx.workspace, ctx.runner.as_str(), &ctx.agent);
            Ok(json!({ "action": "show", "session": session }))
        }
        "reset" => {
            let had_session = ctx.reset_session();
            Ok(json!({ "action": "reset", "hadSession": had_session, "session": null }))
        }
        "set" => {
            let id = wanted_id(args.id.as_deref()).map_err(ToolError::invalid_input)?;
            sessions::save(&ctx.workspace, ctx.runner.as_str(), &ctx.agent, id)
                .map_err(|e| ToolError::internal(format!("session non enregistrée : {e}")))?;
            Ok(json!({ "action": "set", "session": id }))
        }
        other => {
            Err(ToolError::invalid_input(format!("action inconnue '{other}' (show | reset | set)")))
        }
    }
}

/// `set` exige un identifiant : absent, vide ou blanc → erreur explicite.
fn wanted_id(id: Option<&str>) -> Result<&str, String> {
    match id {
        Some(id) if !id.trim().is_empty() => Ok(id),
        _ => Err("action 'set' : 'id' est obligatoire (identifiant de session non vide)".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::{Args, handle, wanted_id};
    use crate::agents::Runner;
    use crate::residents::ctx::Ctx;
    use crate::sessions;
    use serde_json::json;
    use std::sync::Arc;

    fn temp_ctx(name: &str) -> Arc<Ctx> {
        let ws =
            std::env::temp_dir().join(format!("swarm-mcp-session-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&ws);
        Arc::new(Ctx::new("alice", Runner::Claude, ws, "test".into()))
    }

    fn args(action: &str, id: Option<&str>) -> Args {
        Args { action: action.into(), id: id.map(String::from) }
    }

    #[test]
    fn set_refuses_a_missing_id() {
        let err = wanted_id(None).unwrap_err();
        assert!(err.contains("obligatoire"), "message explicite attendu, reçu : {err}");
    }

    #[test]
    fn set_refuses_an_empty_or_blank_id() {
        assert!(wanted_id(Some("")).is_err());
        assert!(wanted_id(Some("   ")).is_err());
    }

    #[test]
    fn set_keeps_a_valid_id_as_given() {
        assert_eq!(wanted_id(Some("sess-42")).unwrap(), "sess-42");
    }

    #[test]
    fn show_set_and_reset_round_trip() {
        let ctx = temp_ctx("aller-retour");
        let shown = handle(&ctx, args("show", None)).unwrap();
        assert_eq!(shown["action"], json!("show"));
        assert!(shown["session"].is_null());

        let reset = handle(&ctx, args("reset", None)).unwrap();
        assert_eq!(reset["action"], json!("reset"));
        assert_eq!(reset["hadSession"], json!(false));
        assert!(reset["session"].is_null());

        let set = handle(&ctx, args("set", Some("sess-42"))).unwrap();
        assert_eq!(set["action"], json!("set"));
        assert_eq!(set["session"], json!("sess-42"));
        assert_eq!(sessions::get(&ctx.workspace, "claude", "alice").as_deref(), Some("sess-42"));
        assert_eq!(handle(&ctx, args("show", None)).unwrap()["session"], json!("sess-42"));

        let reset = handle(&ctx, args("reset", None)).unwrap();
        assert_eq!(reset["hadSession"], json!(true));
        assert!(sessions::get(&ctx.workspace, "claude", "alice").is_none());
        assert!(handle(&ctx, args("show", None)).unwrap()["session"].is_null());
        let _ = std::fs::remove_dir_all(&ctx.workspace);
    }

    #[test]
    fn set_without_an_id_fails_through_the_handler() {
        let ctx = temp_ctx("sans-id");
        assert!(handle(&ctx, args("set", None)).is_err());
        assert!(handle(&ctx, args("set", Some("  "))).is_err());
        assert!(sessions::get(&ctx.workspace, "claude", "alice").is_none());
        let _ = std::fs::remove_dir_all(&ctx.workspace);
    }
}
