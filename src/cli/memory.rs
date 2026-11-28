//! `memory` : la mémoire native d'un agent, en ligne de commande (PLAN §4.6).
//!
//! `memory search "<requête>" [-n N] [--swarm]` · `memory add "<texte>" [--tags a,b]` ·
//! `memory forget <id>`. Même fichier que les outils MCP du résident
//! (`bridge/memory/<runner>-<agent>.jsonl`, voir [`crate::memory`]) : ce qui est ajouté ici
//! est visible par l'agent, et l'inverse. Un souvenir portant un jeton est refusé comme par
//! l'outil. `--json` rend le document brut ; sinon une ligne par souvenir, comme `history`.

use super::{output, target};
use crate::agents::Runner;
use crate::memory::{self, Hit, Scope};
use serde_json::{Value, json};
use std::path::Path;

/// Exécute `memory <action> [args…]` pour `agent`. Le runner est celui de `--runner`, sinon
/// celui de l'agent (hôte local, fichiers, ou mémoire existante).
pub fn run(
    agent: &str,
    runner: Option<Runner>,
    action: &str,
    rest: &[String],
    json: bool,
) -> Result<(), String> {
    // Le workspace est déjà résolu par la CLI (`config::init` avec `--workspace`) : on le
    // relit, on n'en devine pas un autre.
    let ws = crate::config::init(None).workspace.clone();
    execute(&ws, agent, runner, action, rest, json)
}

fn execute(ws: &Path, agent: &str, runner: Option<Runner>, action: &str, rest: &[String], json: bool) -> Result<(), String> {
    let out = perform(ws, agent, runner, action, rest)?;
    if json {
        output::json(&out.json);
    } else {
        print!("{}", out.text);
    }
    Ok(())
}

/// Ce qu'une action rend : le document `--json` et le texte lisible.
#[derive(Debug)]
struct Output {
    json: Value,
    text: String,
}

/// Valide la cible, résout le runner, applique l'action : le nom d'agent est vérifié avant
/// tout accès disque.
fn perform(ws: &Path, agent: &str, runner: Option<Runner>, action: &str, rest: &[String]) -> Result<Output, String> {
    target::validate(agent)?;
    let runner = resolve_runner(ws, agent, runner)?;
    match action {
        "search" => search(ws, agent, runner, rest),
        "add" => add(ws, agent, runner, rest),
        "forget" => forget(ws, agent, runner, rest),
        other => Err(format!("action memory inconnue « {other} » (search | add | forget)")),
    }
}

/// Runner de l'agent : hôte local, fichiers, ou runner dont la mémoire existe déjà (agent
/// supprimé mais souvenirs encore là) ; sinon `--runner` explicite.
fn resolve_runner(ws: &Path, agent: &str, runner: Option<Runner>) -> Result<Runner, String> {
    target::known_runner(ws, agent, runner)
        .or_else(|| {
            target::RUNNERS.into_iter().find(|r| memory::path(ws, r.as_str(), agent).is_file())
        })
        .or(runner)
        .ok_or_else(|| format!("agent {agent} introuvable : --runner claude|hermes|antigravity, ou crée l'agent d'abord"))
}

/// Options reconnues après l'action, où qu'elles soient : `-n`/`--limit N`, `--tags a,b`
/// (ou `--tags=a,b`), `--swarm` ; `--` termine les options. Tout le reste est positionnel.
#[derive(Default)]
struct Parsed {
    positionals: Vec<String>,
    limit: Option<usize>,
    tags: Vec<String>,
    swarm: bool,
}

fn parse(rest: &[String]) -> Result<Parsed, String> {
    let mut out = Parsed::default();
    let mut args = rest.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--" => out.positionals.extend(args.by_ref().cloned()),
            "-n" | "--limit" => {
                let raw = args.next().ok_or("--limit attend un nombre")?;
                out.limit = Some(parse_limit(raw)?);
            }
            "--swarm" => out.swarm = true,
            "--tags" => {
                let raw = args.next().ok_or("--tags attend une liste (a,b)")?;
                out.tags = split_tags(raw);
            }
            other => {
                if let Some(raw) = other.strip_prefix("--tags=") {
                    out.tags = split_tags(raw);
                } else if let Some(raw) = other.strip_prefix("--limit=") {
                    out.limit = Some(parse_limit(raw)?);
                } else if other.starts_with("--") {
                    return Err(format!(
                        "option inconnue « {other} » (memory : -n, --tags, --swarm, --json)"
                    ));
                } else {
                    out.positionals.push(other.to_string());
                }
            }
        }
    }
    Ok(out)
}

fn parse_limit(raw: &str) -> Result<usize, String> {
    let n: usize = raw.trim().parse().map_err(|_| format!("--limit attend un nombre (reçu « {raw} »)"))?;
    if (1..=memory::MAX_LIMIT).contains(&n) {
        Ok(n)
    } else {
        Err(format!("--limit entre 1 et {} (reçu {n})", memory::MAX_LIMIT))
    }
}

