//! Outil `create_agent` : crée un agent persistant (et le rend résident si demandé),
//! éventuellement à partir d'un modèle ; outil `list_templates` enregistré ici aussi
//! (les modèles vivent dans `agents::templates`).

use super::errors::{invalid, parse_runner};
use super::schema::{config_opt, name_schema, runner_opt};
use crate::agents::{self, CreateArgs, Runner, compile, executable, templates};
use crate::{config, residents};
use litemcp::prelude::*;
use litemcp::zod;
use serde::Deserialize;
use serde_json::json;
use std::path::{Path, PathBuf};

const DESCRIPTION: &str = "Crée un agent persistant. Claude : profil agents/<nom>/.claude (prompt, \
    settings, MCP). Hermes : profil natif (hermes profile create) + SOUL.md. Antigravity : profil \
    agents/<nom>/.antigravity (CLI agy). Le prompt définit sa persona ; avec template, la persona \
    du modèle est utilisée (prompt la remplace) et ses variables requises sont vérifiées — voir \
    list_templates. resident:true le rend aussitôt résident (serveur MCP propre, file de \
    demandes). executable:true copie le binaire courant dans <workspace>/bin/<nom> ; unit:true \
    (Linux) écrit en plus une unit systemd --headless (écrite, non activée).";

const LIST_DESCRIPTION: &str = "Liste les modèles d'agents : intégrés (assets/templates/) et \
    surcharges du workspace (<workspace>/templates/) — nom, description, classe, variables \
    requises, serveurs MCP attendus et défauts. `create_agent {template}` les instancie, \
    `doctor --agent X` vérifie leurs prérequis.";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Args {
    name: String,
    prompt: Option<String>,
    template: Option<String>,
    runner: Option<String>,
    model: Option<String>,
    copy_env_from: Option<String>,
    clone_from: Option<String>,
    #[serde(default)]
    resident: bool,
    executable: Option<bool>,
    unit: Option<bool>,
    config: Option<String>,
}

#[derive(Deserialize)]
struct ListArgs {
    config: Option<String>,
}

pub fn register(builder: ServerBuilder) -> ServerBuilder {
    let schema = zod::object()
        .field("name", name_schema("Nom unique [a-zA-Z0-9_-] (Hermes : minuscules uniquement)"))
        .opt(
            "prompt",
            zod::describe(
                "Prompt système (persona, missions, outils autorisés). Avec template : remplace \
                 la persona du modèle",
                zod::string().min(1),
            ),
        )
        .opt(
            "template",
            zod::describe(
                "Modèle d'agent à instancier (voir list_templates) : persona, serveurs MCP \
                 attendus, variables requises, défauts executable/unit/port de pont. \
                 Surchargeable par <workspace>/templates/<nom>.json",
                zod::string().min(1),
            ),
        )
        .opt("runner", runner_opt())
        .opt("model", zod::describe("Modèle ou surnom (voir model-aliases.json)", zod::string().min(1)))
        .opt("copyEnvFrom", zod::describe("Claude : settings.json existant dont copier env + serveurs MCP", zod::string().min(1)))
        .opt("cloneFrom", zod::describe("Hermes : profil existant dont copier config, .env, SOUL, skills (les credentials sont par profil)", zod::string().min(1)))
        .opt("resident", zod::describe("Rendre l'agent résident tout de suite", zod::boolean()))
        .opt(
            "executable",
            zod::describe(
                "Compile un binaire dédié à l'agent (crate généré sous <workspace>/agents/<nom>/build/, \
                 puis cargo build --release en arrière-plan) et le pose dans <workspace>/bin/<nom> ; \
                 delete_agent le retirera. Sans valeur : défaut du template",
                zod::boolean(),
            ),
        )
        .opt(
            "unit",
            zod::describe(
                "Écrire en plus une unit systemd --headless dans <workspace>/bridge/units \
                 (non activée, jamais /etc ; fichier à copier sur l'hôte Linux). \
                 Sans valeur : défaut du template",
                zod::boolean(),
            ),
        )
        .opt("config", config_opt())
        .strict();
    let builder = builder.tool_parse("create_agent", DESCRIPTION, schema, handle);
    let list_schema = zod::object().opt("config", config_opt()).strict();
    builder.tool_parse("list_templates", LIST_DESCRIPTION, list_schema, list_handle).annotate(
        "list_templates",
        ToolAnnotations::new().title("Lister les modèles d'agents").read_only(true),
    )
}

