//! Rendu de la TUI : en-tête, pairs, conversation, file, logs, saisie.
//!
//! Le rendu ne lit que l'état ([`State`]) et ne fait aucune entrée/sortie : c'est `mod` qui
//! rafraîchit, `input` qui traduit les touches. Seule exception assumée : le rendu fixe la borne
//! de défilement, car lui seul connaît la largeur des lignes et la hauteur de la vue.

use super::info::{age_label, duration_label, short_session};
use super::state::{Level, PeerRow, State, Status, wrap};
use crate::history::Entry;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Paragraph};

const ACCENT: Color = Color::Cyan;
const DIM: Color = Color::DarkGray;
const OK: Color = Color::Green;
const WARN: Color = Color::Yellow;
const BAD: Color = Color::Red;

/// Largeur minimale du panneau des pairs, et du terminal pour l'afficher à côté.
const PEERS_WIDTH: u16 = 32;
const SPLIT_MIN: u16 = 60;

pub fn draw(frame: &mut Frame, st: &mut State) {
    let area = frame.area();
    let banner = if st.debug { 1 } else { 0 };
    let fixed = 5 + 5 + 1 + 3 + banner; // en-tête, file, ligne d'état, saisie (+ bandeau)
    if area.width < 40 || area.height < fixed + 3 {
        let hint = "terminal trop petit : agrandissez la fenêtre";
        frame.render_widget(Paragraph::new(Line::styled(hint, Style::new().fg(BAD))), area);
        return;
    }
    // Les logs prennent au plus la moitié de la place restante : la conversation et la saisie
    // gardent toujours leur hauteur, même sur un terminal court.
    let logs = if st.show_logs { ((area.height - fixed) / 2).clamp(0, 12) } else { 0 };
    let layout = Layout::vertical([
        Constraint::Length(banner),
        Constraint::Length(5),
        Constraint::Min(4),
        Constraint::Length(5),
        Constraint::Length(logs),
        Constraint::Length(1),
        Constraint::Length(3),
    ]);
    let [banner_a, header_a, main_a, queue_a, logs_a, notice_a, input_a] = layout.areas(area);
    if st.debug {
        banner_line(frame, banner_a, st);
    }
    header(frame, header_a, st);
    if main_a.width >= SPLIT_MIN {
        let [conversation_a, peers_a] =
            Layout::horizontal([Constraint::Min(24), Constraint::Length(PEERS_WIDTH)])
                .areas(main_a);
        conversation(frame, conversation_a, st);
        peers(frame, peers_a, st);
    } else {
        conversation(frame, main_a, st);
    }
    turn_panel(frame, queue_a, st);
    if st.show_logs {
        logs_panel(frame, logs_a, st);
    }
    notice_line(frame, notice_a, st);
    input_line(frame, input_a, st);
}

fn panel(title: &str) -> Block<'static> {
    Block::bordered()
        .border_style(Style::new().fg(DIM))
        .title(Line::styled(format!(" {title} "), Style::new().fg(Color::Gray)))
}

fn dim(text: impl Into<String>) -> Span<'static> {
    Span::styled(text.into(), Style::new().fg(DIM))
}

fn banner_line(frame: &mut Frame, area: Rect, st: &State) {
    let text = format!(" MODE DEBUG · {} ", st.info.workspace);
    let style = Style::new().fg(Color::Black).bg(WARN).add_modifier(Modifier::BOLD);
    frame.render_widget(Paragraph::new(Line::styled(text, style)), area);
}

