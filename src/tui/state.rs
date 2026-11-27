//! État de la TUI : saisie, conversation, statut, pairs, horloges.
//!
//! Tout ce qui décide **ce qui s'affiche** vit ici, sans terminal : `ui` ne fait que rendre,
//! `input` ne fait que traduire des touches, `mod` ne fait que le pont avec [`Ctx`].
//!
//! Trois règles de conception :
//! - la saisie ne perd jamais un caractère (le curseur est toujours en fin de ligne) ;
//! - le défilement est borné à ce qui a été rendu au dernier tour ([`Conversation::set_max_offset`]),
//!   donc il ne peut pas décrocher au-delà du contenu ;
//! - quand la vue suit la fin (décalage 0), un nouveau tour la laisse en bas ; sinon elle garde
//!   son décalage.
//!
//! [`Ctx`]: crate::residents::Ctx

use crate::history::Entry;
use crate::peers::Peer;
use serde_json::Value;
use std::time::{Duration, Instant};

/// Cadence du statut et de la surveillance du journal.
pub const STATUS_EVERY: Duration = Duration::from_millis(200);
/// Cadence de la découverte des pairs (un `tasklist` par PID inconnu sous Windows).
pub const PEERS_EVERY: Duration = Duration::from_secs(5);
/// Tours demandés au journal à chaque relecture.
pub const HISTORY_LIMIT: usize = 120;
/// Tours gardés en mémoire au-delà desquels les plus anciens sont oubliés.
const MAX_ENTRIES: usize = 120;
/// Budget de caractères de la conversation (une réponse démesurée ne mange pas la RAM).
const MAX_CHARS: usize = 150_000;

/// Gravité d'une ligne d'état.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Ok,
    Warn,
    Error,
}

/// Dernière ligne d'état (retour d'une action, erreur de saisie).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub level: Level,
    pub text: String,
}

impl Notice {
    pub fn new(level: Level, text: impl Into<String>) -> Self {
        Notice { level, text: text.into() }
    }
}

/// Tour en cours, tel que `status_json` le publie.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CurrentTurn {
    pub from: Option<String>,
    pub run_id: Option<String>,
    pub elapsed_ms: u64,
}

/// Dernier tour terminé, tel que `status_json` le publie.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LastTurn {
    pub ok: bool,
    pub error: Option<String>,
    pub duration_ms: u64,
    pub at: Option<u64>,
}

/// Vue typée de `ctx.status_json()`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub state: String,
    pub queue: usize,
    pub current: Option<CurrentTurn>,
    pub session: Option<String>,
    pub turns_done: u64,
    pub last: Option<LastTurn>,
    pub url: Option<String>,
}

impl Status {
    pub fn from_json(doc: &Value) -> Status {
        let text = |key: &str| doc.get(key).and_then(Value::as_str).map(str::to_string);
        let number = |key: &str| doc.get(key).and_then(Value::as_u64).unwrap_or(0);
        let current = doc.get("current").filter(|c| !c.is_null()).map(|c| CurrentTurn {
            from: c.get("from").and_then(Value::as_str).map(str::to_string),
            run_id: c.get("runId").and_then(Value::as_str).map(str::to_string),
            elapsed_ms: c.get("elapsedMs").and_then(Value::as_u64).unwrap_or(0),
        });
        let last = doc.get("lastTurn").filter(|l| !l.is_null()).map(|l| LastTurn {
            ok: l.get("ok").and_then(Value::as_bool).unwrap_or(false),
            error: l.get("error").and_then(Value::as_str).map(str::to_string),
            duration_ms: l.get("durationMs").and_then(Value::as_u64).unwrap_or(0),
            at: l.get("at").and_then(Value::as_u64),
        });
        Status {
            state: text("state").unwrap_or_else(|| "?".to_string()),
            queue: number("queueLength") as usize,
            current,
            session: text("session"),
            turns_done: number("turnsDone"),
            last,
            url: text("url"),
        }
    }

    /// Un tour tourne-t-il ?
    pub fn busy(&self) -> bool {
        self.current.is_some()
    }
}

