//! Formatage du journal des tours pour le panneau conversation de la TUI.
//!
//! Chaque `history::Entry` devient quelques lignes typées [`ConvoLine`] : le rendu
//! (`ui.rs`) leur applique la couleur, les tests vérifient le texte sans ratatui.
//! L'affichage est volontairement condensé (une demande, une réponse, une erreur) ;
//! la sortie brute complète reste disponible via `last_output` (panneau sortie).

use crate::history::Entry;
use crate::store::now_ms;
use crate::tui::view;

/// Le genre d'une ligne de conversation : détermine la couleur au rendu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// En-tête d'un tour : heure, émetteur, durée, session.
    Head,
    /// La demande (prompt).
    Prompt,
    /// La réponse (ou la fin de la sortie).
    Reply,
    /// Un tour qui a échoué.
    Error,
}

/// Une ligne de conversation prête à afficher.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConvoLine {
    pub kind: Kind,
    pub text: String,
}

/// Longueur maximale d'un prompt ou d'une réponse dans la conversation.
const MAX_TEXT: usize = 256;

/// L'heure d'un horodatage Unix (ms), en UTC : le journal ne sert qu'au diagnostic,
/// l'écart entre deux lignes reste juste quelle que soit la fuseau.
pub fn hhmmss(ts_ms: u64) -> String {
    let day_secs = (ts_ms / 1000) % 86_400;
    format!("{:02}:{:02}:{:02}", day_secs / 3600, (day_secs % 3600) / 60, day_secs % 60)
}

/// Les lignes de conversation d'une entrée du journal.
pub fn entry_lines(entry: &Entry) -> Vec<ConvoLine> {
    let mut head = hhmmss(entry.ts);
    head.push_str("  ");
    head.push_str(entry.from.as_deref().unwrap_or("?"));
    if !entry.via.is_empty() {
        head.push_str(&format!(" (via {})", entry.via.join(" → ")));
    }
    head.push_str(" · ");
    head.push_str(&view::human_ms(entry.duration_ms));
    if let Some(session) = entry.session_id.as_deref() {
        head.push_str(" · sess ");
        head.push_str(&view::short(session, 12));
    }
    let mut lines = vec![ConvoLine { kind: Kind::Head, text: head }];
    lines.push(ConvoLine { kind: Kind::Prompt, text: format!("  » {}", view::clip(&entry.prompt)) });
    match entry.error.as_deref() {
        Some(error) => {
            lines.push(ConvoLine { kind: Kind::Error, text: format!("  ✗ {}", view::clip(error)) });
        }
        None if entry.reply.is_empty() => {
            lines.push(ConvoLine { kind: Kind::Reply, text: "  « —".into() });
        }
        None => {
            lines
                .push(ConvoLine { kind: Kind::Reply, text: format!("  « {}", view::clip(&entry.reply)) });
        }
    }
    lines
}

/// Heure locale de maintenant, pour horodater les événements internes de la TUI.
pub fn now_hhmmss() -> String {
    hhmmss(now_ms())
}
