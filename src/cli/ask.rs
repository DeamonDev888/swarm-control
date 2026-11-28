//! `ask` et `run` : un tour pour un agent.
//!
//! Trois chemins, du plus proche au plus lointain :
//! 1. l'agent est **hébergé par ce process** → l'outil `ask` de son propre serveur MCP, qui met
//!    un tour dans sa file et attend (même file, même session) ;
//! 2. un **pair vivant** est annoncé (`swarm-<agent>`) → l'outil `ask` de son serveur MCP ;
//! 3. sinon → un tour **local** (`runners::prepare` + `runners::execute`), même bail et même
//!    session que l'hôte : l'agent se souvient. Ce seul chemin est journalisé **ici** (même
//!    Entry que le résident) : les deux autres le sont par le résident qui exécute le tour.
//!
//! Le tour hébergé passe par le serveur de l'agent (et non par `runners::execute`) parce que
//! `Turn` n'est constructible que dans `residents` : c'est la file de l'hôte qui garantit un
//! tour à la fois sur sa session.
//!
//! La réponse va sur stdout ; les erreurs sortent en code 1.

use super::args::{ASK_MAX_TIMEOUT_MS, Options};
use super::target;
use crate::history;
use crate::owners;
use crate::peers::{self, CallError};
use crate::residents::Ctx;
use crate::runners::{self, RunRequest, RunResult};
use serde_json::json;
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

/// Après une demande d'arrêt, le temps laissé au run pour tuer son processus.
const KILL_GRACE: Duration = Duration::from_secs(5);

pub fn run(options: &Options, ws: &Path, agent: &str, prompt: &str) -> Result<(), String> {
    target::validate(agent)?;
    if prompt.trim().is_empty() {
        return Err("prompt vide.".into());
    }
    if let Some(ctx) = target::hosted(agent, options.runner) {
        return hosted(&ctx, prompt, options);
    }
    if let Some(peer) = target::peer(agent) {
        return remote(&peer.url, agent, prompt, options);
    }
    local(options, ws, agent, prompt)
}

/// Tour confié à l'hôte local : son propre outil `ask`, qui le met dans la même file (un tour à
/// la fois) et attend la réponse.
fn hosted(ctx: &Arc<Ctx>, prompt: &str, options: &Options) -> Result<(), String> {
    match ctx.url() {
        Some(url) => remote(url, &ctx.agent, prompt, options),
        None => Err(format!("agent '{}' hébergé ici mais sans URL MCP.", ctx.agent)),
    }
}

/// Tour confié au serveur MCP d'un agent (même outil `ask` que les agents entre eux).
fn remote(url: &str, agent: &str, prompt: &str, options: &Options) -> Result<(), String> {
    let mut args = json!({
        "prompt": prompt,
        "from": owners::instance_name(),
        "timeoutMs": options.timeout_ms.clamp(1_000, ASK_MAX_TIMEOUT_MS),
    });
    if let Some(model) = &options.model {
        args["model"] = json!(model);
    }
    let wait = Duration::from_millis(options.timeout_ms).saturating_add(Duration::from_secs(60));
    match peers::call_tool(url, "ask", args, wait) {
        Ok(reply) => {
            println!("{}", reply["reply"].as_str().unwrap_or_default());
            Ok(())
        }
        Err(CallError::Tool(message) | CallError::Protocol(message)) => Err(message),
        Err(CallError::Unreachable(why)) => {
            Err(format!("agent '{agent}' annoncé mais injoignable ({why})."))
        }
    }
}