/// Un pair annoncé, prêt à afficher.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerRow {
    pub name: String,
    pub addr: String,
    pub alive: bool,
    /// Est-ce l'agent que cette TUI pilote ?
    pub own: bool,
}

/// Horloges des rafraîchissements ; `due` dit ce qui doit être relu maintenant.
#[derive(Clone, Copy, Debug)]
pub struct Timers {
    status: Instant,
    history: Instant,
    peers: Instant,
}

/// Ce qui doit être rafraîchi à cet instant.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Due {
    pub status: bool,
    pub history: bool,
    pub peers: bool,
}

impl Timers {
    pub fn new(now: Instant) -> Timers {
        // Les horloges partent dans le passé : le premier `due` rafraîchit tout, sans attendre
        // un premier tour d'horloge (la première image montre déjà l'état réel).
        Timers {
            status: now.checked_sub(STATUS_EVERY).unwrap_or(now),
            history: now.checked_sub(STATUS_EVERY).unwrap_or(now),
            peers: now.checked_sub(PEERS_EVERY).unwrap_or(now),
        }
    }

    pub fn due(&mut self, now: Instant) -> Due {
        let due = Due {
            status: now.saturating_duration_since(self.status) >= STATUS_EVERY,
            history: now.saturating_duration_since(self.history) >= STATUS_EVERY,
            peers: now.saturating_duration_since(self.peers) >= PEERS_EVERY,
        };
        if due.status {
            self.status = now;
        }
        if due.history {
            self.history = now;
        }
        if due.peers {
            self.peers = now;
        }
        due
    }
}

/// Conversation issue du journal, avec un défilement borné.
#[derive(Clone, Debug, Default)]
pub struct Conversation {
    entries: Vec<Entry>,
    /// Lignes entre le bas du contenu et le bas de la vue ; 0 = on suit la fin.
    offset: usize,
    /// Borne haute calculée au dernier rendu (le nombre de lignes dépend de la largeur).
    max_offset: usize,
}

impl Conversation {
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    /// La vue suit-elle la fin de la conversation ?
    pub fn following(&self) -> bool {
        self.offset == 0
    }

    /// Appelé par le rendu, qui seul connaît la largeur et la hauteur disponibles.
    pub fn set_max_offset(&mut self, max: usize) {
        self.max_offset = max;
        self.offset = self.offset.min(max);
    }

    pub fn scroll_up(&mut self, lines: usize) {
        self.offset = self.offset.saturating_add(lines).min(self.max_offset);
    }

    pub fn scroll_down(&mut self, lines: usize) {
        self.offset = self.offset.saturating_sub(lines);
    }

    pub fn to_top(&mut self) {
        self.offset = self.max_offset;
    }

    pub fn to_bottom(&mut self) {
        self.offset = 0;
    }

    /// Remplace la fenêtre du journal par celle qui vient d'être relue.
    ///
    /// La fenêtre de `history::read` est contiguë et finit au dernier tour : on garde les tours
    /// plus anciens déjà affichés (ils sortent de la fenêtre quand elle rétrécit), puis on
    /// ajoute la fenêtre telle quelle — aucun doublon possible.
    pub fn refresh(&mut self, entries: Vec<Entry>) {
        let follow = self.following();
        let oldest = entries.first().map(|e| e.ts);
        let mut merged: Vec<Entry> = match oldest {
            Some(ts) => self.entries.iter().filter(|e| e.ts < ts).cloned().collect(),
            None => Vec::new(),
        };
        merged.extend(entries);
        self.entries = trim(merged);
        if follow {
            self.offset = 0;
        }
        self.offset = self.offset.min(self.max_offset);
    }
}

fn entry_chars(entry: &Entry) -> usize {
    let error = entry.error.as_deref().map_or(0, |e| e.chars().count());
    entry.prompt.chars().count() + entry.reply.chars().count() + error
}

/// Garde les tours les plus récents dans les bornes de mémoire.
fn trim(mut entries: Vec<Entry>) -> Vec<Entry> {
    let mut chars: usize = entries.iter().map(entry_chars).sum();
    let mut drop = 0;
    while entries.len() - drop > 1 && (entries.len() - drop > MAX_ENTRIES || chars > MAX_CHARS) {
        chars -= entry_chars(&entries[drop]);
        drop += 1;
    }
    if drop > 0 {
        entries.drain(..drop);
    }
    entries
}

