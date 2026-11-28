//! TUI de l'agent (feature `tui`).
//!
//! Plein écran alterné, mode brut, restauration garantie (sortie normale, erreur, panique).
//! La TUI ne fait que **lire** l'agent (`status_json`, `last_output`, journal) et lui parler par
//! `Ctx::submit` : elle ne bloque jamais le fil de travail.
//!
//! Panneaux (PLAN §4.3) : en-tête · pairs MCP · conversation · file et tour en cours · logs
//! (`d`) · saisie. Raccourcis : Entrée envoie un `ask`, `r` oublie la session, `s` tue le tour
//! en cours, `d` bascule les logs, `q` / Ctrl-C quittent.
//!
//! Le contrat avec `cli/agent.rs` est [`Exit`] et [`run`].

mod info;
mod input;
mod probe;
mod state;
mod ui;

use crate::agents;
use crate::history;
use crate::registry::{self, Status};
use crate::residents::Ctx;
use crate::residents::turn::Turn;
use input::Action;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::cursor;
use ratatui::crossterm::event;
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use state::{Level, PeerRow, State};
use std::io::{self, Stdout};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

/// Ce que la TUI demande en se fermant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    /// L'utilisateur quitte : l'agent s'arrête s'il a été lancé au premier plan.
    Quit,
    /// L'agent doit s'arrêter (même chose, explicite).
    #[allow(dead_code)] // le contrat garde `Stop` pour l'appelant ; la TUI ne s'en sert pas.
    Stop,
}

/// Ouvre la TUI sur l'agent résident `ctx` ; bloque jusqu'à `q` / Ctrl-C.
pub fn run(ctx: Arc<Ctx>, debug: bool) -> Result<Exit, String> {
    let mut screen = Screen::enter().map_err(|e| format!("terminal : {e}"))?;
    let guard = PanicGuard::install();
    // Les sondes (réseau borné) tournent dans leur propre fil ; sans fil, la TUI marche
    // quand même, sans l'état des serveurs.
    let stop = Arc::new(AtomicBool::new(false));
    let sondes = spawn_probes(Arc::clone(&ctx), Arc::clone(&stop)).ok();
    let result = event_loop(&ctx, debug, &mut screen.terminal, sondes.as_ref().map(|(_, rx)| rx));
    stop.store(true, Ordering::SeqCst);
    if let Some((handle, _)) = sondes {
        let _ = handle.join();
    }
    drop(guard);
    result
}

fn event_loop(
    ctx: &Ctx,
    debug: bool,
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    probes: Option<&Receiver<ProbeMsg>>,
) -> Result<Exit, String> {
    let mut st = State::new(info::identify(ctx), debug, Instant::now());
    if probes.is_none() {
        st.say(Level::Warn, "sondes indisponibles : l'état des serveurs ne sera pas affiché");
    }
    loop {
        sync(ctx, &mut st, probes);
        terminal.draw(|frame| ui::draw(frame, &mut st)).map_err(|e| format!("affichage : {e}"))?;
        if !event::poll(state::STATUS_EVERY).map_err(|e| format!("terminal : {e}"))? {
            continue;
        }
        let event = event::read().map_err(|e| format!("terminal : {e}"))?;
        let action = input::translate(event, st.input_empty());
        if let Some(exit) = handle(action, &mut st, ctx) {
            return Ok(exit);
        }
    }
}

/// Un message du fil des sondes vers l'interface.
enum ProbeMsg {
    Peers(Vec<PeerRow>),
    Model(Option<String>),
}

/// Attente d'une sonde réseau : assez courte pour ne pas engorger le fil, assez longue
/// pour un aller-retour HTTP local.
const PROBE_WAIT: Duration = Duration::from_millis(700);

/// Lance le fil des sondes : état des serveurs MCP du registre et modèle de l'agent,
/// toutes les [`state::PEERS_EVERY`]. `stop` est testé par tranches de 200 ms.
fn spawn_probes(
    ctx: Arc<Ctx>,
    stop: Arc<AtomicBool>,
) -> Result<(std::thread::JoinHandle<()>, Receiver<ProbeMsg>), String> {
    let (tx, rx) = mpsc::channel();
    let handle = std::thread::Builder::new()
        .name(format!("swarm-tui-probe-{}", ctx.agent))
        .spawn(move || probe_loop(ctx, tx, stop))
        .map_err(|e| format!("fil des sondes impossible : {e}"))?;
    Ok((handle, rx))
}

