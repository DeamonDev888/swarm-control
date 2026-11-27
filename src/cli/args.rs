//! Analyse des arguments de la ligne de commande.
//!
//! Une commande (facultative : sans commande, c'est `serve`), ses arguments positionnels, et des
//! options qui peuvent précéder ou suivre la commande :
//!
//! ```text
//! swarm-control [--workspace DIR] agents
//! sniper ask "état du marché ?" --model sonnet --timeout-ms 600000
//! swarm-control run sniper "résume la journée"
//! ```
//!
//! Les options propres au serveur restent celles d'avant (`--port`, `--host`, `--resident`…),
//! complétées par `--headless`, `--debug`, `--json` et `--runner`.

use crate::agents::Runner;
use std::path::PathBuf;

/// Attente par défaut d'un tour (`ask` / `run`) : 1 h.
pub const DEFAULT_TIMEOUT_MS: u64 = 3_600_000;

/// Attente maximale acceptée par l'outil `ask` d'un agent (`timeoutMs`).
pub const ASK_MAX_TIMEOUT_MS: u64 = 3_600_000;

pub struct Options {
    /// Port d'écoute ; `None` = premier port libre de 23080..=23099 (et, pour `schema`, une
    /// instance vivante).
    pub port: Option<u16>,
    pub host: String,
    pub workspace: Option<PathBuf>,
    /// Agents à rendre résidents au démarrage (`--resident alice,bob`, répétable).
    pub residents: Vec<String>,
    /// Numéro d'instance (`--instance N`) ; `None` = premier libre.
    pub instance: Option<u32>,
    /// Agent visé : mode agent (`--agent NAME`, ou exécutable nommé d'après l'agent) pour
    /// `serve` ; cible des autres commandes.
    pub agent: Option<String>,
    /// Port du pont HTTP compatible `overmind-bridge` (`--bridge N`, mode agent seulement).
    pub bridge: Option<u16>,
    /// Mode agent : pas de TUI, bannière puis attente.
    pub headless: bool,
    /// Journal de diagnostic sur stderr (`SWARM_DEBUG=1` fait de même).
    pub debug: bool,
    /// Sortie JSON brute au lieu du résumé lisible.
    pub json: bool,
    /// Runner visé quand l'agent existe sous plusieurs (`--runner`).
    pub runner: Option<Runner>,
    /// Modèle imposé au tour (`ask` / `run`).
    pub model: Option<String>,
    /// Attente maximale d'un tour, en ms.
    pub timeout_ms: u64,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            port: None,
            host: "127.0.0.1".into(),
            workspace: None,
            residents: Vec::new(),
            instance: None,
            agent: None,
            bridge: None,
            headless: false,
            debug: false,
            json: false,
            runner: None,
            model: None,
            timeout_ms: DEFAULT_TIMEOUT_MS,
        }
    }
}

/// Commande demandée (`serve` excepté : il a sa variante dans [`Parsed`]).
pub enum Command {
    /// `ask "<prompt>"` : un tour pour l'agent, son hôte ou un pair s'il tourne, sinon local.
    Ask { prompt: String },
    /// `status` : état de l'agent (hôte, pair, ou disque).
    Status,
    /// `history` : derniers tours du journal.
    History { limit: usize, before: Option<u64> },
    /// `reset` : oublie la session de l'agent.
    Reset,
    /// `config` : fichiers de configuration de l'agent (secrets masqués).
    Config { file: Option<String> },
    /// `schema <outil>` : schéma d'entrée d'un outil d'un serveur qui tourne.
    Schema { tool: String },
    /// `agents` : agents du workspace et leur résidence.
    Agents,
    /// `run <agent> "<prompt>"` : un tour pour un agent nommé.
    Run { agent: String, prompt: String },
    /// `runs` : dernier run de chaque agent.
    Runs,
    /// `doctor` : diagnostics non destructifs.
    Doctor,
}