/// Tout l'état de la TUI.
#[derive(Clone, Debug)]
pub struct State {
    pub info: super::info::Identity,
    pub debug: bool,
    pub show_logs: bool,
    pub input: String,
    pub notice: Option<Notice>,
    pub status: Status,
    pub conversation: Conversation,
    pub peers: Vec<PeerRow>,
    pub logs: String,
    /// `(taille, mtime)` du journal lors de la dernière relecture ; `None` = jamais lu.
    pub fingerprint: Option<(u64, u64)>,
    pub timers: Timers,
}

impl State {
    pub fn new(info: super::info::Identity, debug: bool, now: Instant) -> State {
        State {
            info,
            debug,
            show_logs: debug,
            input: String::new(),
            notice: None,
            status: Status::default(),
            conversation: Conversation::default(),
            peers: Vec::new(),
            logs: String::new(),
            fingerprint: None,
            timers: Timers::new(now),
        }
    }

    pub fn input_empty(&self) -> bool {
        self.input.is_empty()
    }

    pub fn type_char(&mut self, c: char) {
        self.input.push(c);
    }

    /// Raccourci de test : ajoute un texte d'un coup.
    #[cfg(test)]
    pub fn type_str(&mut self, text: &str) {
        self.input.push_str(text);
    }

    pub fn backspace(&mut self) {
        self.input.pop();
    }

    pub fn clear_input(&mut self) {
        self.input.clear();
    }

    pub fn set_input(&mut self, text: String) {
        self.input = text;
    }

    /// Ce qu'Entrée envoie : la saisie détourée, ou rien si elle est vide.
    ///
    /// L'espace de tête sert à taper un message qui commence par `r`, `s`, `d` ou `q` : une
    /// fois la ligne non vide, ces lettres sont du texte. Elle est retirée ici.
    pub fn take_prompt(&mut self) -> Option<String> {
        let prompt = self.input.trim().to_string();
        self.input.clear();
        if prompt.is_empty() { None } else { Some(prompt) }
    }

    pub fn say(&mut self, level: Level, text: impl Into<String>) {
        self.notice = Some(Notice::new(level, text));
    }

    pub fn toggle_logs(&mut self) {
        self.show_logs = !self.show_logs;
        let text = if self.show_logs {
            "panneau logs affiché (d pour le masquer)"
        } else {
            "panneau logs masqué"
        };
        self.say(Level::Info, text);
    }

    pub fn apply_status(&mut self, doc: &Value) {
        self.status = Status::from_json(doc);
    }

    pub fn apply_logs(&mut self, text: String) {
        self.logs = text;
    }

    pub fn apply_history(&mut self, entries: Vec<Entry>) {
        self.conversation.refresh(entries);
    }

    pub fn apply_peers(&mut self, peers: Vec<Peer>) {
        self.peers = peers
            .into_iter()
            .map(|p| PeerRow {
                own: p.agent == self.info.agent,
                addr: p.url.trim_start_matches("http://").trim_end_matches("/mcp").to_string(),
                name: p.agent,
                alive: p.alive,
            })
            .collect();
    }

    pub fn scroll_up(&mut self, lines: usize) {
        self.conversation.scroll_up(lines);
    }

    pub fn scroll_down(&mut self, lines: usize) {
        self.conversation.scroll_down(lines);
    }

    pub fn to_top(&mut self) {
        self.conversation.to_top();
    }

    pub fn to_bottom(&mut self) {
        self.conversation.to_bottom();
    }
}