fn handle(a: Args) -> ToolResult {
    let ws = config::workspace_for(a.config.as_deref());
    // Le modèle se charge avant toute écriture : une erreur ne crée rien.
    let loaded = match a.template.as_deref() {
        Some(name) => Some(templates::load(&ws, name).map_err(invalid)?),
        None => None,
    };
    // Runner : explicite > défaut du modèle (déjà validé au chargement) > claude.
    let runner = a
        .runner
        .as_deref()
        .map(parse_runner)
        .transpose()?
        .or_else(|| loaded.as_ref().and_then(|l| l.runner))
        .unwrap_or(Runner::Claude);
    // Variables requises : absentes → refus avant création (l'agent serait inutilisable).
    if let Some(l) = &loaded {
        let missing = templates::missing_vars(&l.template);
        if !missing.is_empty() {
            return Err(invalid(format!(
                "template '{}' : variable(s) requise(s) absente(s) : {} — définit-les dans \
                 l'environnement ou le .env du workspace, puis relance.",
                l.template.name,
                missing.join(", ")
            )));
        }
    }
    // Persona : prompt explicite, sinon celle du modèle — l'un des deux est obligatoire.
    let prompt = match a.prompt.as_deref() {
        Some(p) => p.to_string(),
        None => match &loaded {
            Some(l) => l.prompt.clone(),
            None => {
                return Err(invalid(
                    "prompt ou template requis : donne un prompt, ou un template (voir \
                     list_templates).",
                ));
            }
        },
    };
    if agents::exists_for(&ws, &a.name, runner) {
        return Err(invalid(format!(
            "L'agent '{}' existe déjà ({}). update_agent_config pour le modifier, ou delete_agent d'abord.",
            a.name,
            runner.as_str()
        )));
    }
    let created = agents::create(
        &ws,
        &CreateArgs {
            name: &a.name,
            prompt: &prompt,
            runner,
            model: a.model.as_deref(),
            copy_env_from: a.copy_env_from.as_deref(),
            clone_from: a.clone_from.as_deref(),
        },
    )
    .map_err(ToolError::invalid_input)?;

    // Défauts du modèle : l'appel explicite gagne, sinon la valeur du modèle, sinon false.
    let executable_wanted =
        effective(a.executable, loaded.as_ref().is_some_and(|l| l.template.defaults.executable));
    let unit_wanted = effective(a.unit, loaded.as_ref().is_some_and(|l| l.template.defaults.unit));

    let mut out = json!({
        "created": a.name,
        "runner": runner.as_str(),
        "promptPath": created.prompt_path.display().to_string(),
        "configPath": created.config_path.display().to_string(),
        "next": format!("run_agent(runner:\"{}\", agentName:\"{}\", prompt:\"…\")", runner.as_str(), a.name),
    });
    if !created.warnings.is_empty() {
        out["warnings"] = json!(created.warnings);
    }
    // La marque (template + classe + port de pont conseillé) ne bloque jamais la
    // création : signalée sinon. L'application du port au lancement (défaut de --bridge)
    // est une autre vague : elle lira templates::bridge_port_of.
    if let (Some(name), Some(l)) = (a.template.as_deref(), loaded.as_ref()) {
        match templates::stamp_with_port(
            &ws,
            &a.name,
            runner,
            name,
            l.class(),
            l.template.defaults.bridge_port,
        ) {
            Ok(()) => {
                out["template"] = json!({
                    "name": l.template.name,
                    "class": l.template.class,
                    "source": l.source.as_str(),
                    "expectedMcpServers": l.template.expected_mcp_servers,
                    "bridgePort": l.template.defaults.bridge_port,
                });
            }
            Err(e) => out["templateError"] = json!(e),
        }
    }
    if executable_wanted || unit_wanted {
        if unit_wanted {
            ensure_unit_supported()?;
        }
        let exe = executable::path(&ws, &a.name);
        if unit_wanted {
            let unit_file = write_service(&ws, &a.name, &exe)?;
            out["unit"] = json!({
                "path": unit_file.display().to_string(),
                "installHint": executable::install_hint(&a.name),
            });
        }
        out["executable"] = json!(exe.display().to_string());
        match install_binary(
            &ws,
            &a.name,
            &compile_args(
                &a,
                &prompt,
                runner,
                loaded.as_ref().and_then(|l| l.class()).unwrap_or("custom"),
            ),
        ) {
            Ok(build) => out["build"] = build,
            Err(message) => out["executableError"] = json!(message),
        }
    }
    if a.resident {
        add_residency(&mut out, &ws, runner, &a.name);
    }
    Ok(out)
}

