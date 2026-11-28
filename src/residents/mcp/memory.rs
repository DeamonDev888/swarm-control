//! Outils `memory_store` / `memory_search` / `memory_forget` : la mémoire native de l'agent
//! (PLAN §4.6), portée par [`crate::memory`].
//!
//! Le fichier est celui de l'agent (`bridge/memory/<runner>-<agent>.jsonl`) ; un souvenir qui
//! contient un jeton est refusé avant toute écriture. `scope: "swarm"` ne lit **que** les
//! fichiers voisins (les autres agents du même workspace) : cet outil n'écrit jamais ailleurs
//! que dans le fichier de l'agent.

use crate::memory::{self, Hit, Scope};
use crate::residents::ctx::Ctx;
use litemcp::prelude::*;
use litemcp::zod;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

const STORE_DESCRIPTION: &str = "Mémorise un fait pour cet agent (préférence, décision, leçon, \
    lien…) : un texte court, des mots-clés facultatifs. Rend l'id du souvenir, pour \
    memory_forget. Un texte ou un tag contenant une clé ou un jeton est refusé.";
const SEARCH_DESCRIPTION: &str = "Cherche dans la mémoire de cet agent par mots-clés (majuscules \
    et accents ignorés), les plus pertinents d'abord : score = termes trouvés (les tags \
    comptent double) + récence. limit borne les résultats (défaut 10, max 100). scope \
    \"swarm\" lit, en lecture seule, la mémoire des autres agents du même workspace.";
const FORGET_DESCRIPTION: &str = "Oublie un souvenir par son id (rendu par memory_store ou \
    memory_search). ok vaut false si aucun souvenir ne porte cet id.";

/// Source notée sur les souvenirs écrits par les outils MCP du résident.
const SOURCE: &str = "mcp";

#[derive(Deserialize)]
struct StoreArgs {
    text: String,
    #[serde(default)]
    tags: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct SearchArgs {
    query: String,
    #[serde(default)]
    limit: Option<u64>,
    #[serde(default)]
    scope: Option<String>,
}

#[derive(Deserialize)]
struct ForgetArgs {
    id: String,
}

pub fn register(builder: ServerBuilder, ctx: Arc<Ctx>) -> ServerBuilder {
    let builder = register_store(builder, Arc::clone(&ctx));
    let builder = register_search(builder, Arc::clone(&ctx));
    register_forget(builder, ctx)
}

fn register_store(builder: ServerBuilder, ctx: Arc<Ctx>) -> ServerBuilder {
    let schema = zod::object()
        .field(
            "text",
            zod::describe(
                "Le fait à retenir (aucun secret : refusé)",
                zod::string().min(1).max(memory::MAX_TEXT),
            ),
        )
        .opt(
            "tags",
            zod::describe(
                "Mots-clés, comptent double dans la recherche (max 32)",
                zod::array(zod::string().min(1).max(memory::MAX_TAG)).max(memory::MAX_TAGS),
            ),
        )
        .strict();
    builder
        .tool_parse("memory_store", STORE_DESCRIPTION, schema, move |args: StoreArgs| {
            store(&ctx, args)
        })
        .annotate(
            "memory_store",
            ToolAnnotations::new().title("Mémoriser un fait").read_only(false).idempotent(false),
        )
}

fn register_search(builder: ServerBuilder, ctx: Arc<Ctx>) -> ServerBuilder {
    let schema = zod::object()
        .field(
            "query",
            zod::describe(
                "Mots-clés cherchés (casse et accents ignorés)",
                zod::string().min(1).max(512),
            ),
        )
        .opt(
            "limit",
            zod::describe(
                "Nombre de résultats (défaut 10, max 100)",
                zod::uint().min(1).max(memory::MAX_LIMIT as u64),
            ),
        )
        .opt(
            "scope",
            zod::describe(
                "self (défaut) : la mémoire de cet agent ; swarm : lecture seule des autres agents",
                zod::enumeration(&["self", "swarm"]),
            ),
        )
        .strict();
    builder
        .tool_parse("memory_search", SEARCH_DESCRIPTION, schema, move |args: SearchArgs| {
            search(&ctx, args)
        })
        .annotate(
            "memory_search",
            ToolAnnotations::new()
                .title("Chercher dans la mémoire")
                .read_only(true)
                .idempotent(true),
        )
}

fn register_forget(builder: ServerBuilder, ctx: Arc<Ctx>) -> ServerBuilder {
    let schema = zod::object()
        .field("id", zod::describe("Id du souvenir à oublier", zod::string().min(1).max(128)))
        .strict();
    builder
        .tool_parse("memory_forget", FORGET_DESCRIPTION, schema, move |args: ForgetArgs| {
            forget(&ctx, args)
        })
        .annotate(
            "memory_forget",
            ToolAnnotations::new().title("Oublier un souvenir").destructive(true).idempotent(true),
        )
}

fn store(ctx: &Ctx, args: StoreArgs) -> ToolResult {
    let tags = args.tags.unwrap_or_default();
    let stored =
        memory::store(&ctx.workspace, ctx.runner.as_str(), &ctx.agent, &args.text, &tags, SOURCE)
            .map_err(ToolError::invalid_input)?;
    Ok(json!({ "id": stored.id }))
}

fn search(ctx: &Ctx, args: SearchArgs) -> ToolResult {
    let scope = match args.scope.as_deref().map(str::trim) {
        None | Some("self") => Scope::Own,
        Some(other) => Scope::parse(other).ok_or_else(|| {
            ToolError::invalid_input(format!("scope inconnu '{other}' (self | swarm)"))
        })?,
    };
    let limit = args.limit.map_or(memory::DEFAULT_LIMIT, |n| n as usize);
    let hits =
        memory::search(&ctx.workspace, ctx.runner.as_str(), &ctx.agent, &args.query, limit, scope)
            .map_err(ToolError::invalid_input)?;
    Ok(rendered(&hits, scope))
}

fn forget(ctx: &Ctx, args: ForgetArgs) -> ToolResult {
    let ok = memory::forget(&ctx.workspace, ctx.runner.as_str(), &ctx.agent, args.id.trim())
        .map_err(ToolError::internal)?;
    Ok(json!({ "ok": ok }))
}

/// Réponse de `memory_search` : `{results: [{id, ts, text, tags, score, agent, runner}], scope}`,
/// dans l'ordre du score.
fn rendered(hits: &[Hit], scope: Scope) -> Value {
    json!({ "results": hits.iter().map(Hit::to_json).collect::<Vec<_>>(), "scope": scope.as_str() })
}

#[cfg(test)]
mod tests {
    use super::{ForgetArgs, SearchArgs, StoreArgs, forget, rendered, search, store};
    use crate::agents::Runner;
    use crate::memory::Scope;
    use crate::residents::ctx::Ctx;
    use serde_json::json;
    use std::sync::Arc;