fn probe_loop(ctx: Arc<Ctx>, tx: Sender<ProbeMsg>, stop: Arc<AtomicBool>) {
    let period = state::PEERS_EVERY;
    let step = Duration::from_millis(200);
    while !stop.load(Ordering::SeqCst) {
        let model = agents::list(&ctx.workspace)
            .into_iter()
            .find(|a| a.name == ctx.agent && a.runner == ctx.runner)
            .map(|a| a.model);
        let _ = tx.send(ProbeMsg::Model(model));
        let peers = probe::probe_all(&ctx.agent, PROBE_WAIT);
        let _ = tx.send(ProbeMsg::Peers(peers));
        let mut slept = Duration::ZERO;
        while slept < period && !stop.load(Ordering::SeqCst) {
            std::thread::sleep(step);
            slept += step;
        }
    }
}

/// Sortie en direct : celle du run en cours (registre, en mémoire), sinon la fin de la
/// sortie du dernier tour (le `Ctx` la garde pour `logs` et la TUI).
fn live_output(ctx: &Ctx) -> String {
    let run = registry::find(&ctx.agent, Some(ctx.runner.as_str()));
    match run {
        Some(run) if run.snapshot().status == Status::Running => run.output_since(None),
        _ => ctx.last_output(),
    }
}

/// Relit ce qui a changé : sondes (canal du fil dédié), statut (~200 ms), journal (si son
/// empreinte a changé). Les sondes viennent du fil dédié : ici on ne fait que vider le
/// canal, sans jamais bloquer sur le réseau.
fn sync(ctx: &Ctx, st: &mut State, probes: Option<&Receiver<ProbeMsg>>) {
    if let Some(rx) = probes {
        while let Ok(msg) = rx.try_recv() {
            match msg {
                ProbeMsg::Peers(peers) => st.apply_peers(peers),
                ProbeMsg::Model(model) => st.apply_model(model),
            }
        }
    }
    let due = st.timers.due(Instant::now());
    if due.status {
        st.apply_status(&ctx.status_json());
        if st.show_logs {
            st.apply_logs(live_output(ctx));
        }
    }
    if due.history {
        let fingerprint = history::fingerprint(&ctx.workspace, ctx.runner.as_str(), &ctx.agent);
        if st.fingerprint != Some(fingerprint) {
            match history::read(
                &ctx.workspace,
                ctx.runner.as_str(),
                &ctx.agent,
                state::HISTORY_LIMIT,
                None,
            ) {
                Ok(entries) => st.apply_history(entries),
                Err(error) => st.say(Level::Error, format!("journal illisible : {error}")),
            }
            st.fingerprint = Some(fingerprint);
        }
    }
}

fn handle(action: Action, st: &mut State, ctx: &Ctx) -> Option<Exit> {
    match action {
        Action::None => {}
        Action::Quit => return Some(Exit::Quit),
        Action::Char(c) => st.type_char(c),
        Action::Backspace => st.backspace(),
        Action::Clear => st.clear_input(),
        Action::ScrollUp(lines) => st.scroll_up(lines),
        Action::ScrollDown(lines) => st.scroll_down(lines),
        Action::Top => st.go_top(),
        Action::Bottom => st.go_bottom(),
        Action::Logs => {
            st.toggle_logs();
            if st.show_logs {
                st.apply_logs(live_output(ctx));
            }
        }
        Action::Submit => submit(st, ctx),
        Action::Reset => reset(st, ctx),
        Action::Stop => stop(st, ctx),
    }
    None
}

