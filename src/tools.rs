//! Les 8 outils MCP d'OverMind-rs, enregistrés sur un `ServerBuilder` LiteMCP.
//!
//! | outil                | rôle                                                         |
//! |----------------------|--------------------------------------------------------------|
//! | `run_agent`          | lance un agent (Claude Code / Hermes), reprise de session auto |
//! | `run_agents_parallel`| plusieurs agents en parallèle                                |
//! | `agent_control`      | status / stream / wait / kill / list des runs                |
//! | `create_agent`       | crée un agent persistant (prompt + config)                   |
//! | `list_agents`        | liste les agents                                             |
//! | `get_agent_configs`  | lit les fichiers d'un agent                                  |
//! | `update_agent_config`| modifie modèle / MCP / env / prompt / fichier                |
//! | `delete_agent`       | supprime un agent et sa session                              |

use crate::agents::{self, CreateArgs, Runner, Updates};
use crate::config;
use crate::registry::{self, Snapshot, Status};
use crate::runners::{self, RunRequest, RunResult};
use crate::sessions;
use crate::store::now_ms;
use litemcp::cancel::CancelToken;
use litemcp::prelude::*;
use litemcp::zod;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn invalid(msg: impl Into<String>) -> ToolError {
    ToolError::invalid_input(msg)
}

fn parse_runner(s: &str) -> Result<Runner, ToolError> {
    Runner::parse(s).ok_or_else(|| invalid(format!("runner inconnu '{s}' (claude | hermes)")))
}

fn opt_runner(s: &Option<String>) -> Result<Option<Runner>, ToolError> {
    s.as_deref().map(parse_runner).transpose()
}

// ─── Schémas partagés ───────────────────────────────────────────────────────

const RUNNERS: &[&str] = &["claude", "hermes"];

fn name_schema(desc: &str) -> impl litemcp::zod::Schema + 'static {
    zod::describe(desc, zod::string().min(1).max(128))
}

