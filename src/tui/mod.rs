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
mod state;
mod ui;

use crate::history;
use crate::peers;
use crate::residents::Ctx;
use crate::residents::turn::Turn;
use input::Action;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::cursor;
use ratatui::crossterm::event;
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use state::{Level, State};
use std::io::{self, Stdout};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

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
    let result = event_loop(&ctx, debug, &mut screen.terminal);
    drop(guard);
    result
}

fn event_loop(
    ctx: &Ctx,
    debug: bool,
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
) -> Result<Exit, String> {
    let mut st = State::new(info::identify(ctx), debug, Instant::now());
    loop {
        sync(ctx, &mut st);
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

/// Relit ce qui a changé : statut (~200 ms), journal (si son empreinte a changé), pairs (~5 s).
fn sync(ctx: &Ctx, st: &mut State) {
    let due = st.timers.due(Instant::now());
    if due.status {
        st.apply_status(&ctx.status_json());
        if st.show_logs {
            st.apply_logs(ctx.last_output());
        }
    }
    if due.peers {
        st.apply_peers(peers::discover());
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
        Action::Top => st.to_top(),
        Action::Bottom => st.to_bottom(),
        Action::Logs => {
            st.toggle_logs();
            if st.show_logs {
                st.apply_logs(ctx.last_output());
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