impl Command {
    /// Nom de la commande, pour les messages.
    pub fn name(&self) -> &'static str {
        match self {
            Command::Ask { .. } => "ask",
            Command::Status => "status",
            Command::History { .. } => "history",
            Command::Reset => "reset",
            Command::Config { .. } => "config",
            Command::Schema { .. } => "schema",
            Command::Agents => "agents",
            Command::Run { .. } => "run",
            Command::Runs => "runs",
            Command::Doctor => "doctor",
        }
    }

    /// Cette commande vise-t-elle un agent unique (dont le nom vient de l'exécutable ou de
    /// `--agent`) ?
    pub fn needs_agent(&self) -> bool {
        matches!(
            self,
            Command::Ask { .. }
                | Command::Status
                | Command::History { .. }
                | Command::Reset
                | Command::Config { .. }
        )
    }
}

pub enum Parsed {
    /// `serve` (ou aucune commande) : l'instance ou l'agent résident, comme avant.
    Serve(Options),
    Command(Options, Command),
    Help,
    Version,
}

/// Choix de commande en cours de lecture, avant ses arguments.
enum Kind {
    Serve,
    Ask,
    Status,
    History,
    Reset,
    Config,
    Schema,
    Agents,
    Run,
    Runs,
    Doctor,
}

impl Kind {
    fn of(word: &str) -> Result<Kind, String> {
        Ok(match word {
            "serve" => Kind::Serve,
            "ask" => Kind::Ask,
            "status" => Kind::Status,
            "history" => Kind::History,
            "reset" => Kind::Reset,
            "config" => Kind::Config,
            "schema" => Kind::Schema,
            "agents" => Kind::Agents,
            "run" => Kind::Run,
            "runs" => Kind::Runs,
            "doctor" => Kind::Doctor,
            other => return Err(format!("commande inconnue: {other} (voir --help)")),
        })
    }
}

/// Options d'une commande, lues au fil de la ligne.
#[derive(Default)]
struct CommandOptions {
    limit: Option<usize>,
    before: Option<u64>,
    file: Option<String>,
}

pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Parsed, String> {
    let mut options = Options::default();
    let mut command_options = CommandOptions::default();
    let mut kind: Option<Kind> = None;
    let mut positional: Vec<String> = Vec::new();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--" => {
                positional.extend(args.by_ref());
                break;
            }
            "--port" | "-p" => options.port = Some(parse_port(&value(&mut args, "--port")?)?),
            "--host" => options.host = value(&mut args, "--host")?,
            "--workspace" | "-w" => {
                options.workspace = Some(PathBuf::from(value(&mut args, "--workspace")?));
            }
            "--resident" | "-r" => {
                options.residents.extend(names(&value(&mut args, "--resident")?))
            }
            "--instance" | "-i" => {
                options.instance = Some(parse_instance(&value(&mut args, "--instance")?)?)
            }
            "--agent" | "-a" => options.agent = Some(value(&mut args, "--agent")?),
            "--bridge" | "-b" => options.bridge = Some(parse_port(&value(&mut args, "--bridge")?)?),
            "--headless" => options.headless = true,
            "--debug" => options.debug = true,
            "--json" => options.json = true,
            "--runner" | "-R" => {
                options.runner = Some(parse_runner(&value(&mut args, "--runner")?)?)
            }
            "--model" => options.model = Some(value(&mut args, "--model")?),
            "--timeout-ms" => {
                options.timeout_ms =
                    parse_count(&value(&mut args, "--timeout-ms")?, "--timeout-ms")?;
            }
            "-n" => {
                command_options.limit = Some(parse_count(&value(&mut args, "-n")?, "-n")? as usize)
            }
            "--before" => {
                command_options.before = Some(parse_before(&value(&mut args, "--before")?)?)
            }
            "--file" | "-f" => command_options.file = Some(value(&mut args, "--file")?),
            "--help" | "-h" => return Ok(Parsed::Help),
            "--version" | "-V" => return Ok(Parsed::Version),
            other if other.starts_with('-') && other.len() > 1 => {
                return Err(format!("option inconnue: {other} (voir --help)"));
            }
            other if kind.is_none() => kind = Some(Kind::of(other)?),
            other => positional.push(other.to_string()),
        }
    }
    match kind {
        None => {
            no_more(&positional, "serve")?;
            Ok(Parsed::Serve(options))
        }
        Some(Kind::Serve) => {
            no_more(&positional, "serve")?;
            Ok(Parsed::Serve(options))
        }
        Some(kind) => Ok(Parsed::Command(options, build(kind, &positional, command_options)?)),
    }
}

