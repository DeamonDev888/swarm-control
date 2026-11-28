//! Mode agent : ce process ne sert qu'un agent résident.
//!
//! Pas de serveur d'administration : seulement le serveur MCP de l'agent (`swarm-<agent>`,
//! `ask` / `status` / `stop`) et, avec `--bridge N`, le pont HTTP parlant le dialecte Discord
//! (`/send`, `/reset`, `/status`, `/health`).
//!
//! Une fois l'agent en place :
//! - **terminal** et sans `--headless` → la TUI (`crate::tui::run`) ; à sa fermeture l'agent est
//!   arrêté proprement (`residents::stop`) et le process sort ;
//! - sinon (`--headless`, stdout redirigé, systemd) → bannière puis attente, sans interface.
//!
//! Une TUI qui échoue ne tue pas l'agent : message, puis comportement headless.

use super::args::Options;
use crate::residents::{Ctx, Schedule};
use crate::{agents, config, instance, registry, residents};
use std::io::IsTerminal;
use std::process::ExitCode;
use std::sync::Arc;

pub fn start(options: Options) -> ExitCode {
    match boot(options) {
        Ok((ctx, headless, debug)) => after_boot(ctx, headless, debug),
        Err(e) => {
            eprintln!("swarm-control: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Démarre la résidence et annonce l'URL MCP (`… MCP sur http://127.0.0.1:PORT/mcp …`).
fn boot(options: Options) -> Result<(Arc<Ctx>, bool, bool), String> {
    let agent = options.agent.clone().unwrap_or_default();
    agents::validate_name(&agent).map_err(|e| format!("agent: {e}"))?;
    if !options.residents.is_empty() {
        return Err("--resident ne se combine pas avec le mode agent (un agent par process)".into());
    }
    // La tâche planifiée (`--every`) est validée avant le bail d'instance : une durée ou une
    // fenêtre mal formée ne démarre rien.
    let schedule = schedule_of(&options)?;
    instance::claim(options.instance)?;
    let cfg = config::init(options.workspace);
    registry::start_sweeper(cfg.workspace.clone());

    let runner =
        agents::resolve(&cfg.workspace, &agent, None).map_err(|e| format!("agent {agent}: {e}"))?;
    let listen = options.port.map(|port| format!("{}:{port}", options.host));
    let info =
        residents::start_on(&cfg.workspace, runner, &agent, listen.as_deref(), schedule.as_ref())
            .map_err(|e| format!("agent {agent}: {e}"))?;
    let bridge = match options.bridge {
        Some(port) => {
            let addr = residents::open_bridge(runner, &agent, &format!("{}:{port}", options.host))
                .map_err(|e| format!("--bridge {port}: {e}"))?;
            format!(" · pont HTTP sur http://{addr}")
        }
        None => String::new(),
    };
    // Une ligne contenant l'URL MCP, comme la bannière d'une instance (lue par les tests).
    eprintln!(
        "{} {} — MCP sur {}  (agent: {agent}, runner: {}, workspace: {}){bridge}",
        info.name,
        env!("CARGO_PKG_VERSION"),
        info.url,
        info.runner,
        cfg.workspace.display()
    );
    let ctx = residents::ctx_of(&agent, runner)
        .ok_or_else(|| format!("agent {agent}: contexte résident introuvable après démarrage"))?;
    Ok((ctx, options.headless, options.debug))
}

/// La tâche planifiée de la ligne de commande (PLAN §4.4) : `--every`, le prompt positionnel de
/// `serve`, `--window`. Absente sans `--every` (le `schedule` de la définition fait alors foi) ;
/// mal formée, l'erreur nomme l'option en cause.
fn schedule_of(options: &Options) -> Result<Option<Schedule>, String> {
    let Some(every) = options.every.as_deref() else {
        return Ok(None);
    };
    let prompt = options.prompt.clone().unwrap_or_default();
    match Schedule::parse(every, prompt, options.window.as_deref()) {
        Ok(schedule) => Ok(Some(schedule)),
        // Une fenêtre mal formée vient de `--window` ; le reste, de `--every` (ou du prompt).
        Err(e) if e.starts_with("fenêtre") => {
            Err(format!("--window {} : {e}", options.window.as_deref().unwrap_or("")))
        }
        Err(e) => Err(format!("--every {every} : {e}")),
    }
}

fn after_boot(ctx: Arc<Ctx>, headless: bool, debug: bool) -> ExitCode {
    if !headless && std::io::stdout().is_terminal() {
        match open_tui(Arc::clone(&ctx), debug) {
            Ok(()) => {
                stop(&ctx);
                return ExitCode::SUCCESS;
            }
            Err(e) => eprintln!("swarm-control: TUI indisponible ({e}) : mode headless"),
        }
    }
    // Les serveurs tournent dans leurs propres fils : celui-ci n'a plus qu'à durer.
    loop {
        std::thread::park();
    }
}

/// Arrêt propre : serveur MCP rendu, file vidée, bail d'hébergement libéré.
fn stop(ctx: &Ctx) {
    match residents::stop(&ctx.workspace, &ctx.agent, Some(ctx.runner)) {
        Ok(_) => eprintln!("agent {} arrêté.", ctx.agent),
        Err(e) => eprintln!("swarm-control: arrêt de l'agent: {e}"),
    }
}

#[cfg(feature = "tui")]
fn open_tui(ctx: Arc<Ctx>, debug: bool) -> Result<(), String> {
    // `Quit` comme `Stop` ferment l'agent : la TUI est son interface au premier plan.
    crate::tui::run(ctx, debug).map(|_exit| ())
}

#[cfg(not(feature = "tui"))]
fn open_tui(_ctx: Arc<Ctx>, _debug: bool) -> Result<(), String> {
    Err("binaire compilé sans la feature `tui`".into())
}

#[cfg(test)]
mod tests {
    use super::{Options, boot, schedule_of};
    use crate::cli::exe::renamed;
    use std::ffi::OsStr;
    use std::time::Duration;
    fn une_option(agent: Option<&str>) -> Options {
        Options { agent: agent.map(String::from), ..Options::default() }
    }

    #[test]
    fn un_nom_resolu_vide_ou_invalide_est_refuse_avant_toute_installation() {
        // `boot` reçoit le nom déjà résolu (exécutable renommé ou --agent) : un nom absent
        // (aucune résolution) ou invalide doit être refusé AVANT le bail d'instance.
        let Err(error) = boot(une_option(None)) else {
            panic!("un agent sans nom doit être refusé");
        };
        assert!(error.starts_with("agent:"), "{error}");
        let Err(error) = boot(une_option(Some("../évasion"))) else {
            panic!("un nom invalide doit être refusé");
        };
        assert!(error.starts_with("agent:"), "{error}");
    }

    #[test]
    fn le_mode_agent_refuse_les_residents_combines() {
        // Un process = un agent : les erreurs de mode se voient avant tout effet de bord.
        let mut options = une_option(Some("discord"));
        options.residents = vec!["autre".into()];
        let Err(error) = boot(options) else {
            panic!("--resident doit être refusé en mode agent");
        };
        assert!(error.contains("--resident"), "{error}");
    }

    /// `--every` (ou `--window`) mal formé : refusé par `boot` avant le bail d'instance,
    /// l'erreur nomme l'option en cause ; bien formé, la tâche est celle de la ligne de
    /// commande (durée, prompt positionnel, fenêtre) ; sans `--every`, il n'y en a pas.
    #[test]
    fn un_every_ou_un_window_mal_forme_est_refuse_avant_toute_installation() {
        let mut options = une_option(Some("discord"));
        options.every = Some("5x".into());
        options.prompt = Some("résume".into());
        let Err(error) = boot(options) else {
            panic!("--every 5x doit être refusé");
        };
        assert!(error.starts_with("--every 5x : durée « 5x »"), "{error}");

        let mut options = une_option(Some("discord"));
        options.every = Some("5m".into());
        options.prompt = Some("résume".into());
        options.window = Some("25:00-18:00".into());
        let Err(error) = boot(options) else {
            panic!("--window 25:00-18:00 doit être refusé");
        };
        assert!(error.starts_with("--window 25:00-18:00 : fenêtre"), "{error}");

        let mut options = une_option(Some("discord"));
        options.every = Some("1h30".into());
        options.prompt = Some("résume".into());
        options.window = Some("09:00-18:00".into());
        let schedule = schedule_of(&options).unwrap().expect("--every : une tâche");
        assert_eq!(schedule.every, Duration::from_secs(5400));
        assert_eq!(schedule.prompt, "résume");
        assert_eq!(schedule.window.map(|w| (w.start, w.end)), Some(((9, 0), (18, 0))));
        // Sans `--window` : pas de fenêtre ; sans `--every` : pas de tâche (la définition
        // de l'agent fait foi), même avec un prompt résiduel.
        options.window = None;
        assert_eq!(schedule_of(&options).unwrap().unwrap().window, None);
        options.every = None;
        assert_eq!(schedule_of(&options), Ok(None));
    }

    #[test]
    fn le_nom_du_binaire_renomme_est_la_source_par_defaut_de_l_agent() {
        // Contrat amont de `boot` (résolu dans cli::run_with) : un exécutable renommé
        // (`discord.exe`) désigne l'agent ; `swarm-control` reste l'instance d'admin, et
        // `--agent`, déjà présent dans Options, garde la priorité sur ce défaut.
        assert_eq!(renamed(OsStr::new(r"C:\agents\discord.exe")).as_deref(), Some("discord"));
        assert_eq!(renamed(OsStr::new(r"C:\bin\swarm-control.exe")), None);
    }
}