fn split_tags(raw: &str) -> Vec<String> {
    raw.split(',').map(str::trim).filter(|t| !t.is_empty()).map(String::from).collect()
}

fn search(ws: &Path, agent: &str, runner: Runner, rest: &[String]) -> Result<Output, String> {
    let parsed = parse(rest)?;
    let query = parsed.positionals.join(" ");
    if query.trim().is_empty() {
        return Err("memory search attend une requête : memory search \"<requête>\"".into());
    }
    let scope = if parsed.swarm { Scope::Swarm } else { Scope::Own };
    let limit = parsed.limit.unwrap_or(memory::DEFAULT_LIMIT);
    let hits = memory::search(ws, runner.as_str(), agent, &query, limit, scope)?;
    let mut text = String::new();
    for hit in &hits {
        text.push_str(&line(hit, scope));
    }
    if hits.is_empty() {
        text.push_str(&format!("aucun souvenir pour {agent} ne correspond à « {} ».\n", output::clip(&query, 80)));
    } else {
        text.push_str(&format!(
            "\n{} souvenir(s) · {}\n",
            hits.len(),
            if parsed.swarm { format!("essaim du workspace, sans {agent} (lecture seule)") } else { agent.to_string() }
        ));
    }
    let json = json!({
        "agent": agent,
        "runner": runner.as_str(),
        "scope": scope.as_str(),
        "results": hits.iter().map(Hit::to_json).collect::<Vec<_>>(),
    });
    Ok(Output { json, text })
}

/// Une ligne par souvenir : date, score, id, agent (propriétaire, en essaim), texte, tags.
fn line(hit: &Hit, scope: Scope) -> String {
    let tags = hit.memory.tags.iter().map(|t| format!("#{t}")).collect::<Vec<_>>().join(" ");
    let who = match scope {
        Scope::Own => String::new(),
        Scope::Swarm => format!("{:<12}  ", output::clip(&hit.agent, 12)),
    };
    format!(
        "{}  {:>4.2}  {:<18}  {}{}{}\n",
        output::stamp(hit.memory.ts),
        hit.score,
        output::clip(&hit.memory.id, 18),
        who,
        output::clip(&hit.memory.text, 120),
        if tags.is_empty() { String::new() } else { format!("  {tags}") }
    )
}

fn add(ws: &Path, agent: &str, runner: Runner, rest: &[String]) -> Result<Output, String> {
    let parsed = parse(rest)?;
    let text = parsed.positionals.join(" ");
    if text.trim().is_empty() {
        return Err("memory add attend un texte : memory add \"<texte>\" [--tags a,b]".into());
    }
    if parsed.limit.is_some() || parsed.swarm {
        return Err("memory add n'accepte pas -n ni --swarm (un souvenir est toujours le sien)".into());
    }
    let stored = memory::store(ws, runner.as_str(), agent, &text, &parsed.tags, "cli")?;
    let json = json!({
        "agent": agent,
        "runner": runner.as_str(),
        "id": stored.id,
        "text": stored.text,
        "tags": stored.tags,
    });
    let text = format!("mémorisé {} · {}\n", stored.id, output::clip(&stored.text, 100));
    Ok(Output { json, text })
}

fn forget(ws: &Path, agent: &str, runner: Runner, rest: &[String]) -> Result<Output, String> {
    let parsed = parse(rest)?;
    let [id] = parsed.positionals.as_slice() else {
        return Err("memory forget attend un id : memory forget <id>".into());
    };
    let ok = memory::forget(ws, runner.as_str(), agent, id)?;
    let json = json!({ "agent": agent, "runner": runner.as_str(), "id": id, "ok": ok });
    let text = if ok {
        format!("oublié {id} ({agent}).\n")
    } else {
        format!("aucun souvenir {id} chez {agent}.\n")
    };
    Ok(Output { json, text })
}

#[cfg(test)]
mod tests {
    use super::{add, forget, parse, perform, resolve_runner, search};
    use crate::agents::Runner;
    use serde_json::json;
    use std::path::PathBuf;