/// Tour local : le run prend le bail de l'agent et reprend sa session, comme l'hôte.
///
/// Le run tourne dans un fil pour que le délai demandé puisse l'interrompre (process tué).
/// Le tour est journalisé comme côté résident (mêmes champs, `queuedMs` = 0) ; une demande
/// `run_agent` sans agent nommé reste éphémère et n'emprunte jamais ce chemin.
fn local(options: &Options, ws: &Path, agent: &str, prompt: &str) -> Result<(), String> {
    let runner = target::runner(ws, agent, options.runner)?;
    let request = RunRequest {
        runner,
        prompt: prompt.to_string(),
        agent: Some(agent.to_string()),
        session_id: None,
        auto_resume: true,
        model: options.model.clone(),
        provider: None,
        cwd: None,
        workspace: ws.to_path_buf(),
    };
    let live = match runners::prepare(&request) {
        Ok(live) => live,
        Err(result) => {
            journal(ws, runner.as_str(), agent, prompt, &result, 0);
            return Err(result.error.unwrap_or_else(|| "run refusé.".into()));
        }
    };
    let (sender, receiver) = mpsc::channel::<RunResult>();
    let running = Arc::clone(&live);
    std::thread::Builder::new()
        .name("swarm-cli-run".into())
        .spawn(move || {
            let result = runners::execute(&request, &running, None);
            let _ = sender.send(result);
        })
        .map_err(|e| format!("thread de run impossible: {e}"))?;
    let started = Instant::now();
    match receiver.recv_timeout(Duration::from_millis(options.timeout_ms)) {
        Ok(result) => {
            let duration_ms = started.elapsed().as_millis() as u64;
            journal(ws, runner.as_str(), agent, prompt, &result, duration_ms);
            match result.error {
                Some(error) => Err(error),
                None => {
                    println!("{}", result.result);
                    Ok(())
                }
            }
        }
        Err(RecvTimeoutError::Disconnected) => {
            let error = "le run s'est interrompu sans rendre de résultat.";
            journal(
                ws,
                runner.as_str(),
                agent,
                prompt,
                &RunResult::err(error),
                started.elapsed().as_millis() as u64,
            );
            Err(error.into())
        }
        Err(RecvTimeoutError::Timeout) => {
            let duration_ms = started.elapsed().as_millis() as u64;
            live.request_kill();
            // Laisse au run le temps de tuer son process avant que ce process-ci ne sorte.
            let _ = receiver.recv_timeout(KILL_GRACE);
            let error =
                format!("pas de réponse après {} ms : le run est interrompu.", options.timeout_ms);
            journal(
                ws,
                runner.as_str(),
                agent,
                prompt,
                &RunResult::err(error.clone()),
                duration_ms,
            );
            Err(error)
        }
    }
}

/// Journalise un tour local comme le ferait le résident qui l'exécute : même Entry,
/// `from` = cette instance, `queuedMs` = 0 (pas de file ; la durée couvre le run).
fn journal(
    ws: &Path,
    runner: &str,
    agent: &str,
    prompt: &str,
    result: &RunResult,
    duration_ms: u64,
) {
    history::record(
        ws,
        runner,
        agent,
        history::Entry {
            from: Some(owners::instance_name()),
            prompt: prompt.to_string(),
            reply: result.result.clone(),
            error: result.error.clone(),
            session_id: result.session_id.clone(),
            duration_ms,
            ..Default::default()
        },
    );
}

#[cfg(test)]
mod tests {
    use super::{Options, hosted, local, run};
    use crate::agents::Runner;
    use crate::cli::args::ASK_MAX_TIMEOUT_MS;
    use crate::history;
    use crate::owners;
    use crate::residents::Ctx;
    use serde_json::{Value, json};
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread::JoinHandle;

    /// Réponse d'un serveur MCP d'agent qui a traité l'`ask`.
    const REPONSE_OK: &str =
        r#"{"jsonrpc":"2.0","id":1,"result":{"structuredContent":{"reply":"tout va bien"}}}"#;
    /// Réponse d'un serveur dont l'outil `ask` refuse (`isError`) : le message remonte tel quel.
    const REPONSE_REFUS: &str = concat!(
        r#"{"jsonrpc":"2.0","id":1,"result":{"isError":true,"#,
        r#""content":[{"text":"AGENT_BUSY: un tour est déjà en cours."}]}}"#
    );