/// Champs d'un run (utilisés par `run_agent` et par chaque entrée de `run_agents_parallel`).
fn run_object() -> zod::Obj {
    zod::object()
        .field("runner", zod::describe("claude = Claude Code, hermes = Nous Hermes", zod::enumeration(RUNNERS)))
        .field("prompt", zod::describe("Instruction envoyée à l'agent", zod::string().min(1)))
        .opt("agentName", name_schema("Agent persistant (créé par create_agent). Sa session est reprise automatiquement."))
        .opt("sessionId", zod::describe("Force la reprise d'une session précise", zod::string().min(1)))
        .opt("autoResume", zod::describe("Reprendre la dernière session de l'agent (défaut: true)", zod::boolean()))
        .opt("model", zod::describe("Modèle (override du modèle de l'agent)", zod::string().min(1)))
        .opt("provider", zod::describe("Provider Hermes (override)", zod::string().min(1)))
        .opt("path", zod::describe("Répertoire de travail du run (défaut: workspace)", zod::string().min(1)))
        .opt("config", zod::describe("Racine OverMind (défaut: workspace global)", zod::string().min(1)))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunArgs {
    runner: String,
    prompt: String,
    agent_name: Option<String>,
    session_id: Option<String>,
    auto_resume: Option<bool>,
    model: Option<String>,
    provider: Option<String>,
    path: Option<String>,
    config: Option<String>,
    #[serde(default)]
    background: bool,
    task_id: Option<String>,
}

impl RunArgs {
    fn request(&self) -> Result<RunRequest, ToolError> {
        let runner = parse_runner(&self.runner)?;
        Ok(RunRequest {
            runner,
            prompt: self.prompt.clone(),
            agent: self.agent_name.clone(),
            session_id: self.session_id.clone(),
            // Agent nommé = agent persistant : on reprend sa session sauf demande contraire.
            auto_resume: self.auto_resume.unwrap_or(self.agent_name.is_some()),
            model: self.model.clone(),
            provider: self.provider.clone(),
            cwd: self.path.clone(),
            workspace: config::workspace_for(self.config.as_deref()),
        })
    }
}

fn run_result_json(req: &RunRequest, r: &RunResult, duration: Duration) -> Value {
    let mut v = json!({
        "runner": req.runner.as_str(),
        "agentName": req.agent,
        "result": r.result,
        "sessionId": r.session_id,
        "durationMs": duration.as_millis() as u64,
    });
    if let Some(m) = &r.model {
        v["model"] = json!(m);
    }
    if let Some(e) = &r.error {
        v["error"] = json!(e);
    }
    v
}

// ─── run_agent ──────────────────────────────────────────────────────────────

fn run_agent(args: RunArgs) -> ToolResult {
    let req = args.request()?;
    let live = runners::prepare(&req).map_err(|r| ToolError::new(ErrorCode::InvalidInput, r.error.unwrap_or_default()))?;
    if args.background {
        let id = live.run_id.clone();
        let agent = req.agent.clone();
        let runner = req.runner;
        let handle = std::thread::Builder::new().name(format!("overmind-run-{id}"));
        let live2 = live.clone();
        handle
            .spawn(move || {
                runners::execute(&req, &live2, None);
            })
            .map_err(|e| ToolError::internal(format!("thread de run impossible: {e}")))?;
        return Ok(json!({
            "runId": id,
            "agentName": agent,
            "runner": runner.as_str(),
            "status": "running",
            "hint": "suivre avec agent_control (status | stream | wait | kill)",
        }));
    }
    let start = Instant::now();
    let cancel = litemcp::cancel::token();
    let result = runners::execute(&req, &live, cancel.as_ref());
    let out = run_result_json(&req, &result, start.elapsed());
    match &result.error {
        Some(e) => Err(ToolError::upstream(format!(
            "{e}\n{}",
            serde_json::to_string(&out).unwrap_or_default()
        ))),
        None => Ok(out),
    }
}

// ─── run_agents_parallel ────────────────────────────────────────────────────

fn run_agents_parallel(agents_list: Vec<RunArgs>, wait_all: bool) -> ToolResult {
    if agents_list.is_empty() || agents_list.len() > 10 {
        return Err(invalid("entre 1 et 10 agents"));
    }
    let cancel: Option<CancelToken> = litemcp::cancel::token();
    let (tx, rx) = mpsc::channel::<(usize, Value, bool)>();
    let total = agents_list.len();
    let started = Instant::now();
    let mut immediate: Vec<(usize, Value)> = Vec::new();
    let mut launched = 0usize;

    for (idx, args) in agents_list.into_iter().enumerate() {
        let task_id = args.task_id.clone().unwrap_or_else(|| format!("task_{}", idx + 1));
        let fail = |msg: String| json!({ "taskId": task_id, "error": msg });
        let req = match args.request() {
            Ok(r) => r,
            Err(e) => {
                immediate.push((idx, fail(e.message)));
                continue;
            }
        };
        let live = match runners::prepare(&req) {
            Ok(l) => l,
            Err(r) => {
                immediate.push((idx, fail(r.error.unwrap_or_default())));
                continue;
            }
        };
        launched += 1;
        let tx = tx.clone();
        let cancel = cancel.clone();
        std::thread::Builder::new()
            .name(format!("overmind-par-{idx}"))
            .spawn(move || {
                let t0 = Instant::now();
                let r = runners::execute(&req, &live, cancel.as_ref());
                let mut v = run_result_json(&req, &r, t0.elapsed());
                v["taskId"] = json!(task_id);
                let ok = r.error.is_none();
                let _ = tx.send((idx, v, ok));
            })
            .map_err(|e| ToolError::internal(format!("thread impossible: {e}")))?;
    }
    drop(tx);

    let mut results: Vec<(usize, Value)> = immediate;
    let mut received = 0usize;
    for _ in 0..launched {
        match rx.recv() {
            Ok((idx, v, ok)) => {
                received += 1;
                results.push((idx, v));
                if ok && !wait_all {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    results.sort_by_key(|(i, _)| *i);
    let finished = results.len();
    let failed = results.iter().filter(|(_, v)| v.get("error").is_some()).count();
    Ok(json!({
        "total": total,
        "completed": finished,
        "failed": failed,
        "stillRunning": launched - received,
        "durationMs": started.elapsed().as_millis() as u64,
        "results": results.into_iter().map(|(_, v)| v).collect::<Vec<_>>(),
    }))
}

// ─── agent_control ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ControlArgs {
    action: String,
    agent_name: Option<String>,
    run_id: Option<String>,
    runner: Option<String>,
    config: Option<String>,
    timeout_ms: Option<u64>,
    since_timestamp: Option<u64>,
}

fn snapshot_json(s: &Snapshot) -> Value {
    let mut v = json!({
        "runId": s.run_id,
        "runner": s.runner,
        "agentName": s.agent,
        "pid": s.pid,
        "status": s.status.as_str(),
        "exitCode": s.exit_code,
        "sessionId": s.session_id,
        "startedAt": s.started_at,
        "lastOutputAt": s.last_output_at,
        "elapsedMs": now_ms().saturating_sub(s.started_at),
    });
    if let Some(o) = &s.outcome {
        v["result"] = json!(o.result);
        if let Some(e) = &o.error {
            v["error"] = json!(e);
        }
    }
    v
}

fn find_run(args: &ControlArgs) -> Result<Option<std::sync::Arc<registry::LiveRun>>, ToolError> {
    if let Some(id) = &args.run_id {
        return Ok(registry::find_by_run_id(id));
    }
    let name = args.agent_name.as_deref().ok_or_else(|| invalid("agentName ou runId requis"))?;
    agents::validate_name(name).map_err(invalid)?;
    let runner = args.runner.as_deref();
    if let Some(r) = runner {
        parse_runner(r)?;
    }
    Ok(registry::find(name, runner))
}

fn agent_control(args: ControlArgs) -> ToolResult {
    if args.action == "list" {
        let runs: Vec<Value> = registry::all().iter().map(snapshot_json).collect();
        return Ok(json!({ "count": runs.len(), "runs": runs }));
    }
    let workspace = config::workspace_for(args.config.as_deref());
    let Some(run) = find_run(&args)? else {
        return control_from_disk(&args, &workspace);
    };
    match args.action.as_str() {
        "status" => Ok(snapshot_json(&run.snapshot())),
        "stream" => {
            let snap = run.snapshot();
            Ok(json!({
                "runId": snap.run_id,
                "status": snap.status.as_str(),
                "isComplete": snap.status != Status::Running,
                "output": run.output_since(args.since_timestamp),
                "timestamp": now_ms(),
                "sessionId": snap.session_id,
            }))
        }
        "kill" => {
            if run.snapshot().status != Status::Running {
                return Ok(json!({ "killed": false, "reason": "le run n'est pas en cours", "run": snapshot_json(&run.snapshot()) }));
            }
            run.request_kill();
            let deadline = Instant::now() + Duration::from_secs(10);
            while run.snapshot().status == Status::Running && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(100));
            }
            let snap = run.snapshot();
            Ok(json!({ "killed": snap.status != Status::Running, "run": snapshot_json(&snap) }))
        }
        "wait" => {
            let timeout = Duration::from_millis(args.timeout_ms.unwrap_or(900_000).clamp(1_000, 3_600_000));
            let deadline = Instant::now() + timeout;
            let cancel = litemcp::cancel::token();
            while run.snapshot().status == Status::Running {
                if Instant::now() >= deadline {
                    return Err(ToolError::timeout(format!(
                        "délai {}ms atteint : l'agent tourne toujours",
                        timeout.as_millis()
                    )));
                }
                if cancel.as_ref().is_some_and(CancelToken::is_cancelled) {
                    return Err(ToolError::timeout("annulé par le client"));
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            Ok(snapshot_json(&run.snapshot()))
        }
        other => Err(invalid(format!("action inconnue '{other}' (status | stream | kill | wait | list)"))),
    }
}

/// Run absent de la mémoire : le serveur a peut-être redémarré depuis son
/// lancement — on s'appuie alors sur le registre disque.
fn control_from_disk(args: &ControlArgs, workspace: &std::path::Path) -> ToolResult {
    let Some(name) = args.agent_name.as_deref() else {
        return Err(invalid(format!("run introuvable: {}", args.run_id.as_deref().unwrap_or("?"))));
    };
    let Some((key, entry)) = registry::disk_entry(workspace, name, args.runner.as_deref()) else {
        return Err(invalid(format!("aucun run connu pour l'agent '{name}'")));
    };
    let pid = entry.get("pid").and_then(Value::as_u64).map(|p| p as u32);
    let alive = pid.is_some_and(crate::process::pid_alive);
    let recorded = entry.get("status").and_then(Value::as_str).unwrap_or("unknown");
    let status = if recorded == "running" && !alive { "orphaned" } else { recorded };
    match args.action.as_str() {
        "status" | "stream" | "wait" => Ok(json!({
            "source": "disk",
            "agentName": name,
            "pid": pid,
            "status": status,
            "alive": alive,
            "sessionId": entry.get("id"),
            "note": "run lancé avant le démarrage de ce serveur : pas de sortie en direct disponible",
        })),
        "kill" => {
            if let (Some(pid), true) = (pid, alive && recorded == "running") {
                crate::process::kill_tree(pid);
                registry::mark_disk_killed(workspace, &key);
                Ok(json!({ "killed": true, "pid": pid, "source": "disk" }))
            } else {
                Ok(json!({ "killed": false, "reason": "aucun process vivant", "status": status }))
            }
        }
        other => Err(invalid(format!("action inconnue '{other}'"))),
    }
}

// ─── Gestion des agents ─────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateToolArgs {
    name: String,
    prompt: String,
    runner: Option<String>,
    model: Option<String>,
    copy_env_from: Option<String>,
    clone_from: Option<String>,
    config: Option<String>,
}

fn create_agent(a: CreateToolArgs) -> ToolResult {
    let ws = config::workspace_for(a.config.as_deref());
    let runner = a.runner.as_deref().map(parse_runner).transpose()?.unwrap_or(Runner::Claude);
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
            prompt: &a.prompt,
            runner,
            model: a.model.as_deref(),
            copy_env_from: a.copy_env_from.as_deref(),
            clone_from: a.clone_from.as_deref(),
        },
    )
    .map_err(ToolError::invalid_input)?;
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
    Ok(out)
}

#[derive(Deserialize)]
struct ListArgs {
    #[serde(default)]
    details: bool,
    config: Option<String>,
}

fn list_agents(a: ListArgs) -> ToolResult {
    let ws = config::workspace_for(a.config.as_deref());
    let list = agents::list(&ws);
    let items: Vec<Value> = list
        .iter()
        .map(|i| {
            let session = sessions::get(&ws, i.runner.as_str(), &i.name);
            let running = registry::find(&i.name, Some(i.runner.as_str()))
                .is_some_and(|r| r.snapshot().status == Status::Running);
            let mut v = json!({
                "name": i.name,
                "runner": i.runner.as_str(),
                "hasSession": session.is_some(),
                "running": running,
            });
            if a.details {
                v["model"] = json!(i.model);
                v["mcpServers"] = json!(i.mcp_servers);
                v["promptBytes"] = json!(i.prompt_bytes);
                v["sessionId"] = json!(session);
                if i.missing_config {
                    v["warning"] = json!("configuration incomplète");
                }
            }
            v
        })
        .collect();
    Ok(json!({ "count": items.len(), "agents": items }))
}

#[derive(Deserialize)]
struct NameArgs {
    name: String,
    runner: Option<String>,
    config: Option<String>,
}

fn get_agent_configs(a: NameArgs) -> ToolResult {
    let ws = config::workspace_for(a.config.as_deref());
    let files = agents::get_configs(&ws, &a.name, opt_runner(&a.runner)?).map_err(ToolError::invalid_input)?;
    let map: Map<String, Value> = files.into_iter().map(|(k, v)| (k, json!(v))).collect();
    Ok(json!({ "name": a.name, "files": map }))
}

fn delete_agent(a: NameArgs) -> ToolResult {
    let ws = config::workspace_for(a.config.as_deref());
    let d = agents::delete(&ws, &a.name, opt_runner(&a.runner)?).map_err(ToolError::invalid_input)?;
    let mut v = json!({ "deleted": a.name, "files": d.files });
    if !d.errors.is_empty() {
        v["errors"] = json!(d.errors);
    }
    Ok(v)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateArgs {
    name: String,
    runner: Option<String>,
    model: Option<String>,
    mcp_servers: Option<Vec<String>>,
    env: Option<Map<String, Value>>,
    prompt: Option<String>,
    file: Option<String>,
    content: Option<String>,
    config: Option<String>,
}

fn update_agent_config(a: UpdateArgs) -> ToolResult {
    let ws = config::workspace_for(a.config.as_deref());
    let file = match (a.file, a.content) {
        (Some(f), Some(c)) => Some((f, c)),
        (None, None) => None,
        _ => return Err(invalid("file et content vont ensemble")),
    };
    let up = Updates { model: a.model, mcp_servers: a.mcp_servers, env: a.env, prompt: a.prompt, file };
    let changes = agents::update(&ws, &a.name, opt_runner(&a.runner)?, &up).map_err(ToolError::invalid_input)?;
    Ok(json!({ "agent": a.name, "changes": changes }))
}

// ─── Enregistrement ─────────────────────────────────────────────────────────

fn str_array(desc: &str) -> impl litemcp::zod::Schema + 'static {
    zod::describe(desc, zod::array(zod::string().min(1)))
}

pub fn register(builder: ServerBuilder) -> ServerBuilder {
    let runner_opt = || zod::describe("Runner (si l'agent existe côté claude ET hermes)", zod::enumeration(RUNNERS));
    let config_opt = || zod::describe("Racine OverMind (défaut: workspace global)", zod::string().min(1));

    builder
        .tool_parse(
            "run_agent",
            "Lance un agent persistant (Claude Code ou Hermes). Avec agentName, la session précédente est reprise \
             automatiquement (autoResume) et la nouvelle est sauvegardée : l'agent garde sa mémoire de conversation \
             d'un appel à l'autre. background:true rend la main tout de suite avec un runId à suivre via agent_control.",
            run_object().opt("background", zod::describe("Ne pas attendre : renvoie un runId", zod::boolean())).strict(),
            run_agent,
        )
        .tool_parse(
            "run_agents_parallel",
            "Lance jusqu'à 10 agents en parallèle (runners mélangeables). waitAll:false rend la main dès le premier succès ; \
             les autres continuent et restent suivis par agent_control.",
            zod::object()
                .field(
                    "agents",
                    zod::array(run_object().opt("taskId", zod::string().min(1)).strict()).min(1).max(10),
                )
                .opt("waitAll", zod::describe("Attendre tous les agents (défaut: true)", zod::boolean()))
                .strict(),
            |a: ParallelArgs| run_agents_parallel(a.agents, a.wait_all.unwrap_or(true)),
        )
        .tool_parse(
            "agent_control",
            "Contrôle les runs : status (état + PID + session), stream (sortie en direct, since=timestamp), \
             wait (bloque jusqu'à la fin), kill (arrêt de l'arbre de processus), list (tous les runs).",
            zod::object()
                .field("action", zod::enumeration(&["status", "stream", "wait", "kill", "list"]))
                .opt("agentName", name_schema("Agent ciblé"))
                .opt("runId", zod::describe("Run ciblé (rendu par run_agent background:true)", zod::string().min(1)))
                .opt("runner", runner_opt())
                .opt("timeoutMs", zod::describe("Délai de wait en ms (défaut 900000, max 3600000)", zod::uint().min(1000).max(3_600_000)))
                .opt("sinceTimestamp", zod::describe("stream : sortie après ce timestamp (ms)", zod::uint()))
                .opt("config", config_opt())
                .strict(),
            agent_control,
        )
        .tool_parse(
            "create_agent",
            "Crée un agent persistant. Claude : .claude/agents/<nom>.md + settings_<nom>.json + .mcp.<nom>.json. \
             Hermes : profil natif (hermes profile create) + SOUL.md. Le prompt définit sa persona.",
            zod::object()
                .field("name", name_schema("Nom unique [a-zA-Z0-9_-]"))
                .field("prompt", zod::describe("Prompt système (persona, missions, outils autorisés)", zod::string().min(1)))
                .opt("runner", runner_opt())
                .opt("model", zod::describe("Modèle ou surnom (voir model-aliases.json)", zod::string().min(1)))
                .opt("copyEnvFrom", zod::describe("Claude : settings.json existant dont copier env + serveurs MCP", zod::string().min(1)))
                .opt("cloneFrom", zod::describe("Hermes : profil existant dont copier config, .env, SOUL, skills (les credentials sont par profil)", zod::string().min(1)))
                .opt("config", config_opt())
                .strict(),
            create_agent,
        )
        .tool_parse(
            "list_agents",
            "Liste les agents (Claude et Hermes) avec leur état de session. details:true ajoute modèle, MCP, taille du prompt.",
            zod::object().opt("details", zod::boolean()).opt("config", config_opt()).strict(),
            list_agents,
        )
        .tool_parse(
            "get_agent_configs",
            "Lit les fichiers de configuration d'un agent (prompt, settings, MCP, env — secrets masqués).",
            zod::object()
                .field("name", name_schema("Agent"))
                .opt("runner", runner_opt())
                .opt("config", config_opt())
                .strict(),
            get_agent_configs,
        )
        .tool_parse(
            "update_agent_config",
            "Modifie un agent : model, mcpServers, env (clé→valeur), prompt — ou réécrit un fichier entier \
             (file: prompt.md | settings.json | .mcp.json | skill.md, avec content).",
            zod::object()
                .field("name", name_schema("Agent"))
                .opt("runner", runner_opt())
                .opt("model", zod::string().min(1))
                .opt("mcpServers", str_array("Serveurs MCP (noms de .mcp.json)"))
                .opt("env", zod::describe("Variables d'environnement à écrire", zod::record(zod::string())))
                .opt("prompt", zod::describe("Nouveau prompt système", zod::string().min(1)))
                .opt("file", zod::enumeration(&["prompt.md", "settings.json", ".mcp.json", "skill.md"]))
                .opt("content", zod::describe("Contenu complet du fichier (avec file)", zod::string()))
                .opt("config", config_opt())
                .strict(),
            update_agent_config,
        )
        .tool_parse(
            "delete_agent",
            "Supprime un agent (fichiers, profil Hermes) et sa session sauvegardée. Refusé s'il a un run en cours.",
            zod::object()
                .field("name", name_schema("Agent"))
                .opt("runner", runner_opt())
                .opt("config", config_opt())
                .strict(),
            delete_agent,
        )
        .annotate("list_agents", ToolAnnotations::new().title("Lister les agents").read_only(true))
        .annotate("get_agent_configs", ToolAnnotations::new().title("Lire la config d'un agent").read_only(true))
        .annotate("delete_agent", ToolAnnotations::new().title("Supprimer un agent").destructive(true))
        .annotate("agent_control", ToolAnnotations::new().title("Contrôler les runs"))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ParallelArgs {
    agents: Vec<RunArgs>,
    wait_all: Option<bool>,
}
