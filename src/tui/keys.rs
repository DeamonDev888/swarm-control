//! Traduction des événements clavier/souris en actions de la TUI.
//!
//! Fonction pure, testée sans terminal : le fil d'interface se contente d'appliquer
//! l'action rendue. Les raccourcis (`r`, `s`, `d`, `q`) ne s'appliquent que lorsque
//! le champ de message est vide ; sinon les lettres tapent le message en cours.

use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, KeyEventKind, MouseEventKind};
use std::time::Duration;

/// Lignes de défilement d'un cran de molette.
const WHEEL: usize = 3;
/// Lignes de défilement d'une page (approximation : la vraie hauteur n'est pas connue ici).
const PAGE: usize = 20;

/// Attente maximale d'un événement entre deux rendus.
pub const POLL: Duration = Duration::from_millis(150);

/// Une action demandée par l'utilisateur.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Un caractère est ajouté au message en cours.
    Insert(char),
    /// Le caractère avant le curseur est effacé.
    Backspace,
    /// Le curseur recule d'un caractère.
    Left,
    /// Le curseur avance d'un caractère.
    Right,
    /// Le message est envoyé (un `ask` à l'agent).
    Send,
    /// Le message en cours est abandonné.
    Discard,
    /// La session de l'agent est oubliée (`r`).
    Reset,
    /// L'agent résident est arrêté (`s`).
    Stop,
    /// Le panneau debug est agrandi / réduit (`d`).
    ToggleDebug,
    /// Défile la conversation vers le passé (haut).
    ScrollUp(usize),
    /// Défile la conversation vers le présent (bas).
    ScrollDown(usize),
    /// Suit la fin de la conversation.
    Follow,
    /// Quitte la TUI (`q`) : l'agent continue.
    Quit,
    /// Quitte en demandant l'arrêt de l'agent (Ctrl-C).
    Halt,
    /// Rien à faire.
    Ignore,
}

/// Traduit un événement terminal en action. `typing` : le champ de message n'est pas vide.
pub fn handle(event: Event, typing: bool) -> Action {
    match event {
        Event::Mouse(mouse) => match mouse.kind {
            MouseEventKind::ScrollUp => Action::ScrollUp(WHEEL),
            MouseEventKind::ScrollDown => Action::ScrollDown(WHEEL),
            _ => Action::Ignore,
        },
        Event::Key(key) => key_action(key, typing),
        _ => Action::Ignore,
    }
}

fn key_action(key: KeyEvent, typing: bool) -> Action {
    // Sous Windows, crossterm signale aussi les relâchements et répétitions :
    // ne réagir qu'à l'appui, sinon chaque touche compte double.
    if key.kind != KeyEventKind::Press {
        return Action::Ignore;
    }
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Action::Halt;
    }
    match key.code {
        KeyCode::Enter => if typing { Action::Send } else { Action::Ignore },
        KeyCode::Backspace => if typing { Action::Backspace } else { Action::Ignore },
        KeyCode::Esc => if typing { Action::Discard } else { Action::Ignore },
        KeyCode::Left => Action::Left,
        KeyCode::Right => Action::Right,
        KeyCode::Up => Action::ScrollUp(1),
        KeyCode::Down => Action::ScrollDown(1),
        KeyCode::PageUp => Action::ScrollUp(PAGE),
        KeyCode::PageDown => Action::ScrollDown(PAGE),
        KeyCode::End => Action::Follow,
        KeyCode::Char(ch) => char_action(ch, key.modifiers, typing),
        _ => Action::Ignore,
    }
}

fn char_action(ch: char, modifiers: KeyModifiers, typing: bool) -> Action {
    if modifiers.contains(KeyModifiers::CONTROL) {
        return Action::Ignore;
    }
    match ch {
        'q' if !typing => Action::Quit,
        'r' if !typing => Action::Reset,
        's' if !typing => Action::Stop,
        'd' if !typing => Action::ToggleDebug,
        _ => Action::Insert(ch),
    }
}
