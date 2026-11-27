//! Traduction des évènements du terminal en actions de la TUI.
//!
//! Règle des raccourcis : les lettres nues (`r`, `s`, `d`, `q`) n'agissent que **ligne de
//! saisie vide** ; dès qu'un caractère est présent, tout est du texte. Pour taper un message
//! qui commence par une de ces minuscules, on commence par une espace (retirée à l'envoi).
//! `Ctrl-C` quitte toujours.

use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

/// Ce que la TUI doit faire d'une touche.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Quit,
    Submit,
    Char(char),
    Backspace,
    Clear,
    ScrollUp(usize),
    ScrollDown(usize),
    Top,
    Bottom,
    Logs,
    Reset,
    Stop,
}

pub fn translate(event: Event, input_empty: bool) -> Action {
    match event {
        Event::Key(key) => key_action(key, input_empty),
        _ => Action::None,
    }
}

pub fn key_action(key: KeyEvent, input_empty: bool) -> Action {
    // Sous Windows, crossterm signale aussi les relâchements : sans ce filtre, chaque
    // caractère serait tapé deux fois.
    if matches!(key.kind, KeyEventKind::Release) {
        return Action::None;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) && !key.modifiers.contains(KeyModifiers::ALT) {
        return match key.code {
            KeyCode::Char('c' | 'C') => Action::Quit,
            _ => Action::None,
        };
    }
    match key.code {
        KeyCode::Enter => Action::Submit,
        KeyCode::Backspace | KeyCode::Delete => Action::Backspace,
        KeyCode::Esc => Action::Clear,
        KeyCode::Up => Action::ScrollUp(1),
        KeyCode::Down => Action::ScrollDown(1),
        KeyCode::PageUp => Action::ScrollUp(10),
        KeyCode::PageDown => Action::ScrollDown(10),
        KeyCode::Home => Action::Top,
        KeyCode::End => Action::Bottom,
        KeyCode::Char(c) => {
            if input_empty && !key.modifiers.contains(KeyModifiers::ALT) {
                match c {
                    'q' => return Action::Quit,
                    'r' => return Action::Reset,
                    's' => return Action::Stop,
                    'd' => return Action::Logs,
                    _ => {}
                }
            }
            Action::Char(c)
        }
        _ => Action::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn les_lettres_sont_du_texte_quand_la_ligne_est_pleine() {
        assert_eq!(key_action(key(KeyCode::Char('r')), false), Action::Char('r'));
        assert_eq!(key_action(key(KeyCode::Char('s')), false), Action::Char('s'));
        assert_eq!(key_action(key(KeyCode::Char('d')), false), Action::Char('d'));
        assert_eq!(key_action(key(KeyCode::Char('q')), false), Action::Char('q'));
        assert_eq!(key_action(key(KeyCode::Char('a')), true), Action::Char('a'));
    }

    #[test]
    fn les_raccourcis_agissent_sur_une_ligne_vide() {
        assert_eq!(key_action(key(KeyCode::Char('r')), true), Action::Reset);
        assert_eq!(key_action(key(KeyCode::Char('s')), true), Action::Stop);
        assert_eq!(key_action(key(KeyCode::Char('d')), true), Action::Logs);
        assert_eq!(key_action(key(KeyCode::Char('q')), true), Action::Quit);
        // Une majuscule reste du texte : « Regarde » ne déclenche pas `r`.
        let shift = KeyEvent::new(KeyCode::Char('R'), KeyModifiers::SHIFT);
        assert_eq!(key_action(shift, true), Action::Char('R'));
    }

    #[test]
    fn entree_effacement_et_defilement() {
        assert_eq!(key_action(key(KeyCode::Enter), false), Action::Submit);
        assert_eq!(key_action(key(KeyCode::Backspace), false), Action::Backspace);
        assert_eq!(key_action(key(KeyCode::Delete), false), Action::Backspace);
        assert_eq!(key_action(key(KeyCode::Esc), false), Action::Clear);
        assert_eq!(key_action(key(KeyCode::Up), true), Action::ScrollUp(1));
        assert_eq!(key_action(key(KeyCode::PageDown), true), Action::ScrollDown(10));
        assert_eq!(key_action(key(KeyCode::Home), true), Action::Top);
        assert_eq!(key_action(key(KeyCode::End), true), Action::Bottom);
        assert_eq!(key_action(key(KeyCode::Tab), true), Action::None);
    }

    #[test]
    fn ctrl_c_quitte_toujours() {
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(key_action(ctrl_c, false), Action::Quit);
        assert_eq!(key_action(ctrl_c, true), Action::Quit);
        // Ctrl+r n'est ni un reset ni du texte : rien.
        let ctrl_r = KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL);
        assert_eq!(key_action(ctrl_r, true), Action::None);
    }

    #[test]
    fn les_caracteres_altgr_restent_du_texte() {
        // AltGr arrive comme Ctrl+Alt : `{`, `[`, `|`, `@`… doivent s'écrire.
        let altgr = KeyEvent::new(KeyCode::Char('{'), KeyModifiers::CONTROL | KeyModifiers::ALT);
        assert_eq!(key_action(altgr, true), Action::Char('{'));
        assert_eq!(key_action(altgr, false), Action::Char('{'));
    }

    #[test]
    fn les_relachements_sont_ignores() {
        let release =
            KeyEvent::new_with_kind(KeyCode::Char('a'), KeyModifiers::NONE, KeyEventKind::Release);
        assert_eq!(key_action(release, true), Action::None);
        let repeat =
            KeyEvent::new_with_kind(KeyCode::Char('a'), KeyModifiers::NONE, KeyEventKind::Repeat);
        assert_eq!(key_action(repeat, true), Action::Char('a'));
    }

    #[test]
    fn les_evenements_non_clavier_sont_ignores() {
        assert_eq!(translate(Event::Resize(80, 24), true), Action::None);
    }
}
