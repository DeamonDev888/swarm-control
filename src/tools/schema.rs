//! Briques de schéma d'entrée partagées entre les outils.

use litemcp::zod::{self, Obj, Schema};

pub const RUNNERS: &[&str] = &["claude", "hermes", "antigravity"];

pub fn name_schema(desc: &str) -> impl Schema + 'static {
    zod::describe(desc, zod::string().min(1).max(128))
}

pub fn runner_opt() -> impl Schema + 'static {
    zod::describe("Runner (si l'agent existe sur plusieurs runners)", zod::enumeration(RUNNERS))
}

pub fn config_opt() -> impl Schema + 'static {
    zod::describe("Racine du workspace (défaut: workspace global)", zod::string().min(1))
}

pub fn str_array(desc: &str) -> impl Schema + 'static {
    zod::describe(desc, zod::array(zod::string().min(1)))
}

/// Champs d'un run (`run_agent` et chaque entrée de `run_agents_parallel`).
pub fn run_object() -> Obj {
    zod::object()
        .field(
            "runner",
            zod::describe(
                "claude = Claude Code, hermes = Nous Hermes, antigravity = Antigravity CLI (agy)",
                zod::enumeration(RUNNERS),
            ),
        )
        .field("prompt", zod::describe("Instruction envoyée à l'agent", zod::string().min(1)))
        .opt(
            "agentName",
            name_schema(
                "Agent persistant (créé par create_agent). Sa session est reprise automatiquement. \
                 Sans agentName : agent éphémère — un tour, rien de sauvegardé.",
            ),
        )
        .opt(
            "sessionId",
            zod::describe("Force la reprise d'une session précise", zod::string().min(1)),
        )
        .opt(
            "autoResume",
            zod::describe(
                "Reprendre la dernière session de l'agent (défaut: true)",
                zod::boolean(),
            ),
        )
        .opt("model", zod::describe("Modèle (override du modèle de l'agent)", zod::string().min(1)))
        .opt("provider", zod::describe("Provider Hermes (override)", zod::string().min(1)))
        .opt(
            "path",
            zod::describe("Répertoire de travail du run (défaut: workspace)", zod::string().min(1)),
        )
        .opt("config", config_opt())
}