/// Entrée : la saisie part en `ask` (`from = "tui"`).
fn submit(st: &mut State, ctx: &Ctx) {
    let Some(prompt) = st.take_prompt() else {
        st.say(Level::Warn, "message vide : rien n'a été envoyé");
        return;
    };
    let busy = st.status.busy();
    let (turn, _outcome) = Turn::new(prompt.clone(), Some("tui".to_string()), None);
    match ctx.submit(turn) {
        Ok(()) => {
            let text = if busy {
                "demande mise en file : un tour est déjà en cours"
            } else {
                "demande envoyée : elle démarre"
            };
            st.say(Level::Ok, text);
        }
        Err(error) => {
            st.set_input(prompt);
            st.say(Level::Error, error);
        }
    }
}

/// `r` : oublie la session (le prochain tour repart à neuf).
fn reset(st: &mut State, ctx: &Ctx) {
    let busy = st.status.busy();
    let existed = ctx.reset_session();
    let (level, text) = if busy {
        (Level::Ok, "session oubliée : elle le sera de nouveau à la fin du tour en cours")
    } else if existed {
        (Level::Ok, "session oubliée : le prochain tour repart à neuf")
    } else {
        (Level::Warn, "aucune session à oublier")
    };
    st.say(level, text);
}

/// `s` : tue le tour en cours (la file n'est pas touchée).
fn stop(st: &mut State, ctx: &Ctx) {
    if ctx.kill_current() {
        st.say(Level::Warn, "tour en cours interrompu");
    } else {
        st.say(Level::Info, "aucun tour en cours");
    }
}

/// Terminal alterné et mode brut, restaurés par [`Drop`] comme par le crochet de panique.
struct Screen {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl Screen {
    fn enter() -> io::Result<Screen> {
        terminal::enable_raw_mode()?;
        let entered = (|| {
            execute!(io::stdout(), EnterAlternateScreen)?;
            let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
            terminal.clear()?;
            Ok(terminal)
        })();
        match entered {
            Ok(terminal) => Ok(Screen { terminal }),
            Err(error) => {
                restore();
                Err(error)
            }
        }
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        restore();
    }
}

fn restore() {
    let _ = terminal::disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen, cursor::Show);
}

/// Restaure le terminal si la TUI panique, sans toucher aux paniques des autres fils
/// (le fil de travail d'un agent résident vit dans le même process).
struct PanicGuard {
    armed: Arc<AtomicBool>,
}

impl PanicGuard {
    fn install() -> PanicGuard {
        let armed = Arc::new(AtomicBool::new(true));
        let flag = Arc::clone(&armed);
        let tui_thread = std::thread::current().id();
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if flag.load(Ordering::SeqCst) && std::thread::current().id() == tui_thread {
                restore();
            }
            previous(info);
        }));
        PanicGuard { armed }
    }
}