    fn temp_ctx(name: &str) -> Arc<Ctx> {
        let ws =
            std::env::temp_dir().join(format!("swarm-mcp-memory-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&ws);
        Arc::new(Ctx::new("alice", Runner::Claude, ws, "test".into()))
    }

    fn tags(items: &[&str]) -> Option<Vec<String>> {
        Some(items.iter().map(|t| t.to_string()).collect())
    }

    #[test]
    fn store_then_search_then_forget_through_the_tools() {
        let ctx = temp_ctx("aller-retour");
        let stored =
            store(&ctx, StoreArgs { text: " Le port est 23090 ".into(), tags: tags(&["réseau"]) })
                .unwrap();
        assert_eq!(stored.as_object().unwrap().len(), 1, "la réponse est {stored}");
        let id = stored["id"].as_str().unwrap().to_string();
        assert!(!id.is_empty());

        let found =
            search(&ctx, SearchArgs { query: "PORT".into(), limit: None, scope: None }).unwrap();
        assert_eq!(found["scope"], json!("self"));
        assert_eq!(found["results"].as_array().unwrap().len(), 1);
        let first = &found["results"][0];
        assert_eq!(first["id"], json!(id));
        assert_eq!(first["text"], json!("Le port est 23090"));
        assert_eq!(first["tags"], json!(["réseau"]));
        assert_eq!(first["agent"], json!("alice"));
        assert_eq!(first["runner"], json!("claude"));
        assert!(first["ts"].as_u64().unwrap() > 0);
        assert!(
            first["score"].as_f64().unwrap() >= 2.0,
            "tag + texte + récence : {}",
            first["score"]
        );

        // Le fichier est bien celui de l'agent, source `mcp`.
        let line = std::fs::read_to_string(ctx.workspace.join("bridge/memory/claude-alice.jsonl"))
            .unwrap();
        assert!(line.contains("\"source\":\"mcp\""), "{line}");

        let gone = forget(&ctx, ForgetArgs { id: id.clone() }).unwrap();
        assert_eq!(gone, json!({ "ok": true }));
        assert_eq!(forget(&ctx, ForgetArgs { id }).unwrap(), json!({ "ok": false }), "déjà oublié");
        let none =
            search(&ctx, SearchArgs { query: "port".into(), limit: None, scope: None }).unwrap();
        assert_eq!(none["results"], json!([]));
        let _ = std::fs::remove_dir_all(&ctx.workspace);
    }

    #[test]
    fn a_secret_is_refused_as_invalid_input() {
        let ctx = temp_ctx("jeton");
        let error =
            store(&ctx, StoreArgs { text: "clé sk-abc123def456ghi789jkl".into(), tags: None })
                .unwrap_err();
        assert!(error.message.starts_with("SECRET:"), "{error:?}");
        assert!(!error.message.contains("sk-abc123"), "le jeton n'est pas répété : {error:?}");
        assert!(!ctx.workspace.join("bridge/memory").exists(), "rien n'a été écrit");
        let _ = std::fs::remove_dir_all(&ctx.workspace);
    }

    #[test]
    fn a_query_without_terms_or_an_unknown_scope_is_an_invalid_input() {
        let ctx = temp_ctx("requête");
        let error = search(&ctx, SearchArgs { query: " !! ".into(), limit: None, scope: None })
            .unwrap_err();
        assert!(error.message.contains("sans terme"), "{error:?}");
        let error = search(
            &ctx,
            SearchArgs { query: "port".into(), limit: None, scope: Some("team".into()) },
        )
        .unwrap_err();
        assert!(error.message.contains("scope inconnu"), "{error:?}");
        let _ = std::fs::remove_dir_all(&ctx.workspace);
    }

    #[test]
    fn swarm_scope_reads_the_neighbours_but_never_writes_them() {
        let ctx = temp_ctx("essaim");
        // bob (hermes) et carol (claude) ont une mémoire dans le même workspace.
        for (runner, agent, text) in [
            ("hermes", "bob", "bob note que le port 23091 répond"),
            ("claude", "carol", "carol : le port 23092 est libre"),
        ] {
            crate::memory::store(&ctx.workspace, runner, agent, text, &[], "mcp").unwrap();
        }
        store(&ctx, StoreArgs { text: "alice sait que le port est 23090".into(), tags: None })
            .unwrap();
        let own =
            search(&ctx, SearchArgs { query: "port".into(), limit: None, scope: None }).unwrap();
        assert_eq!(own["results"].as_array().unwrap().len(), 1);
        assert_eq!(own["results"][0]["agent"], json!("alice"));

        let swarm = search(
            &ctx,
            SearchArgs { query: "port".into(), limit: None, scope: Some("swarm".into()) },
        )
        .unwrap();
        assert_eq!(swarm["scope"], json!("swarm"));
        let agents: Vec<&str> = swarm["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["agent"].as_str().unwrap())
            .collect();
        assert_eq!(agents.len(), 2, "{agents:?}");
        assert!(agents.contains(&"bob") && agents.contains(&"carol"), "jamais alice : {agents:?}");
        // La limite s'applique à l'essaim entier.
        let one = search(
            &ctx,
            SearchArgs { query: "port".into(), limit: Some(1), scope: Some("swarm".into()) },
        )
        .unwrap();
        assert_eq!(one["results"].as_array().unwrap().len(), 1);
        // Rien n'a été écrit chez les voisins.
        assert_eq!(crate::memory::read(&ctx.workspace, "hermes", "bob").len(), 1);
        assert_eq!(crate::memory::read(&ctx.workspace, "claude", "carol").len(), 1);
        let _ = std::fs::remove_dir_all(&ctx.workspace);
    }

    #[test]
    fn rendered_lists_results_in_score_order_with_the_scope() {
        let ctx = temp_ctx("rendu");
        crate::memory::store(&ctx.workspace, "claude", "alice", "le port", &[], "mcp").unwrap();
        crate::memory::store(&ctx.workspace, "claude", "alice", "le port du serveur", &[], "mcp")
            .unwrap();
        let hits = crate::memory::search(
            &ctx.workspace,
            "claude",
            "alice",
            "port serveur",
            10,
            Scope::Own,
        )
        .unwrap();
        let out = rendered(&hits, Scope::Own);
        assert_eq!(out["scope"], json!("self"));
        assert_eq!(out["results"][0]["text"], json!("le port du serveur"), "deux termes devant un");
        assert_eq!(out["results"][1]["text"], json!("le port"));
        assert_eq!(rendered(&[], Scope::Swarm)["results"], json!([]));
        let _ = std::fs::remove_dir_all(&ctx.workspace);
    }
}