fn build(
    kind: Kind,
    positional: &[String],
    command_options: CommandOptions,
) -> Result<Command, String> {
    let CommandOptions { limit, before, file } = command_options;
    Ok(match kind {
        Kind::Serve => unreachable!("serve est traité avant build"),
        Kind::Ask => Command::Ask { prompt: one(positional, "ask", "un prompt : ask \"…\"")? },
        Kind::Status => {
            no_more(positional, "status")?;
            Command::Status
        }
        Kind::History => {
            no_more(positional, "history")?;
            Command::History { limit: limit.unwrap_or(20).max(1), before }
        }
        Kind::Reset => {
            no_more(positional, "reset")?;
            Command::Reset
        }
        Kind::Config => {
            no_more(positional, "config")?;
            Command::Config { file }
        }
        Kind::Schema => {
            Command::Schema { tool: one(positional, "schema", "un nom d'outil : schema ask")? }
        }
        Kind::Agents => {
            no_more(positional, "agents")?;
            Command::Agents
        }
        Kind::Run => match positional {
            [agent, prompt] => Command::Run { agent: agent.clone(), prompt: prompt.clone() },
            [] | [_] => return Err("run attend un agent et un prompt : run AGENT \"…\"".into()),
            [_, extra, ..] => return Err(format!("run: argument inattendu « {extra} »")),
        },
        Kind::Runs => {
            no_more(positional, "runs")?;
            Command::Runs
        }
        Kind::Doctor => {
            no_more(positional, "doctor")?;
            Command::Doctor
        }
    })
}

fn value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    args.next().ok_or_else(|| format!("{flag} attend une valeur"))
}

fn parse_port(raw: &str) -> Result<u16, String> {
    raw.parse().map_err(|_| format!("port: nombre 0-65535 attendu (reçu « {raw} »)"))
}

fn parse_instance(raw: &str) -> Result<u32, String> {
    raw.parse::<u32>()
        .ok()
        .filter(|n| *n > 0)
        .ok_or_else(|| "--instance: entier ≥ 1 attendu".into())
}

fn parse_runner(raw: &str) -> Result<Runner, String> {
    Runner::parse(raw)
        .ok_or_else(|| format!("runner: claude, hermes ou antigravity attendu (reçu « {raw} »)"))
}

fn parse_count(raw: &str, flag: &str) -> Result<u64, String> {
    raw.parse::<u64>()
        .ok()
        .filter(|n| *n >= 1)
        .ok_or_else(|| format!("{flag}: entier ≥ 1 attendu (reçu « {raw} »)"))
}

fn parse_before(raw: &str) -> Result<u64, String> {
    raw.parse::<u64>().map_err(|_| format!("--before: horodatage en ms attendu (reçu « {raw} »)"))
}

/// `alice,bob` → `["alice", "bob"]` (vides ignorés).
fn names(raw: &str) -> Vec<String> {
    raw.split(',').map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect()
}

fn no_more(positional: &[String], command: &str) -> Result<(), String> {
    match positional.first() {
        Some(extra) => Err(format!("{command}: argument inattendu « {extra} »")),
        None => Ok(()),
    }
}

fn one(positional: &[String], command: &str, what: &str) -> Result<String, String> {
    match positional {
        [only] => Ok(only.clone()),
        [] => Err(format!("{command} attend {what}")),
        [_, extra, ..] => Err(format!("{command}: argument inattendu « {extra} »")),
    }
}