impl Drop for PanicGuard {
    fn drop(&mut self) {
        self.armed.store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::Runner;
    use crate::history::{self, Entry};
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    use serde_json::json;

    /// Un Ctx isolé (workspace temporaire unique) : aucun serveur résident n'est démarré, donc
    /// aucun fil de travail — un tour soumis reste en file, rien n'est lancé.
    fn ctx(agent: &str) -> Ctx {
        let ws = std::env::temp_dir().join(format!("swarm-tui-{}-{agent}", std::process::id()));
        Ctx::new(agent, Runner::Claude, ws, "swarm-control-1".into())
    }

    fn state(ctx: &Ctx) -> State {
        State::new(info::identify(ctx), false, Instant::now())
    }

    /// Un faux terminal de 100×30.
    fn terminal() -> Terminal<TestBackend> {
        Terminal::new(TestBackend::new(100, 30)).unwrap()
    }

    fn touche(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    /// Une itération de `event_loop` sur le faux terminal : la touche est traduite selon l'état
    /// de la saisie, l'action appliquée, l'écran redessiné — sans `poll`/`read` ni terminal réel.
    fn iteration(
        st: &mut State,
        ctx: &Ctx,
        term: &mut Terminal<TestBackend>,
        event: Event,
    ) -> Option<Exit> {
        let action = input::translate(event, st.input_empty());
        let exit = handle(action, st, ctx);
        term.draw(|frame| ui::draw(frame, st)).unwrap();
        exit
    }

    /// Tape `texte` touche par touche, comme l'utilisateur.
    fn taper(st: &mut State, ctx: &Ctx, term: &mut Terminal<TestBackend>, texte: &str) {
        for c in texte.chars() {
            assert_eq!(iteration(st, ctx, term, touche(KeyCode::Char(c))), None, "touche {c:?}");
        }
    }

    fn ecran(term: &mut Terminal<TestBackend>) -> String {
        term.backend().to_string()
    }

    fn notice(st: &State) -> String {
        st.notice.as_ref().map(|n| n.text.clone()).unwrap_or_default()
    }

    fn tour(ts: u64, prompt: &str) -> Entry {
        Entry { ts, prompt: prompt.into(), ..Default::default() }
    }

    #[test]
    fn le_rendu_initial_montre_les_panneaux_et_est_deterministe() {
        let ctx = ctx("rendu");
        let mut st = state(&ctx);
        let mut term = terminal();
        term.draw(|frame| ui::draw(frame, &mut st)).unwrap();
        let premier = ecran(&mut term);
        for panneau in ["agent", "serveurs MCP", "conversation", "file et tour en cours", "message"]
        {
            assert!(premier.contains(panneau), "panneau absent « {panneau} »\n{premier}");
        }
        // État vide : placeholders prévus, aucun panic.
        assert!(premier.contains("le journal est vide"));
        assert!(premier.contains("au repos"));
        term.draw(|frame| ui::draw(frame, &mut st)).unwrap();
        assert_eq!(ecran(&mut term), premier, "même état → même écran");
        // Terminal trop petit : un message, pas de panic.
        let mut petit = Terminal::new(TestBackend::new(30, 10)).unwrap();
        petit.draw(|frame| ui::draw(frame, &mut st)).unwrap();
        assert!(petit.backend().to_string().contains("terminal trop petit"));
    }

    #[test]
    fn la_saisie_part_dans_la_file_de_l_agent_par_un_ask_sans_lancer_de_tour() {
        let ctx = ctx("envoi");
        let mut st = state(&ctx);
        let mut term = terminal();
        term.draw(|frame| ui::draw(frame, &mut st)).unwrap();
        taper(&mut st, &ctx, &mut term, "état du marché ?");
        assert_eq!(st.input, "état du marché ?");
        assert!(ecran(&mut term).contains("état du marché ?"), "la saisie s'affiche");
        assert_eq!(ctx.status_json()["queueLength"], json!(0), "rien n'est envoyé avant Entrée");
        assert_eq!(iteration(&mut st, &ctx, &mut term, touche(KeyCode::Enter)), None);
        assert!(st.input.is_empty(), "la saisie est détournée");
        let status = ctx.status_json();
        assert_eq!(status["queueLength"], json!(1), "le tour est dans la file de l'agent");
        assert_eq!(status["state"], json!("idle"), "aucun fil de travail : rien ne tourne");
        assert!(notice(&st).contains("demande envoyée"), "{}", notice(&st));
        assert!(ecran(&mut term).contains("demande envoyée"));
    }

    #[test]
    fn entree_sur_ligne_vide_avertit_et_n_envoie_rien() {
        let ctx = ctx("vide");
        let mut st = state(&ctx);
        let mut term = terminal();
        assert_eq!(iteration(&mut st, &ctx, &mut term, touche(KeyCode::Enter)), None);
        assert_eq!(ctx.status_json()["queueLength"], json!(0));
        assert!(notice(&st).contains("vide"), "{}", notice(&st));
    }

    #[test]
    fn les_raccourcis_r_s_d_q_n_agissent_que_sur_une_ligne_vide() {
        let ctx = ctx("raccourcis");
        let mut st = state(&ctx);
        let mut term = terminal();
        term.draw(|frame| ui::draw(frame, &mut st)).unwrap();
        // Ligne vide : `r` oublie la session, `s` tue le tour, `d` bascule les logs.
        assert_eq!(iteration(&mut st, &ctx, &mut term, touche(KeyCode::Char('r'))), None);
        assert!(notice(&st).contains("aucune session à oublier"), "{}", notice(&st));
        assert_eq!(iteration(&mut st, &ctx, &mut term, touche(KeyCode::Char('s'))), None);
        assert!(notice(&st).contains("aucun tour en cours"), "{}", notice(&st));
        assert!(!st.show_logs);
        assert_eq!(iteration(&mut st, &ctx, &mut term, touche(KeyCode::Char('d'))), None);
        assert!(st.show_logs);
        assert!(ecran(&mut term).contains("logs · sortie brute du dernier tour"), "panneau logs");
        assert_eq!(iteration(&mut st, &ctx, &mut term, touche(KeyCode::Char('d'))), None);
        assert!(!st.show_logs);
        assert!(!ecran(&mut term).contains("logs · sortie brute du dernier tour"), "masqué");
        assert!(st.input.is_empty(), "aucun raccourci n'écrit dans la saisie");
        // Ligne écrite (une espace de tête, puis les quatre lettres) : tout est du texte, rien
        // ne se déclenche, pas même `q`.
        taper(&mut st, &ctx, &mut term, " qrsd");
        assert_eq!(st.input, " qrsd");
        assert!(!st.show_logs);
        assert_eq!(ctx.status_json()["queueLength"], json!(0));
        // Entrée envoie le mot tel quel (l'espace de tête est retirée) ; la ligne redevient vide.
        assert_eq!(iteration(&mut st, &ctx, &mut term, touche(KeyCode::Enter)), None);
        assert_eq!(ctx.status_json()["queueLength"], json!(1));
        assert!(st.input.is_empty());
        // `q` sur une ligne vide ferme la TUI : le contrat avec cli/agent.rs.
        assert_eq!(
            iteration(&mut st, &ctx, &mut term, touche(KeyCode::Char('q'))),
            Some(Exit::Quit)
        );
    }

    #[test]
    fn echap_efface_la_saisie_et_reaffiche_le_placeholder() {
        let ctx = ctx("echap");
        let mut st = state(&ctx);
        let mut term = terminal();
        taper(&mut st, &ctx, &mut term, "bonjour");
        assert!(ecran(&mut term).contains("bonjour"), "la saisie s'affiche");
        assert_eq!(iteration(&mut st, &ctx, &mut term, touche(KeyCode::Esc)), None);
        assert!(st.input.is_empty(), "Échap efface la ligne");
        let vide = ecran(&mut term);
        assert!(!vide.contains("bonjour"), "{vide}");
        assert!(vide.contains("écrivez un message…"), "{vide}");
        // Échap sur une ligne déjà vide : rien, pas de panique.
        assert_eq!(iteration(&mut st, &ctx, &mut term, touche(KeyCode::Esc)), None);
        assert!(st.input.is_empty());
    }

    #[test]
    fn le_defilement_est_borne_par_ce_qui_a_ete_rendu() {
        let ctx = ctx("defilement");
        let mut st = state(&ctx);
        st.apply_history((1..=12).map(|i| tour(i, &format!("tour {i:02}"))).collect());
        let mut term = terminal();
        term.draw(|frame| ui::draw(frame, &mut st)).unwrap();
        let bas = ecran(&mut term);
        assert!(bas.contains("tour 12") && !bas.contains("tour 01"), "en bas : la fin\n{bas}");
        assert!(st.conversation.following());
        assert_eq!(iteration(&mut st, &ctx, &mut term, touche(KeyCode::PageUp)), None);
        assert_eq!(st.conversation.offset(), 10);
        assert_eq!(iteration(&mut st, &ctx, &mut term, touche(KeyCode::Up)), None);
        assert_eq!(st.conversation.offset(), 11);
        assert_eq!(iteration(&mut st, &ctx, &mut term, touche(KeyCode::Home)), None);
        let haut = ecran(&mut term);
        assert!(
            haut.contains("tour 01") && !haut.contains("tour 12"),
            "en haut : le début\n{haut}"
        );
        let maximum = st.conversation.offset();
        assert!(maximum >= 11, "Home va jusqu'à la borne rendue ({maximum})");
        assert_eq!(iteration(&mut st, &ctx, &mut term, touche(KeyCode::PageUp)), None);
        assert_eq!(st.conversation.offset(), maximum, "borné par ce qui a été rendu");
        assert_eq!(iteration(&mut st, &ctx, &mut term, touche(KeyCode::End)), None);
        assert_eq!(st.conversation.offset(), 0);
        assert!(ecran(&mut term).contains("tour 12"), "End revient à la fin");
        assert_eq!(iteration(&mut st, &ctx, &mut term, touche(KeyCode::Down)), None);
        assert_eq!(st.conversation.offset(), 0, "déjà en bas : on ne descend pas plus bas");
        assert_eq!(iteration(&mut st, &ctx, &mut term, touche(KeyCode::PageUp)), None);
        assert_eq!(st.conversation.offset(), 10);
        assert_eq!(iteration(&mut st, &ctx, &mut term, touche(KeyCode::PageDown)), None);
        assert_eq!(st.conversation.offset(), 0);
        // Un contenu plus court que la vue : plus aucun décalage possible.
        st.apply_history(vec![tour(1, "seul tour")]);
        term.draw(|frame| ui::draw(frame, &mut st)).unwrap();
        assert_eq!(iteration(&mut st, &ctx, &mut term, touche(KeyCode::Home)), None);
        assert_eq!(st.conversation.offset(), 0);
    }

    #[test]
    fn la_premiere_synchronisation_alimente_statut_et_journal() {
        let ctx = ctx("sync");
        let mut st = state(&ctx);
        sync(&ctx, &mut st, None);
        assert_eq!(st.status.state, "idle", "aucun tour en cours sur un contexte neuf");
        assert!(st.fingerprint.is_some(), "le journal a été relu (même vide)");
        assert!(st.conversation.is_empty());
    }

    #[test]
    fn la_synchronisation_charge_le_journal_de_l_agent_dans_la_conversation() {
        let ctx = ctx("journal");
        history::record(
            &ctx.workspace,
            "claude",
            &ctx.agent,
            Entry { prompt: "question ?".into(), reply: "réponse.".into(), ..Default::default() },
        );
        let mut st = state(&ctx);
        sync(&ctx, &mut st, None);
        assert_eq!(st.conversation.len(), 1);
        assert_eq!(st.conversation.entries()[0].reply, "réponse.");
        let mut term = terminal();
        term.draw(|frame| ui::draw(frame, &mut st)).unwrap();
        let ecran = ecran(&mut term);
        assert!(ecran.contains("conversation (1)"), "{ecran}");
        assert!(ecran.contains("question ?") && ecran.contains("réponse."), "{ecran}");
        let _ = std::fs::remove_dir_all(&ctx.workspace);
    }

    #[test]
    fn les_messages_du_fil_des_sondes_sont_vides_dans_l_etat() {
        let ctx = ctx("sondes");
        let (tx, rx) = mpsc::channel();
        tx.send(ProbeMsg::Model(Some("sonnet-4-5".into()))).unwrap();
        tx.send(ProbeMsg::Peers(vec![PeerRow {
            name: "swarm-alice".into(),
            addr: "127.0.0.1:23082".into(),
            alive: true,
            own: true,
            state: "ok".into(),
            detail: "idle · file 0".into(),
        }]))
        .unwrap();
        drop(tx);
        let mut st = state(&ctx);
        sync(&ctx, &mut st, Some(&rx));
        assert_eq!(st.model.as_deref(), Some("sonnet-4-5"));
        assert_eq!(st.peers.len(), 1);
        assert!(st.peers[0].own);
        // Canal épuisé (fil des sondes parti) : le passage suivant ne bloque pas, ne change rien.
        sync(&ctx, &mut st, Some(&rx));
        assert_eq!(st.peers.len(), 1);
        assert_eq!(st.model.as_deref(), Some("sonnet-4-5"));
    }

    #[test]
    fn sans_run_vivant_la_sortie_affichee_est_celle_du_dernier_tour() {
        let ctx = ctx("logs");
        assert_eq!(live_output(&ctx), "", "registre sans run : la sortie du contexte (vide)");
    }
}