fn header(frame: &mut Frame, area: Rect, st: &State) {
    let block = panel("agent");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let status = &st.status;
    let badge = if status.busy() {
        Span::styled("● occupé", Style::new().fg(WARN).add_modifier(Modifier::BOLD))
    } else {
        Span::styled("● au repos", Style::new().fg(OK))
    };
    let session = status.session.as_deref().map(short_session).unwrap_or_else(|| "—".to_string());
    let lines = vec![
        Line::from(vec![
            Span::styled(
                format!("swarm-{}", st.info.agent),
                Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            dim("  ·  "),
            Span::styled(st.info.runner.clone(), Style::new().fg(Color::White)),
            dim("  ·  "),
            Span::styled(
                format!("modèle {}", st.model.as_deref().unwrap_or("—")),
                Style::new().fg(Color::White),
            ),
            dim("  ·  "),
            Span::styled(format!("instance {}", st.info.instance), Style::new().fg(Color::White)),
            dim("  ·  "),
            badge,
        ]),
        Line::from(vec![
            dim("session  "),
            Span::styled(session, Style::new().fg(Color::White)),
            dim("  ·  tours  "),
            Span::styled(status.turns_done.to_string(), Style::new().fg(Color::White)),
            dim("  ·  file  "),
            Span::styled(status.queue.to_string(), Style::new().fg(Color::White)),
        ]),
        Line::from(vec![
            dim("MCP  "),
            Span::styled(
                status.url.clone().unwrap_or_else(|| "—".into()),
                Style::new().fg(Color::Gray),
            ),
            dim("  ·  pont  "),
            Span::styled(
                st.info.bridge.clone().unwrap_or_else(|| "—".into()),
                Style::new().fg(Color::Gray),
            ),
        ]),
    ];
    frame.render_widget(Paragraph::new(lines), inner);
}

fn peers(frame: &mut Frame, area: Rect, st: &State) {
    // Le titre donne d'emblée combien de pairs résidents sont vivants.
    let vivants = st.peers.iter().filter(|p| p.alive).count();
    let block = panel(&format!("serveurs MCP ({}) · {vivants} vivants", st.peers.len()));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if st.peers.is_empty() {
        let lines = vec![
            Line::styled("aucun pair annoncé", Style::new().fg(Color::Gray)),
            Line::from(""),
            Line::styled(
                "les serveurs swarm-* vivants du registre LiteMCP apparaissent ici",
                Style::new().fg(DIM),
            ),
        ];
        frame.render_widget(Paragraph::new(lines), inner);
        return;
    }
    let lines: Vec<Line> = st.peers.iter().map(peer_line).collect();
    frame.render_widget(Paragraph::new(lines), inner);
}

/// Une ligne du panneau serveurs : `●` répond, `▲` vivant mais en erreur, `○` mort.
fn peer_line(peer: &PeerRow) -> Line<'static> {
    let (dot, dot_color) = match peer.state.as_str() {
        "ok" => ("● ", OK),
        "inactif" => ("○ ", BAD),
        _ => ("▲ ", WARN),
    };
    let mut spans = vec![Span::styled(dot, Style::new().fg(dot_color))];
    if peer.own {
        spans.push(Span::styled(
            peer.name.clone(),
            Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(" (vous)", Style::new().fg(ACCENT)));
    } else {
        spans.push(Span::styled(peer.name.clone(), Style::new().fg(Color::White)));
    }
    spans.push(dim(format!("  {}", peer.addr)));
    if peer.state != "ok" {
        spans.push(Span::styled(
            format!("  {} · {}", peer.state, peer.detail),
            Style::new().fg(if peer.alive { WARN } else { DIM }),
        ));
    } else if !peer.detail.is_empty() {
        spans.push(dim(format!("  {}", peer.detail)));
    }
    Line::from(spans)
}

fn conversation(frame: &mut Frame, area: Rect, st: &mut State) {
    let block = panel(&format!("conversation ({})", st.conversation.len()));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height == 0 || inner.width == 0 {
        return;
    }
    if st.conversation.is_empty() {
        let lines = vec![
            Line::styled("le journal est vide", Style::new().fg(Color::Gray)),
            Line::from(""),
            Line::styled(
                "écrivez un message puis Entrée : il part en « ask » et la réponse s'affiche ici",
                Style::new().fg(DIM),
            ),
        ];
        frame.render_widget(Paragraph::new(lines), inner);
        return;
    }
    let now = crate::store::now_ms();
    let lines = conversation_lines(st.conversation.entries(), inner.width, now);
    let viewport = inner.height as usize;
    let total = lines.len();
    st.conversation.set_max_offset(total.saturating_sub(viewport));
    let offset = st.conversation.offset();
    let top = total.saturating_sub(viewport + offset);
    let end = (top + viewport).min(total);
    frame.render_widget(Paragraph::new(Text::from(lines[top..end].to_vec())), inner);
}

/// Lignes de la conversation : un titre par tour, le corps replié à la largeur.
fn conversation_lines(entries: &[Entry], width: u16, now: u64) -> Vec<Line<'static>> {
    let width = (width as usize).max(8);
    let body = width.saturating_sub(2);
    let mut lines = Vec::new();
    for entry in entries {
        let mut head =
            vec![Span::styled("▌ demande", Style::new().fg(ACCENT).add_modifier(Modifier::BOLD))];
        if let Some(from) = &entry.from {
            head.push(dim(format!("  de {from}")));
        }
        head.push(dim(format!("  {}", age_label(now, entry.ts))));
        head.push(dim(format!("  {}", duration_label(entry.duration_ms))));
        lines.push(Line::from(head));
        push_body(&mut lines, &entry.prompt, body, None);
        match &entry.error {
            Some(error) => {
                lines.push(Line::from(Span::styled(
                    "▌ erreur",
                    Style::new().fg(BAD).add_modifier(Modifier::BOLD),
                )));
                push_body(&mut lines, error, body, Some(Style::new().fg(BAD)));
            }
            None => {
                lines.push(Line::from(Span::styled(
                    "▌ réponse",
                    Style::new().fg(OK).add_modifier(Modifier::BOLD),
                )));
                push_body(&mut lines, &entry.reply, body, None);
            }
        }
        lines.push(Line::from(""));
    }
    lines
}