#[cfg(test)]
mod tests {
    use super::{Command, DEFAULT_TIMEOUT_MS, Options, Parsed, parse};
    use crate::agents::Runner;

    fn run(args: &[&str]) -> Result<Parsed, String> {
        parse(args.iter().map(|s| s.to_string()))
    }

    fn serve(args: &[&str]) -> Options {
        match run(args) {
            Ok(Parsed::Serve(options)) => options,
            Ok(_) => panic!("serve attendu"),
            Err(e) => panic!("parse: {e}"),
        }
    }

    fn command(args: &[&str]) -> Command {
        match run(args) {
            Ok(Parsed::Command(_, command)) => command,
            Ok(_) => panic!("commande attendue"),
            Err(e) => panic!("parse: {e}"),
        }
    }

    #[test]
    fn defaults_to_loopback_without_port() {
        let o = serve(&[]);
        assert_eq!((o.host.as_str(), o.port), ("127.0.0.1", None));
        assert!(o.residents.is_empty());
        assert!(!o.headless);
        assert!(!o.debug);
        assert!(!o.json);
        assert_eq!(o.timeout_ms, DEFAULT_TIMEOUT_MS);
        // `serve` explicite : même chose.
        assert_eq!(serve(&["serve"]).host, "127.0.0.1");
    }

    #[test]
    fn reads_port_host_and_workspace() {
        let o = serve(&["-p", "0", "--host", "0.0.0.0", "-w", "ws"]);
        assert_eq!((o.port, o.host.as_str()), (Some(0), "0.0.0.0"));
        assert_eq!(o.workspace.unwrap().to_str(), Some("ws"));
    }

    #[test]
    fn resident_accepts_commas_and_repetition() {
        let o = serve(&["--resident", "alice, bob", "-r", "carol,,"]);
        assert_eq!(o.residents, ["alice", "bob", "carol"]);
    }

    #[test]
    fn reads_agent_mode_and_bridge() {
        let o = serve(&["--agent", "sniper", "-b", "3001"]);
        assert_eq!((o.agent.as_deref(), o.bridge), (Some("sniper"), Some(3001)));
        assert!(run(&["--bridge", "x"]).is_err());
        assert!(run(&["--agent"]).is_err());
    }

    #[test]
    fn reads_headless_and_debug() {
        let o = serve(&["--headless", "--debug"]);
        assert!(o.headless && o.debug);
        let o = serve(&["--debug", "--headless"]);
        assert!(o.headless && o.debug);
        // Une commande les porte aussi (le mode est global).
        match run(&["--debug", "doctor"]) {
            Ok(Parsed::Command(o, Command::Doctor)) => assert!(o.debug),
            _ => panic!("doctor attendu"),
        }
    }

    #[test]
    fn ask_carries_the_prompt_and_its_options() {
        match command(&["ask", "tu es là ?"]) {
            Command::Ask { prompt } => assert_eq!(prompt, "tu es là ?"),
            _ => panic!("ask attendu"),
        }
        match run(&["ask", "salut", "--runner", "hermes", "--model", "m", "--timeout-ms", "600000"])
        {
            Ok(Parsed::Command(o, Command::Ask { prompt })) => {
                assert_eq!(prompt, "salut");
                assert_eq!(
                    (o.runner, o.model.as_deref(), o.timeout_ms),
                    (Some(Runner::Hermes), Some("m"), 600_000)
                );
            }
            _ => panic!("ask attendu"),
        }
        // L'agent visé vient de l'exécutable (rempli plus tard) ; `--agent` le force.
        match run(&["ask", "salut", "--agent", "bob", "--json"]) {
            Ok(Parsed::Command(o, Command::Ask { .. })) => {
                assert_eq!(o.agent.as_deref(), Some("bob"));
                assert!(o.json);
            }
            _ => panic!("ask attendu"),
        }
        assert!(run(&["ask"]).is_err());
        assert!(run(&["ask", "a", "b"]).is_err());
    }