    fn ws(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("swarm-cli-memory-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn add_search_forget_round_trip_in_text_and_json() {
        let ws = ws("aller-retour");
        let added = add(&ws, "alice", Runner::Claude, &args(&["Le port est 23090", "--tags", "réseau, Port"])).unwrap();
        let id = added.json["id"].as_str().unwrap().to_string();
        assert_eq!(added.json["agent"], json!("alice"));
        assert_eq!(added.json["runner"], json!("claude"));
        assert_eq!(added.json["tags"], json!(["réseau", "Port"]));
        assert!(added.text.starts_with(&format!("mémorisé {id} · Le port est 23090")), "{}", added.text);
        // Écrit par la CLI, lu par le module (source `cli`).
        let line = std::fs::read_to_string(ws.join("bridge/memory/claude-alice.jsonl")).unwrap();
        assert!(line.contains("\"source\":\"cli\""), "{line}");

        let found = search(&ws, "alice", Runner::Claude, &args(&["PORT"])).unwrap();
        assert_eq!(found.json["scope"], json!("self"));
        assert_eq!(found.json["results"][0]["id"], json!(id));
        assert_eq!(found.json["results"][0]["agent"], json!("alice"));
        assert!(found.text.contains("Le port est 23090  #réseau #Port"), "{}", found.text);
        assert!(found.text.ends_with("1 souvenir(s) · alice\n"), "{}", found.text);

        let gone = forget(&ws, "alice", Runner::Claude, &args(&[&id])).unwrap();
        assert_eq!(gone.json["ok"], json!(true));
        assert_eq!(gone.text, format!("oublié {id} (alice).\n"));
        let again = forget(&ws, "alice", Runner::Claude, &args(&[&id])).unwrap();
        assert_eq!(again.json["ok"], json!(false));
        let empty = search(&ws, "alice", Runner::Claude, &args(&["port"])).unwrap();
        assert_eq!(empty.json["results"], json!([]));
        assert!(empty.text.contains("aucun souvenir"), "{}", empty.text);
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn options_are_parsed_wherever_they_sit() {
        let parsed = parse(&args(&["un texte", "--tags=a, b ,", "--swarm", "-n", "3"])).unwrap();
        assert_eq!(parsed.positionals, ["un texte"]);
        assert_eq!(parsed.tags, ["a", "b"]);
        assert!(parsed.swarm);
        assert_eq!(parsed.limit, Some(3));
        assert_eq!(parse(&args(&["--", "--pas-une-option"])).unwrap().positionals, ["--pas-une-option"]);
        assert!(parse(&args(&["x", "--inconnu"])).is_err());
        assert!(parse(&args(&["-n", "abc"])).is_err());
        assert!(parse(&args(&["-n", "0"])).is_err());
        assert!(parse(&args(&["--limit"])).is_err());
        assert!(parse(&args(&["--tags"])).is_err());
    }

    #[test]
    fn bad_usage_is_an_explicit_error() {
        let ws = ws("usage");
        let err = perform(&ws, "alice", Some(Runner::Claude), "list", &[]).unwrap_err();
        assert!(err.contains("action memory inconnue"), "{err}");
        assert!(search(&ws, "alice", Runner::Claude, &[]).unwrap_err().contains("attend une requête"));
        assert!(add(&ws, "alice", Runner::Claude, &args(&["  "])).unwrap_err().contains("attend un texte"));
        assert!(add(&ws, "alice", Runner::Claude, &args(&["x", "--swarm"])).unwrap_err().contains("--swarm"));
        assert!(forget(&ws, "alice", Runner::Claude, &[]).unwrap_err().contains("attend un id"));
        assert!(forget(&ws, "alice", Runner::Claude, &args(&["a", "b"])).unwrap_err().contains("attend un id"));
        assert!(search(&ws, "../évasion", Runner::Claude, &args(&["x"])).is_err(), "nom validé");
        assert!(!ws.exists(), "rien n'a été créé");
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn a_secret_is_refused_by_the_cli_as_by_the_tool() {
        let ws = ws("jeton");
        let err = add(&ws, "alice", Runner::Claude, &args(&["API_KEY=abc123def456ghi789"])).unwrap_err();
        assert!(err.starts_with("SECRET:"), "{err}");
        assert!(!ws.exists());
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn swarm_search_lists_the_neighbours_with_their_agent() {
        let ws = ws("essaim");
        crate::memory::store(&ws, "hermes", "bob", "bob note que le port 23091 répond", &[], "cli").unwrap();
        add(&ws, "alice", Runner::Claude, &args(&["le port 23090"])).unwrap();
        let own = search(&ws, "alice", Runner::Claude, &args(&["port"])).unwrap();
        assert_eq!(own.json["results"].as_array().unwrap().len(), 1);
        let swarm = search(&ws, "alice", Runner::Claude, &args(&["port", "--swarm", "-n", "5"])).unwrap();
        assert_eq!(swarm.json["scope"], json!("swarm"));
        assert_eq!(swarm.json["results"][0]["agent"], json!("bob"));
        assert!(swarm.text.contains("bob"), "{}", swarm.text);
        assert!(swarm.text.contains("lecture seule"), "{}", swarm.text);
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn the_runner_falls_back_to_an_existing_memory_file() {
        let ws = ws("runner");
        // Aucun agent déclaré : seul --runner convainc.
        assert_eq!(resolve_runner(&ws, "alice", Some(Runner::Hermes)), Ok(Runner::Hermes));
        assert!(resolve_runner(&ws, "alice", None).unwrap_err().contains("introuvable"));
        // Agent supprimé mais mémoire encore là : on la trouve.
        crate::memory::store(&ws, "antigravity", "alice", "souvenir", &[], "cli").unwrap();
        assert_eq!(resolve_runner(&ws, "alice", None), Ok(Runner::Antigravity));
        assert_eq!(resolve_runner(&ws, "alice", Some(Runner::Claude)), Ok(Runner::Claude), "--runner garde la priorité");
        let _ = std::fs::remove_dir_all(&ws);
    }
}