fn push_body(lines: &mut Vec<Line<'static>>, text: &str, width: usize, style: Option<Style>) {
    if text.trim().is_empty() {
        return;
    }
    let style = style.unwrap_or_default();
    for line in wrap(text, width) {
        lines.push(Line::from(Span::styled(format!("  {line}"), style)));
    }
}

fn turn_panel(frame: &mut Frame, area: Rect, st: &State) {
    let block = panel("file et tour en cours");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let status = &st.status;
    let mut lines = Vec::new();
    match &status.current {
        Some(current) => {
            let from = current.from.as_deref().unwrap_or("—");
            let run = current.run_id.as_deref().unwrap_or("—");
            lines.push(Line::from(vec![
                Span::styled("● tour en cours", Style::new().fg(WARN).add_modifier(Modifier::BOLD)),
                dim(format!(
                    "  de {from}  ·  run {run}  ·  {}",
                    duration_label(current.elapsed_ms)
                )),
            ]));
        }
        None => lines.push(Line::from(vec![
            Span::styled("○ aucun tour", Style::new().fg(DIM)),
            dim("  ·  l'agent attend une demande"),
        ])),
    }
    let queue = if status.queue == 0 {
        "file vide".to_string()
    } else {
        format!("file : {} en attente", status.queue)
    };
    lines.push(Line::from(vec![
        dim("file  "),
        Span::styled(queue, Style::new().fg(Color::White)),
        dim("  ·  tours terminés  "),
        Span::styled(status.turns_done.to_string(), Style::new().fg(Color::White)),
    ]));
    lines.push(last_turn_line(status, crate::store::now_ms()));
    frame.render_widget(Paragraph::new(lines), inner);
}

fn last_turn_line(status: &Status, now: u64) -> Line<'static> {
    match &status.last {
        Some(last) if last.ok => Line::from(vec![
            Span::styled("✓ dernier tour", Style::new().fg(OK)),
            dim(format!("  {}  ·  {}", duration_label(last.duration_ms), age_of(last.at, now))),
        ]),
        Some(last) => {
            let error = last.error.as_deref().unwrap_or("erreur sans message");
            Line::from(vec![
                Span::styled("✗ dernier tour", Style::new().fg(BAD)),
                dim(format!("  {}", duration_label(last.duration_ms))),
                Span::styled(format!("  ·  {error}"), Style::new().fg(BAD)),
            ])
        }
        None => Line::from(Span::styled("aucun tour terminé", Style::new().fg(DIM))),
    }
}

fn age_of(at: Option<u64>, now: u64) -> String {
    at.map(|at| age_label(now, at)).unwrap_or_else(|| "—".to_string())
}

fn logs_panel(frame: &mut Frame, area: Rect, st: &State) {
    let block = panel("logs · sortie brute du dernier tour (d)");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height == 0 || inner.width == 0 {
        return;
    }
    if st.logs.trim().is_empty() {
        frame.render_widget(
            Paragraph::new(Line::styled(
                "aucune sortie : le prochain tour en laissera une ici",
                Style::new().fg(DIM),
            )),
            inner,
        );
        return;
    }
    let lines: Vec<Line> = wrap(&st.logs, inner.width.max(8) as usize)
        .into_iter()
        .map(|line| Line::styled(line, Style::new().fg(Color::Gray)))
        .collect();
    let start = lines.len().saturating_sub(inner.height as usize);
    frame.render_widget(Paragraph::new(Text::from(lines[start..].to_vec())), inner);
}

fn notice_line(frame: &mut Frame, area: Rect, st: &State) {
    let line = match &st.notice {
        Some(notice) => {
            Line::styled(format!(" {}", notice.text), Style::new().fg(level_color(notice.level)))
        }
        None => Line::styled(
            " Entrée envoie · r reset session · s stop du tour · d logs · q quitter",
            Style::new().fg(DIM),
        ),
    };
    frame.render_widget(Paragraph::new(line), area);
}