/// Découpe un texte en lignes d'au plus `width` caractères, en coupant aux espaces.
///
/// Les espaces internes sont conservés (sauf celui qui sert de point de coupe, et les espaces
/// de fin de ligne) et un mot plus long que la largeur est coupé net : aucune ligne ne dépasse,
/// donc le défilement en lignes reste exact.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    for raw in text.split('\n') {
        let mut rest: Vec<char> = raw.chars().collect();
        while rest.len() > width {
            // Si le caractère qui suit la fenêtre est une espace, la fenêtre est pleine ; sinon
            // on coupe à la dernière espace de la fenêtre, ou net si le mot est trop long.
            let at = if rest[width] == ' ' {
                width
            } else {
                rest[..width].iter().rposition(|c| *c == ' ').filter(|i| *i > 0).unwrap_or(width)
            };
            let line: String = rest.drain(..at).collect();
            lines.push(line.trim_end().to_string());
            if rest.first() == Some(&' ') {
                rest.drain(..1);
            }
        }
        lines.push(rest.into_iter().collect::<String>().trim_end().to_string());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::info::Identity;
    use serde_json::json;

    fn identity() -> Identity {
        Identity {
            agent: "alice".into(),
            runner: "claude".into(),
            instance: "swarm-control-1".into(),
            workspace: "ws".into(),
            bridge: None,
        }
    }

    fn state() -> State {
        State::new(identity(), false, Instant::now())
    }

    fn entry(ts: u64, prompt: &str) -> Entry {
        Entry { ts, prompt: prompt.into(), reply: format!("écho {prompt}"), ..Default::default() }
    }

    #[test]
    fn frappe_et_efface_des_caracteres() {
        let mut st = state();
        st.type_str("bonjour");
        st.type_char('!');
        assert_eq!(st.input, "bonjour!");
        st.backspace();
        assert_eq!(st.input, "bonjour");
        while !st.input.is_empty() {
            st.backspace();
        }
        st.backspace();
        assert_eq!(st.input, "");
        assert!(st.input_empty());
    }

    #[test]
    fn entree_construit_le_prompt_et_vide_la_ligne() {
        let mut st = state();
        st.type_str("  regarde le journal  ");
        assert_eq!(st.take_prompt().as_deref(), Some("regarde le journal"));
        assert!(st.input.is_empty());
        assert_eq!(st.take_prompt(), None);
        st.type_str("   ");
        assert_eq!(st.take_prompt(), None);
        assert!(st.input.is_empty());
    }

    #[test]
    fn une_espace_de_tete_permet_de_commencer_par_un_raccourci() {
        let mut st = state();
        // Ligne vide : la touche est un raccourci, pas du texte (cf. `input`).
        st.type_char(' ');
        assert!(!st.input_empty());
        st.type_str("ranger");
        assert_eq!(st.take_prompt().as_deref(), Some("ranger"));
    }

    #[test]
    fn defilement_borne_par_ce_qui_est_affiche() {
        let mut c = Conversation::default();
        c.refresh(vec![entry(1, "un"), entry(2, "deux")]);
        c.set_max_offset(10);
        c.scroll_up(3);
        assert_eq!(c.offset(), 3);
        c.scroll_up(100);
        assert_eq!(c.offset(), 10);
        assert!(!c.following());
        c.scroll_down(4);
        assert_eq!(c.offset(), 6);
        c.scroll_down(100);
        assert_eq!(c.offset(), 0);
        assert!(c.following());
        c.scroll_up(2);
        c.to_top();
        assert_eq!(c.offset(), 10);
        c.to_bottom();
        assert_eq!(c.offset(), 0);
        // Un contenu plus court que la vue ne laisse aucun décalage possible.
        c.set_max_offset(0);
        c.scroll_up(5);
        assert_eq!(c.offset(), 0);
    }

    #[test]
    fn fusionne_le_journal_sans_doublon() {
        let mut c = Conversation::default();
        c.refresh(vec![entry(1, "un"), entry(2, "deux")]);
        assert_eq!(c.len(), 2);
        // La fenêtre relue chevauche : le tour 2 ne doit pas apparaître deux fois.
        c.refresh(vec![entry(2, "deux"), entry(3, "trois")]);
        let ts: Vec<u64> = c.entries().iter().map(|e| e.ts).collect();
        assert_eq!(ts, [1, 2, 3]);
        assert_eq!(c.entries()[2].prompt, "trois");
        // Journal disparu : on n'affiche pas du contenu fantôme.
        c.refresh(Vec::new());
        assert!(c.is_empty());
    }

    #[test]
    fn la_vue_reste_en_bas_si_elle_suivait_et_garde_sinon_son_decalage() {
        let mut c = Conversation::default();
        c.refresh(vec![entry(1, "un")]);
        c.set_max_offset(20);
        c.refresh(vec![entry(1, "un"), entry(2, "deux")]);
        assert_eq!(c.offset(), 0, "en bas, on reste en bas");
        c.scroll_up(4);
        c.refresh(vec![entry(1, "un"), entry(2, "deux"), entry(3, "trois")]);
        assert_eq!(c.offset(), 4, "en lecture, on garde son décalage");
    }

    #[test]
    fn lit_le_statut_et_le_dernier_tour() {
        let doc = json!({
            "agent": "alice",
            "state": "busy",
            "queueLength": 2,
            "current": {"from": "tui", "runId": "run-1", "elapsedMs": 1500},
            "session": "3f2a9c1e-4b5d",
            "turnsDone": 7,
            "lastTurn": {"ok": false, "error": "boum", "durationMs": 420, "at": 99},
            "url": "http://127.0.0.1:23082/mcp",
        });
        let status = Status::from_json(&doc);
        assert!(status.busy());
        assert_eq!(status.queue, 2);
        assert_eq!(status.current.as_ref().unwrap().run_id.as_deref(), Some("run-1"));
        assert!(status.url.as_deref().unwrap().ends_with("/mcp"));
        let last = status.last.as_ref().unwrap();
        assert_eq!((last.ok, last.error.as_deref()), (false, Some("boum")));

        let idle = Status::from_json(&json!({"state": "idle", "current": null, "lastTurn": null}));
        assert!(!idle.busy());
        assert_eq!((idle.queue, idle.turns_done), (0, 0));
        assert!(idle.last.is_none());
    }

    #[test]
    fn les_horloges_ne_sonnent_qu_a_echeance() {
        let t0 = Instant::now();
        let mut timers = Timers::new(t0);
        assert_eq!(timers.due(t0), Due { status: true, history: true, peers: true });
        let soon = t0 + Duration::from_millis(199);
        assert_eq!(timers.due(soon), Due::default());
        let after = t0 + STATUS_EVERY;
        let due = timers.due(after);
        assert!(due.status && due.history && !due.peers);
        let later = t0 + PEERS_EVERY;
        assert!(timers.due(later).peers);
    }

    #[test]
    fn les_pairs_marquent_l_agent_courant() {
        let mut st = state();
        st.apply_peers(vec![
            Peer {
                name: "swarm-alice".into(),
                agent: "alice".into(),
                url: "http://127.0.0.1:1/mcp".into(),
                pid: 1,
                alive: true,
            },
            Peer {
                name: "swarm-bob".into(),
                agent: "bob".into(),
                url: "http://127.0.0.1:2/mcp".into(),
                pid: 2,
                alive: false,
            },
        ]);
        assert_eq!(st.peers.len(), 2);
        assert!(st.peers[0].own && st.peers[0].alive);
        assert_eq!(st.peers[0].addr, "127.0.0.1:1");
        assert!(!st.peers[1].own && !st.peers[1].alive);
    }

    #[test]
    fn decoupe_le_texte_sans_depasser_la_largeur() {
        assert_eq!(wrap("un deux trois", 7), ["un deux", "trois"]);
        assert_eq!(wrap("un deux trois", 20), ["un deux trois"]);
        assert_eq!(wrap("aaaaaaaaaa", 4), ["aaaa", "aaaa", "aa"]);
        assert_eq!(wrap("a\nb", 10), ["a", "b"]);
        assert_eq!(wrap("", 10), [""]);
        assert_eq!(wrap("mot", 0), ["m", "o", "t"]);
        for line in wrap("un  deux   trois quatre", 9) {
            assert!(line.chars().count() <= 9, "ligne trop longue : {line:?}");
        }
    }

    #[test]
    fn les_tours_anciens_sortent_au_dela_du_budget() {
        let entries = (0..MAX_ENTRIES as u64 + 5).map(|i| entry(i, "x")).collect();
        let trimmed = trim(entries);
        assert_eq!(trimmed.len(), MAX_ENTRIES);
        assert_eq!(trimmed.last().unwrap().ts, MAX_ENTRIES as u64 + 4);
    }
}