    #[test]
    fn status_history_reset_and_config() {
        assert!(matches!(command(&["status"]), Command::Status));
        match command(&["history", "-n", "5", "--before", "1700000000000", "--json"]) {
            Command::History { limit, before } => {
                assert_eq!((limit, before), (5, Some(1_700_000_000_000)));
            }
            _ => panic!("history attendu"),
        }
        assert!(matches!(command(&["history"]), Command::History { limit: 20, before: None }));
        assert!(matches!(command(&["reset"]), Command::Reset));
        match command(&["config", "--file", "settings"]) {
            Command::Config { file } => assert_eq!(file.as_deref(), Some("settings")),
            _ => panic!("config attendu"),
        }
        assert!(matches!(command(&["config"]), Command::Config { file: None }));
        assert!(run(&["history", "-n", "0"]).is_err());
        assert!(run(&["history", "--before", "hier"]).is_err());
        assert!(run(&["reset", "x"]).is_err());
    }

    #[test]
    fn run_takes_an_agent_and_a_prompt() {
        match command(&["run", "sniper", "résume"]) {
            Command::Run { agent, prompt } => {
                assert_eq!((agent.as_str(), prompt.as_str()), ("sniper", "résume"));
            }
            _ => panic!("run attendu"),
        }
        assert!(run(&["run"]).is_err());
        assert!(run(&["run", "sniper"]).is_err());
        assert!(run(&["run", "sniper", "a", "b"]).is_err());
    }

    #[test]
    fn instance_commands_and_schema() {
        assert!(matches!(command(&["agents"]), Command::Agents));
        assert!(matches!(command(&["runs"]), Command::Runs));
        assert!(matches!(command(&["doctor"]), Command::Doctor));
        match command(&["schema", "run_agent", "--port", "23080"]) {
            Command::Schema { tool } => assert_eq!(tool, "run_agent"),
            _ => panic!("schema attendu"),
        }
        assert!(run(&["schema"]).is_err());
        assert!(run(&["agents", "x"]).is_err());
        assert!(run(&["schema", "a", "--agent", "bob"]).is_ok());
    }

    #[test]
    fn options_may_precede_the_command() {
        match run(&["--workspace", "ws", "--json", "runs"]) {
            Ok(Parsed::Command(o, Command::Runs)) => {
                assert_eq!(o.workspace.unwrap().to_str(), Some("ws"));
                assert!(o.json);
            }
            _ => panic!("runs attendu"),
        }
    }

    #[test]
    fn rejects_bad_input() {
        assert!(run(&["--port"]).is_err());
        assert!(run(&["--port", "abc"]).is_err());
        assert!(run(&["--resident"]).is_err());
        assert!(run(&["--nope"]).is_err());
        assert!(run(&["--instance", "0"]).is_err());
        assert!(run(&["--instance", "x"]).is_err());
        assert_eq!(serve(&["-i", "3"]).instance, Some(3));
        assert!(run(&["--timeout-ms", "0"]).is_err());
        assert!(run(&["--timeout-ms", "abc"]).is_err());
        assert!(run(&["--runner", "gpt"]).is_err());
        assert!(run(&["--model"]).is_err());
        assert!(run(&["bidon"]).is_err());
        assert!(run(&["serve", "x"]).is_err());
        assert!(matches!(run(&["-V"]), Ok(Parsed::Version)));
        assert!(matches!(run(&["--help"]), Ok(Parsed::Help)));
    }

    #[test]
    fn double_dash_ends_the_options() {
        match run(&["ask", "--", "-un prompt qui commence par un tiret"]) {
            Ok(Parsed::Command(_, Command::Ask { prompt })) => {
                assert_eq!(prompt, "-un prompt qui commence par un tiret");
            }
            _ => panic!("ask attendu"),
        }
    }
}