fn level_color(level: Level) -> Color {
    match level {
        Level::Info => Color::Gray,
        Level::Ok => OK,
        Level::Warn => WARN,
        Level::Error => BAD,
    }
}

fn input_line(frame: &mut Frame, area: Rect, st: &mut State) {
    let block = panel("message").title_bottom(
        Line::styled(" Entrée = ask · Échap efface ", Style::new().fg(DIM)).right_aligned(),
    );
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let marker = Span::styled("› ", Style::new().fg(ACCENT).add_modifier(Modifier::BOLD));
    let avail = inner.width.saturating_sub(3) as usize;
    let len = st.input.chars().count();
    let skip = len.saturating_sub(avail);
    let line = if st.input.is_empty() {
        Line::from(vec![marker, Span::styled("écrivez un message…", Style::new().fg(DIM))])
    } else {
        let shown: String = st.input.chars().skip(skip).collect();
        Line::from(vec![marker, Span::styled(shown, Style::new().fg(Color::White))])
    };
    frame.render_widget(Paragraph::new(line), inner);
    let x = (inner.x + 2 + (len - skip) as u16).min(inner.right().saturating_sub(1));
    frame.set_cursor_position(Position::new(x, inner.y));
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn entry(ts: u64, prompt: &str, reply: &str) -> Entry {
        Entry { ts, prompt: prompt.into(), reply: reply.into(), ..Default::default() }
    }

    #[test]
    fn la_conversation_replie_et_titre_chaque_tour() {
        let lines = conversation_lines(&[entry(1, "bonjour", "salut")], 40, 2);
        let text: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
        assert!(text[0].contains("demande"));
        assert!(text.iter().any(|l| l.contains("bonjour")));
        assert!(text.iter().any(|l| l.contains("réponse")));
        assert!(text.iter().any(|l| l.contains("salut")));
        for line in &text {
            assert!(line.chars().count() <= 40, "ligne trop large : {line:?}");
        }
    }

    #[test]
    fn une_erreur_est_affichee_en_rouge() {
        let mut e = entry(1, "question", "");
        e.error = Some("boum".into());
        let lines = conversation_lines(&[e], 40, 2);
        let error = lines.iter().find(|l| l.to_string().contains("erreur")).unwrap();
        assert_eq!(error.spans[0].style.fg, Some(BAD));
        assert!(lines.iter().any(|l| l.to_string().contains("boum")));
    }

    #[test]
    fn la_ligne_de_pair_marque_le_soimeme_et_les_morts() {
        let vous = PeerRow {
            name: "swarm-alice".into(),
            addr: "127.0.0.1:1".into(),
            alive: true,
            own: true,
            state: "ok".into(),
            detail: String::new(),
        };
        let line = peer_line(&vous).to_string();
        assert!(line.contains("(vous)"), "ligne : {line}");
        let mort = PeerRow {
            name: "swarm-bob".into(),
            addr: "127.0.0.1:2".into(),
            alive: false,
            own: false,
            state: "inactif".into(),
            detail: "PID 7 mort".into(),
        };
        let line = peer_line(&mort).to_string();
        assert!(line.contains("inactif") && line.contains("PID 7 mort"), "ligne : {line}");
    }

    #[test]
    fn un_rendu_de_bout_en_bout_ne_panique_pas() {
        use crate::tui::info::Identity;
        use crate::tui::state::State;
        use std::time::Instant;

        let info = Identity {
            agent: "alice".into(),
            runner: "claude".into(),
            instance: "swarm-control-1".into(),
            workspace: "ws".into(),
            bridge: Some("http://127.0.0.1:3001".into()),
        };
        let mut st = State::new(info, false, Instant::now());
        st.apply_status(&serde_json::json!({
            "state": "busy",
            "queueLength": 1,
            "current": {"from": "tui", "runId": "r1", "elapsedMs": 1200},
            "turnsDone": 3,
            "lastTurn": {"ok": true, "durationMs": 900, "at": 1},
            "url": "http://127.0.0.1:23082/mcp",
        }));
        st.apply_history(vec![entry(1, "bonjour", "salut"), entry(2, "encore", "oui")]);
        st.type_str("un message");
        let mut terminal = Terminal::new(TestBackend::new(90, 32)).unwrap();
        terminal.draw(|frame| draw(frame, &mut st)).unwrap();
        st.toggle_logs();
        terminal.draw(|frame| draw(frame, &mut st)).unwrap();
        // Écran minuscule : un message, pas de panique.
        let mut small = Terminal::new(TestBackend::new(20, 8)).unwrap();
        small.draw(|frame| draw(frame, &mut st)).unwrap();
    }
}