/// Valeur effective d'un drapeau : explicite, sinon défaut du template, sinon false.
fn effective(explicit: Option<bool>, template_default: bool) -> bool {
    explicit.unwrap_or(template_default)
}

fn list_handle(a: ListArgs) -> ToolResult {
    let ws = config::workspace_for(a.config.as_deref());
    let items: Vec<serde_json::Value> =
        templates::list(&ws).iter().map(templates::TemplateInfo::to_json).collect();
    Ok(json!({ "count": items.len(), "templates": items }))
}

/// L'unit systemd n'a de sens que sous Linux : hors Linux, l'option est
/// refusée avant toute copie de binaire, avec l'erreur claire attendue.
#[cfg(target_os = "linux")]
fn ensure_unit_supported() -> Result<(), ToolError> {
    Ok(())
}

/// Voir `ensure_unit_supported` : le message dit ce qui existe déjà.
#[cfg(not(target_os = "linux"))]
fn ensure_unit_supported() -> Result<(), ToolError> {
    Err(invalid(
        "unit:true : non supporté ici (Linux uniquement). L'agent est créé avec ses fichiers ; \
         relancez sans unit, ou générez l'unité sur l'hôte Linux.",
    ))
}

/// Arguments de `create_agent` réduits à ce qui entre dans la définition par défaut
/// embarquée (`agent.json` du crate généré) : jamais un identifiant, jamais un env.
fn compile_args(a: &Args, prompt: &str, runner: Runner, class: &str) -> serde_json::Value {
    json!({
        "name": a.name,
        "class": class,
        "prompt": prompt,
        "runner": runner.as_str(),
        "model": a.model,
        "template": a.template,
    })
}

/// Lance la compilation du binaire dédié (PLAN §4.8) : le crate est généré sous
/// `<ws>/agents/<nom>/build/`, puis `cargo build --release` tourne en arrière-plan
/// (journal dans `<ws>/logs/build-<nom>.log`). L'agent est créé quoi qu'il arrive :
/// sans `cargo`, ou avec `SWARM_SKIP_AGENT_BUILD=1` (tests, CI sans Rust), l'exécutable
/// manque et l'erreur le dit — rendue comme message, jamais comme échec de l'outil.
fn install_binary(
    ws: &Path,
    name: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    compile::refuse_secrets(args)?;
    let class = args.get("class").and_then(serde_json::Value::as_str).unwrap_or("custom");
    let dir = compile::generate(ws, name, class, env!("CARGO_PKG_VERSION"), args)?;
    let mut build = json!({
        "buildDir": dir.display().to_string(),
        "log": compile::log_path(ws, name).display().to_string(),
    });
    if std::env::var_os("SWARM_SKIP_AGENT_BUILD").is_some() {
        build["status"] = json!("skipped");
        return Ok(build);
    }
    compile::spawn_build(ws, name, compile::BuildOptions::default())?;
    build["status"] = json!("building");
    Ok(build)
}

/// Même contrat : binaire en place, unit manquante ne doit pas faire croire à un échec total.
fn write_service(ws: &Path, name: &str, exe: &Path) -> Result<PathBuf, ToolError> {
    executable::write_unit(ws, name, exe).map_err(|e| {
        invalid(format!(
            "L'agent '{name}' est créé et son exécutable installé ('{}'), mais l'unit systemd \
             n'a pas pu être écrite : {e}.",
            exe.display()
        ))
    })
}

/// L'agent est créé quoi qu'il arrive : un échec de résidence est signalé, pas fatal.
fn add_residency(out: &mut serde_json::Value, ws: &Path, runner: Runner, name: &str) {
    match residents::start(ws, runner, name) {
        Ok(info) => out["resident"] = info.to_json(),
        Err(e) => out["residentError"] = json!(e),
    }
}

#[cfg(test)]
mod tests {
    use super::effective;

    #[test]
    fn l_appel_explicite_gagne_sur_le_defaut_du_template() {
        assert!(!effective(Some(false), true), "false explicite : le template ne force pas");
        assert!(effective(None, true), "absent : défaut du template");
        assert!(effective(Some(true), false));
        assert!(!effective(None, false));
    }
}