    /// Workspace temporaire unique par test (pid + nom) : la résolution n'y trouve rien.
    fn ws(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("swarm-cli-ask-{}-{tag}", std::process::id()))
    }

    fn options(dir: &Path) -> Options {
        Options { workspace: Some(dir.to_path_buf()), timeout_ms: 1_000, ..Options::default() }
    }

    /// Nom d'agent unique à ce test : jamais hébergé ici ni annoncé par un vrai pair.
    fn agent(tag: &str) -> String {
        format!("ut-{tag}-{}", std::process::id())
    }

    /// Un port de loopback certainement fermé : lié sur `0` puis libéré aussitôt.
    fn port_ferme() -> u16 {
        TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
    }

    /// Faux serveur MCP sur `127.0.0.1:0` : chaque POST reçoit `reponse` (corps JSON) et la
    /// requête JSON-RPC reçue est gardée. Aucun process, aucun réseau hors loopback.
    struct FauxMcp {
        url: String,
        addr: SocketAddr,
        vus: Arc<Mutex<Vec<Value>>>,
        stop: Arc<AtomicBool>,
        fil: Option<JoinHandle<()>>,
    }

    impl FauxMcp {
        fn start(reponse: &str) -> FauxMcp {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = listener.local_addr().unwrap();
            let vus = Arc::new(Mutex::new(Vec::new()));
            let stop = Arc::new(AtomicBool::new(false));
            let (vus_fil, stop_fil, reponse) =
                (Arc::clone(&vus), Arc::clone(&stop), reponse.to_string());
            let fil = std::thread::Builder::new()
                .name("swarm-test-faux-mcp".into())
                .spawn(move || {
                    for flux in listener.incoming() {
                        if stop_fil.load(Ordering::SeqCst) {
                            break;
                        }
                        if let Ok(mut flux) = flux {
                            servir(&mut flux, &reponse, &vus_fil);
                        }
                    }
                })
                .unwrap();
            FauxMcp { url: format!("http://{addr}/mcp"), addr, vus, stop, fil: Some(fil) }
        }

        /// Les requêtes JSON-RPC reçues jusqu'ici.
        fn vues(&self) -> Vec<Value> {
            self.vus.lock().unwrap_or_else(|e| e.into_inner()).clone()
        }
    }

    impl Drop for FauxMcp {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            // Une connexion de courtoisie réveille `incoming` ; le fil voit `stop` et sort.
            let _ = TcpStream::connect(self.addr);
            if let Some(fil) = self.fil.take() {
                let _ = fil.join();
            }
        }
    }

    /// Une requête HTTP/1.1 (lut jusqu'à `Content-Length`), une réponse, connexion fermée :
    /// ce qu'attend `peers::http::post`.
    fn servir(flux: &mut TcpStream, reponse: &str, vus: &Mutex<Vec<Value>>) {
        let mut brut = Vec::new();
        let mut tampon = [0u8; 4096];
        let (mut corps, mut longueur) = (None, 0usize);
        while let Ok(n) = flux.read(&mut tampon) {
            if n == 0 {
                break;
            }
            brut.extend_from_slice(&tampon[..n]);
            if corps.is_none() {
                if let Some(at) = brut.windows(4).position(|w| w == b"\r\n\r\n") {
                    corps = Some(at + 4);
                    longueur = content_length(&brut[..at]);
                }
            }
            if corps.is_some_and(|at| brut.len() >= at + longueur) {
                break;
            }
        }
        if let Some(at) = corps {
            let fin = (at + longueur).min(brut.len());
            if let Ok(requete) = serde_json::from_slice::<Value>(&brut[at..fin]) {
                vus.lock().unwrap_or_else(|e| e.into_inner()).push(requete);
            }
        }
        let tete = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            reponse.len()
        );
        let _ = flux.write_all(tete.as_bytes());
        let _ = flux.write_all(reponse.as_bytes());
        let _ = flux.flush();
    }

    fn content_length(tete: &[u8]) -> usize {
        String::from_utf8_lossy(tete)
            .lines()
            .find_map(|ligne| {
                let (nom, valeur) = ligne.split_once(':')?;
                nom.eq_ignore_ascii_case("content-length")
                    .then(|| valeur.trim().parse().unwrap_or(0))
            })
            .unwrap_or(0)
    }

    /// Annonce LiteMCP d'un pair (`%TEMP%/litemcp-registry/swarm-<agent>.json`), retirée à la
    /// fin du test : `peers::find` ne lit que ce registre global, c'est le seul moyen de lui
    /// faire voir un pair sans lancer de serveur.
    struct Annonce {
        fichier: PathBuf,
    }

    impl Annonce {
        fn ecrire(agent: &str, port: u16, pid: u32) -> Annonce {
            let dossier = std::env::temp_dir().join("litemcp-registry");
            std::fs::create_dir_all(&dossier).unwrap();
            let fichier = dossier.join(format!("swarm-{agent}.json"));
            let annonce = json!({
                "name": format!("swarm-{agent}"),
                "addr": format!("127.0.0.1:{port}"),
                "port": port,
                "pid": pid,
            });
            std::fs::write(&fichier, annonce.to_string()).unwrap();
            Annonce { fichier }
        }
    }

    impl Drop for Annonce {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.fichier);
        }
    }

    #[test]
    fn un_nom_invalide_ou_un_prompt_vide_sont_refuses_avant_tout_chemin() {
        let dir = ws("gards");
        let error = run(&options(&dir), &dir, "discord", "   ").unwrap_err();
        assert_eq!(error, "prompt vide.");
        let error = run(&options(&dir), &dir, "../évasion", "salut").unwrap_err();
        assert!(error.starts_with("agent:"), "{error}");
        // `--json` (option globale) ne change ni les gardes ni leur message.
        let json = Options { json: true, ..options(&dir) };
        assert_eq!(run(&json, &dir, "discord", "   ").unwrap_err(), "prompt vide.");
        let error = run(&json, &dir, "../évasion", "salut").unwrap_err();
        assert!(error.starts_with("agent:"), "{error}");
    }

    #[test]
    fn un_heberge_sans_url_est_rejete_sans_reseau() {
        let dir = ws("heberge");
        // Chemin 1 : l'agent est hébergé par ce process, mais son serveur n'a pas d'URL
        // (fenêtre entre le démarrage et l'annonce) : erreur dédiée, aucun réseau.
        let ctx =
            Arc::new(Ctx::new("sans-url", Runner::Hermes, dir.clone(), "swarm-control-1".into()));
        let error = hosted(&ctx, "salut", &options(&dir)).unwrap_err();
        assert!(error.contains("hébergé ici mais sans URL MCP"), "{error}");
    }

    #[test]
    fn un_heberge_avec_url_confie_le_tour_a_l_outil_ask_de_son_serveur() {
        let dir = ws("heberge-url");
        let serveur = FauxMcp::start(REPONSE_OK);
        let ctx =
            Arc::new(Ctx::new("alice", Runner::Claude, dir.clone(), "swarm-control-1".into()));
        ctx.set_url(serveur.url.clone());
        let mut opts = options(&dir);
        opts.timeout_ms = 10; // borné à 1 s pour l'outil `ask`
        opts.model = Some("sonnet".into());
        assert_eq!(hosted(&ctx, "état du marché ?", &opts), Ok(()));
        let vues = serveur.vues();
        assert_eq!(vues.len(), 1, "un seul appel");
        assert_eq!(vues[0]["method"], json!("tools/call"));
        let args = &vues[0]["params"]["arguments"];
        assert_eq!(vues[0]["params"]["name"], json!("ask"));
        assert_eq!(args["prompt"], json!("état du marché ?"));
        assert_eq!(args["from"], json!(owners::instance_name()));
        assert_eq!(args["timeoutMs"], json!(1_000), "délai borné par le bas");
        assert_eq!(args["model"], json!("sonnet"));
        // Sans modèle imposé le champ reste absent ; le délai est plafonné par le haut.
        opts.model = None;
        opts.timeout_ms = ASK_MAX_TIMEOUT_MS * 2;
        assert_eq!(hosted(&ctx, "et la suite ?", &opts), Ok(()));
        let args = &serveur.vues()[1]["params"]["arguments"];
        assert!(args.get("model").is_none());
        assert_eq!(args["timeoutMs"], json!(ASK_MAX_TIMEOUT_MS), "délai plafonné");
    }

    #[test]
    fn un_refus_de_l_outil_ask_du_serveur_remonte_tel_quel() {
        let dir = ws("heberge-refus");
        let serveur = FauxMcp::start(REPONSE_REFUS);
        let ctx = Arc::new(Ctx::new("bob", Runner::Claude, dir.clone(), "swarm-control-1".into()));
        ctx.set_url(serveur.url.clone());
        let error = hosted(&ctx, "un tour ?", &options(&dir)).unwrap_err();
        assert_eq!(error, "AGENT_BUSY: un tour est déjà en cours.");
    }

    #[test]
    fn un_pair_vivant_annonce_recoit_l_ask() {
        let dir = ws("pair");
        let serveur = FauxMcp::start(REPONSE_OK);
        let agent = agent("pair");
        // Annoncé vivant sous `swarm-<agent>` (notre propre PID) : `run` choisit le pair.
        let _annonce = Annonce::ecrire(&agent, serveur.addr.port(), std::process::id());
        assert_eq!(run(&options(&dir), &dir, &agent, "un coup de main ?"), Ok(()));
        let vues = serveur.vues();
        assert_eq!(vues.len(), 1, "un seul appel, au pair");
        assert_eq!(vues[0]["method"], json!("tools/call"));
        assert_eq!(vues[0]["params"]["name"], json!("ask"));
        assert_eq!(vues[0]["params"]["arguments"]["prompt"], json!("un coup de main ?"));
        assert_eq!(vues[0]["params"]["arguments"]["timeoutMs"], json!(1_000));
    }

    #[test]
    fn un_pair_vivant_mais_injoignable_est_signale_et_ne_passe_pas_en_local() {
        let dir = ws("injoignable");
        let agent = agent("muet");
        let _annonce = Annonce::ecrire(&agent, port_ferme(), std::process::id());
        let error = run(&options(&dir), &dir, &agent, "personne ?").unwrap_err();
        assert!(error.starts_with(&format!("agent '{agent}' annoncé mais injoignable")), "{error}");
    }

    #[test]
    fn une_annonce_morte_ne_compte_pas_et_la_demande_chute_en_local() {
        let dir = ws("annonce-morte");
        let agent = agent("mort");
        // PID fantôme (impair : jamais attribuable sous Windows) : annoncé, mais mort.
        let _annonce = Annonce::ecrire(&agent, port_ferme(), 3_000_000_105);
        let error = run(&options(&dir), &dir, &agent, "salut").unwrap_err();
        assert!(error.contains(&agent), "{error}");
        assert!(error.contains("introuvable"), "{error}");
    }

    #[test]
    fn sans_heberge_ni_pair_la_demande_chute_jusqu_au_run_local() {
        let dir = ws("local");
        let agent = agent("local");
        // Ni hébergé (registre vide), ni pair annoncé à ce nom, ni fichiers d'agent : le
        // chemin local est bien le dernier recours, et son refus ne lance aucun process.
        let error = run(&options(&dir), &dir, &agent, "salut").unwrap_err();
        assert!(error.contains(&agent), "{error}");
        assert!(error.contains("introuvable"), "{error}");
    }

    #[test]
    fn un_run_local_refuse_est_journalise_comme_un_tour_du_resident() {
        let dir = ws("journal");
        let agent = agent("journal");
        // L'agent existe sur disque (fichiers Claude dans le workspace) : c'est le chemin
        // local. Un prompt blanc y est refusé par `runners::prepare` AVANT toute réservation
        // ou lancement : aucun bail, aucun process, et le refus est quand même journalisé.
        let profil = crate::agents::claude_prompt_path(&dir, &agent);
        std::fs::create_dir_all(profil.parent().unwrap()).unwrap();
        std::fs::write(&profil, "# profil de test").unwrap();
        let error = local(&options(&dir), &dir, &agent, "   ").unwrap_err();
        assert!(error.contains("prompt vide"), "{error}");
        let tours = history::read(&dir, "claude", &agent, 10, None).unwrap();
        assert_eq!(tours.len(), 1, "le refus est un tour journalisé");
        let tour = &tours[0];
        assert_eq!(tour.prompt, "   ");
        assert_eq!(tour.error.as_deref(), Some(error.as_str()));
        assert_eq!(tour.from.as_deref(), Some(owners::instance_name().as_str()));
        assert_eq!((tour.runner.as_str(), tour.agent.as_str()), ("claude", agent.as_str()));
        assert_eq!(tour.queued_ms, 0, "pas de file côté CLI");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
