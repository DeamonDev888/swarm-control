//! Tâches planifiées d'un agent résident (PLAN §4.4) : `--every 5m "<prompt>"` ou
//! `schedule` dans la définition de l'agent.
//!
//! Les tours planifiés vont dans la **même file** que les demandes (jamais deux tours en
//! même temps), marqués `from: "schedule"` au journal. Un tour manqué (agent arrêté) n'est
//! pas rejoué deux fois : au redémarrage, la prochaine échéance est recalculée.
//!
//! Le planificateur est un fil à part ([`spawn`]). Chaque tour est attendu un intervalle
//! entier depuis la **fin** du tour précédent, jamais depuis une échéance manquée : une
//! longue veille de la machine ne rattrape rien, un tour manqué ne part qu'une fois. La
//! fenêtre horaire est vérifiée au moment de déposer le tour (fermée → report à
//! l'ouverture) ; la fin du tour est attendue avant de recompter. Chaque attente sonde
//! `Ctx::stopping` : l'arrêt de l'agent arrête le fil.

use super::ctx::Ctx;
use super::turn::{Turn, TurnOutcome};
use crate::agents::Runner;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::thread::JoinHandle;
use std::time::Duration;

/// Origine journalisée d'un tour planifié (`from`).
pub const ORIGIN: &str = "schedule";

/// Granularité de la sonde d'arrêt pendant les attentes du planificateur.
const STOP_POLL: Duration = Duration::from_millis(250);

/// Minutes dans une journée.
const DAY: u16 = 24 * 60;

/// Fenêtre horaire **locale** : ouverte de `start` (inclus) à `end` (exclus). `start > end`
/// décrit une fenêtre à cheval sur minuit (`22:00-06:00`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Window {
    pub start: (u8, u8),
    pub end: (u8, u8),
}

impl Window {
    /// Une heure `HH:MM` en minutes depuis minuit.
    fn minutes(hm: (u8, u8)) -> u16 {
        u16::from(hm.0) * 60 + u16::from(hm.1)
    }

    /// L'heure locale `now` (minutes depuis minuit) est-elle dans la fenêtre ?
    pub fn contains(self, now: u16) -> bool {
        let (start, end) = (Self::minutes(self.start), Self::minutes(self.end));
        if start < end { (start..end).contains(&now) } else { now >= start || now < end }
    }

    /// Minutes avant la prochaine ouverture ; `0` si la fenêtre est ouverte.
    pub fn minutes_until_open(self, now: u16) -> u16 {
        if self.contains(now) {
            return 0;
        }
        (Self::minutes(self.start) + DAY - now) % DAY
    }
}

/// Une tâche planifiée : un prompt toutes les `every`, dans `window` si elle est posée.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Schedule {
    pub every: Duration,
    pub prompt: String,
    pub window: Option<Window>,
}

impl Schedule {
    /// Depuis la ligne de commande : `--every`, prompt positionnel de `serve`, `--window`.
    pub fn parse(every: &str, prompt: String, window: Option<&str>) -> Result<Self, String> {
        if prompt.trim().is_empty() {
            return Err("tâche planifiée : prompt vide (--every 5m \"<prompt>\").".into());
        }
        Ok(Schedule {
            every: parse_duration(every)?,
            prompt,
            window: window.map(parse_window).transpose()?,
        })
    }
}

/// Un intervalle : `30s`, `5m`, `1h`, `1h30`, `90m`, `2h`. Plusieurs composants `s`/`m`/`h`
/// sont admis (`1h30m`) ; un nombre nu n'a de sens qu'après une heure (« 1h30 » = 1 h 30).
pub fn parse_duration(text: &str) -> Result<Duration, String> {
    let mut seconds: u64 = 0;
    let mut digits = String::new();
    let mut last_unit: Option<char> = None;
    for c in text.trim().chars() {
        match c {
            '0'..='9' => digits.push(c),
            's' | 'm' | 'h' | 'S' | 'M' | 'H' => {
                let unit = c.to_ascii_lowercase();
                seconds = add(seconds, scale(number(&digits, text)?, unit, text)?, text)?;
                digits.clear();
                last_unit = Some(unit);
            }
            c if c.is_whitespace() => {}
            _ => {
                return Err(format!(
                    "durée « {text} » : « {c} » inattendu (chiffres et unités s, m, h)."
                ));
            }
        }
    }
    if !digits.is_empty() {
        match last_unit {
            Some('h') => seconds = add(seconds, scale(number(&digits, text)?, 'm', text)?, text)?,
            _ => {
                return Err(format!(
                    "durée « {text} » : unité manquante après « {digits} » (s, m ou h)."
                ));
            }
        }
    } else if last_unit.is_none() {
        return Err(format!("durée « {text} » vide : écris par ex. 30s, 5m, 1h, 1h30 ou 2h."));
    }
    if seconds == 0 {
        return Err(format!("durée « {text} » : l'intervalle doit être strictement positif."));
    }
    Ok(Duration::from_secs(seconds))
}

/// Le nombre en cours de lecture (erreur claire s'il est absent ou déborde).
fn number(digits: &str, whole: &str) -> Result<u64, String> {
    if digits.is_empty() {
        return Err(format!("durée « {whole} » : nombre attendu avant l'unité."));
    }
    digits.parse().map_err(|_| format!("durée « {whole} » : « {digits} » est trop grand."))
}

/// `n` unités, en secondes.
fn scale(n: u64, unit: char, whole: &str) -> Result<u64, String> {
    let factor = match unit {
        'h' => 3600,
        'm' => 60,
        _ => 1,
    };
    n.checked_mul(factor).ok_or_else(|| format!("durée « {whole} » : trop grande."))
}

fn add(a: u64, b: u64, whole: &str) -> Result<u64, String> {
    a.checked_add(b).ok_or_else(|| format!("durée « {whole} » : trop grande."))
}

/// Une fenêtre horaire locale : `HH:MM-HH:MM` (`09:00-18:00`, ou `22:00-06:00` à cheval
/// sur minuit). Début et fin identiques sont refusés : une journée entière s'écrit sans
/// fenêtre.
pub fn parse_window(text: &str) -> Result<Window, String> {
    let text = text.trim();
    let Some((start, end)) = text.split_once('-') else {
        return Err(format!("fenêtre « {text} » : format HH:MM-HH:MM attendu (ex. 09:00-18:00)."));
    };
    let window =
        Window { start: parse_clock(start.trim(), text)?, end: parse_clock(end.trim(), text)? };
    if window.start == window.end {
        return Err(format!(
            "fenêtre « {text} » : début et fin identiques ; omettre la fenêtre pour tourner toute la journée."
        ));
    }
    Ok(window)
}

/// Une heure `HH:MM` (`24:00` et `09:60` sont refusés).
fn parse_clock(text: &str, whole: &str) -> Result<(u8, u8), String> {
    let invalid =
        || format!("fenêtre « {whole} » : heure « {text} » invalide (HH:MM, de 00:00 à 23:59).");
    let Some((hour, minute)) = text.split_once(':') else {
        return Err(invalid());
    };
    if ![hour, minute]
        .iter()
        .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(invalid());
    }
    let (Ok(hour), Ok(minute)) = (hour.parse::<u8>(), minute.parse::<u8>()) else {
        return Err(invalid());
    };
    if hour > 23 || minute > 59 {
        return Err(invalid());
    }
    Ok((hour, minute))
}

/// Le `schedule` de la définition de l'agent, si elle en porte un. Définition absente,
/// JSON illisible ou sans `schedule` → `Ok(None)` ; `schedule` mal formé → l'erreur dit quoi.
///
/// La définition est le JSON de configuration existant : `settings_<nom>.json` (Claude,
/// Antigravity) ou `swarm.json` du profil (Hermes). Formes admises :
/// `{ "every": "5m", "prompt": "…", "window": "09:00-18:00" }`, ou la chaîne simple
/// `"<durée> <prompt>"` (`"5m Vérifie tes mentions"`, sans fenêtre).
pub fn read_definition(ws: &Path, runner: Runner, agent: &str) -> Result<Option<Schedule>, String> {
    let Some(doc) = crate::store::read_json(&definition_path(ws, runner, agent)) else {
        return Ok(None);
    };
    match doc.get("schedule") {
        None | Some(Value::Null) => Ok(None),
        Some(field) => {
            from_field(field).map(Some).map_err(|e| format!("définition de '{agent}' : {e}"))
        }
    }
}

/// Comme [`read_definition`], sans erreur : un `schedule` illisible est ignoré (une ligne
/// au journal de diagnostic) et l'agent tourne sans tâche planifiée.
pub fn from_definition(ws: &Path, runner: Runner, agent: &str) -> Option<Schedule> {
    read_definition(ws, runner, agent).unwrap_or_else(|e| {
        crate::debug!("résidence {agent} : tâche planifiée ignorée · {e}");
        None
    })
}

/// Fichier de définition de l'agent pour ce runner (lecture seule).
fn definition_path(ws: &Path, runner: Runner, agent: &str) -> PathBuf {
    match runner {
        Runner::Claude => crate::agents::claude_settings_path(ws, agent),
        Runner::Antigravity => crate::agents::antigravity_settings_path(ws, agent),
        // Même chemin que `agents::paths::hermes_profile_dir` (privé) : `<home>/profiles/<nom>`.
        Runner::Hermes => {
            crate::config::native_hermes_home().join("profiles").join(agent).join("swarm.json")
        }
    }
}

/// Le champ `schedule` : objet `{ every, prompt, window? }` ou chaîne « <durée> <prompt> ».
fn from_field(field: &Value) -> Result<Schedule, String> {
    match field {
        Value::Object(map) => {
            let every = map
                .get("every")
                .and_then(Value::as_str)
                .ok_or("schedule.every : durée attendue (ex. \"5m\").")?;
            let prompt = map
                .get("prompt")
                .and_then(Value::as_str)
                .ok_or("schedule.prompt : texte de la demande attendu.")?;
            let window = match map.get("window") {
                None | Some(Value::Null) => None,
                Some(Value::String(w)) => Some(w.as_str()),
                Some(_) => return Err("schedule.window : fenêtre « HH:MM-HH:MM » attendue.".into()),
            };
            Schedule::parse(every, prompt.to_string(), window)
                .map_err(|e| format!("schedule : {e}"))
        }
        Value::String(text) => {
            let text = text.trim();
            let Some((every, prompt)) = text.split_once(char::is_whitespace) else {
                return Err(format!(
                    "schedule « {text} » : forme simple « <durée> <prompt> » attendue (ex. « 5m Vérifie tes mentions »)."
                ));
            };
            Schedule::parse(every, prompt.trim().to_string(), None)
                .map_err(|e| format!("schedule « {text} » : {e}"))
        }
        _ => Err(
            "schedule : objet { \"every\", \"prompt\", \"window\"? } ou chaîne « <durée> <prompt> » attendu."
                .into(),
        ),
    }
}

/// Lance le planificateur : un fil qui attend la prochaine échéance, vérifie la fenêtre,
/// dépose un tour `from: "schedule"` dans la file de l'agent ([`Ctx::submit`]), attend sa
/// fin, puis recompte. S'arrête dès que `Ctx::stopping` est sondé vrai (pendant chaque
/// attente) ; la file refuse alors le tour. La première échéance tombe un intervalle
/// entier après le démarrage.
pub fn spawn(ctx: Arc<Ctx>, schedule: Schedule) -> Result<JoinHandle<()>, String> {
    if schedule.every.is_zero() {
        return Err("tâche planifiée : l'intervalle doit être strictement positif.".into());
    }
    let agent = ctx.agent.clone();
    std::thread::Builder::new()
        .name(format!("swarm-sched-{agent}"))
        .spawn(move || run_loop(&ctx, &schedule, &mut RealClock))
        .map_err(|e| format!("thread du planificateur impossible: {e}"))
}

/// Boucle du planificateur, sur l'horloge `clock` (fausse dans les tests).
fn run_loop(ctx: &Ctx, schedule: &Schedule, clock: &mut dyn Clock) {
    crate::debug!("planificateur {} : démarré, toutes les {:?}", ctx.agent, schedule.every);
    while !ctx.stopping() {
        // Un intervalle entier depuis maintenant (la fin du tour précédent), jamais depuis
        // l'échéance précédente : pas de rafale après une longue veille.
        if !clock.sleep(ctx, schedule.every) {
            break;
        }
        match in_window(ctx, schedule, clock) {
            Verdict::Run => {}
            Verdict::Skip => continue,
            Verdict::Stop => break,
        }
        let (turn, outcome) = Turn::new(schedule.prompt.clone(), Some(ORIGIN.into()), None);
        if ctx.submit(turn).is_err() {
            break; // arrêt en cours : la file est fermée
        }
        // Un seul tour planifié à la fois : sa fin est attendue avant de recompter.
        await_turn(ctx, &outcome);
    }
}

/// Décision de la fenêtre pour un tour échu.
enum Verdict {
    /// La fenêtre est ouverte (ou absente) : le tour part.
    Run,
    /// Heure locale illisible : on ne suppose pas la fenêtre ouverte, ce tour est sauté.
    Skip,
    /// Arrêt sonné pendant le report.
    Stop,
}

/// Attend l'ouverture de la fenêtre : un tour échu est reporté (jamais annulé, et un seul
/// part à l'ouverture, quoi qu'il se soit passé pendant la nuit).
fn in_window(ctx: &Ctx, schedule: &Schedule, clock: &mut dyn Clock) -> Verdict {
    let Some(window) = schedule.window else {
        return Verdict::Run;
    };
    loop {
        match clock.local_minute() {
            Some(now) if window.contains(now) => return Verdict::Run,
            Some(now) => {
                let minutes = u64::from(window.minutes_until_open(now));
                crate::debug!(
                    "planificateur {} : fenêtre fermée, report dans {minutes} min",
                    ctx.agent
                );
                if !clock.sleep(ctx, Duration::from_secs(minutes * 60)) {
                    return Verdict::Stop;
                }
            }
            None => {
                crate::debug!(
                    "planificateur {} : heure locale illisible, tour de cette échéance sauté",
                    ctx.agent
                );
                return Verdict::Skip;
            }
        }
    }
}

/// Attend la fin du tour déposé ; ressort dès l'arrêt (le tour en cours est tué par
/// l'arrêt, on ne le retient pas).
fn await_turn(ctx: &Ctx, outcome: &Receiver<TurnOutcome>) {
    while !ctx.stopping() {
        match outcome.recv_timeout(STOP_POLL) {
            // Terminé, ou plus personne pour répondre : dans les deux cas, rien à retenir.
            Ok(_) | Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
}

/// Ce dont la boucle a besoin pour attendre. L'horloge vraie dort par tranches (pour
/// sonder l'arrêt) et lit l'heure locale du système ; les tests avancent le temps sans
/// dormir.
trait Clock {
    /// Attend `d` ; `false` si l'arrêt a été sonné pendant l'attente.
    fn sleep(&mut self, ctx: &Ctx, d: Duration) -> bool;
    /// Heure locale, en minutes depuis minuit ; `None` si elle est illisible.
    fn local_minute(&self) -> Option<u16>;
}

struct RealClock;

impl Clock for RealClock {
    fn sleep(&mut self, ctx: &Ctx, mut d: Duration) -> bool {
        while !d.is_zero() && !ctx.stopping() {
            let step = d.min(STOP_POLL);
            std::thread::sleep(step);
            d -= step;
        }
        !ctx.stopping()
    }

    fn local_minute(&self) -> Option<u16> {
        local_minute_of_day()
    }
}

/// Heure locale (minutes depuis minuit), sans dépendance ni FFI : `date +%H:%M` (Unix) ou
/// l'heure du système via `cmd` (Windows). `None` si le résultat est illisible.
fn local_minute_of_day() -> Option<u16> {
    #[cfg(windows)]
    let output = std::process::Command::new("cmd").args(["/c", "echo", "%TIME%"]).output();
    #[cfg(not(windows))]
    let output = std::process::Command::new("date").arg("+%H:%M").output();
    parse_local_minute(&String::from_utf8_lossy(&output.ok()?.stdout))
}

/// `09:05`, ` 9:05:12,34` (le `%TIME%` de cmd), `09:05:12.45` → minutes depuis minuit.
fn parse_local_minute(text: &str) -> Option<u16> {
    let first = text.split_whitespace().next()?;
    let mut parts = first.split(':');
    let hour: u8 = parts.next()?.parse().ok()?;
    let minute: u8 = parts.next()?.parse().ok()?;
    (hour < 24 && minute < 60).then_some(u16::from(hour) * 60 + u16::from(minute))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runners::RunResult;
    use crate::store::write_json_atomic;
    use serde_json::json;
    use std::collections::VecDeque;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    static SEQ: AtomicU32 = AtomicU32::new(0);

    /// Workspace temporaire unique (pid + compteur), supprimé à la fin du test.
    struct TempWs(PathBuf);

    impl TempWs {
        fn new(tag: &str) -> Self {
            let n = SEQ.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir()
                .join(format!("swarm-schedule-{tag}-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            TempWs(dir)
        }
    }

    impl Drop for TempWs {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Un Ctx sans serveur ni fil de travail, sur un workspace temporaire.
    fn ctx(name: &str, ws: &Path) -> Arc<Ctx> {
        Arc::new(Ctx::new(name, Runner::Claude, ws.to_path_buf(), "test".into()))
    }

    fn minutes(h: u8, m: u8) -> u16 {
        u16::from(h) * 60 + u16::from(m)
    }

    // --- Ligne de commande -------------------------------------------------

    #[test]
    fn les_durees_de_la_ligne_de_commande_se_lisent() {
        for (text, secs) in [
            ("30s", 30),
            ("5m", 300),
            ("1h", 3600),
            ("1h30", 5400),
            ("90m", 5400),
            ("2h", 7200),
            ("1h30m", 5400),
            ("1h 30", 5400),
            (" 45s ", 45),
            ("1h2m3s", 3723),
        ] {
            assert_eq!(parse_duration(text).unwrap(), Duration::from_secs(secs), "{text}");
        }
    }

    #[test]
    fn les_durees_invalides_sont_refusees() {
        for text in [
            "",
            "   ",
            "0s",
            "0h",
            "5",
            "abc",
            "5x",
            "h",
            "m30",
            "1m30",
            "-5m",
            "1.5h",
            "5m5",
            "99999999999999999999s",
            "18446744073709551615h",
        ] {
            let err = parse_duration(text).unwrap_err();
            assert!(err.starts_with("durée"), "{text} → {err}");
        }
        assert!(parse_duration("5").unwrap_err().contains("unité manquante"));
        assert!(parse_duration("0s").unwrap_err().contains("strictement positif"));
        assert!(parse_duration("1m30").unwrap_err().contains("unité manquante après « 30 »"));
    }

    #[test]
    fn les_fenetres_se_lisent() {
        assert_eq!(parse_window("09:00-18:00").unwrap(), Window { start: (9, 0), end: (18, 0) });
        assert_eq!(parse_window(" 9:00 - 18:00 ").unwrap().start, (9, 0));
        // À cheval sur minuit : le début après la fin.
        assert_eq!(parse_window("22:00-06:00").unwrap(), Window { start: (22, 0), end: (6, 0) });
        assert_eq!(parse_window("00:00-23:59").unwrap().end, (23, 59));
    }

    #[test]
    fn les_fenetres_invalides_sont_refusees() {
        for text in [
            "",
            "09:00",
            "09-18",
            "24:00-18:00",
            "09:60-18:00",
            "0900-1800",
            "09:00-09:00",
            "abc",
            "09:00-18:00-20:00",
            "09h00-18h00",
            "09:00--18:00",
        ] {
            let err = parse_window(text).unwrap_err();
            assert!(err.starts_with("fenêtre"), "{text} → {err}");
        }
        assert!(parse_window("09:00-09:00").unwrap_err().contains("toute la journée"));
        assert!(parse_window("24:00-18:00").unwrap_err().contains("24:00"));
    }

    #[test]
    fn une_fenetre_est_fermee_hors_de_ses_heures_meme_a_cheval_sur_minuit() {
        let day = Window { start: (9, 0), end: (18, 0) };
        for (hm, open) in
            [((8, 59), false), ((9, 0), true), ((17, 59), true), ((18, 0), false), ((0, 0), false)]
        {
            assert_eq!(day.contains(minutes(hm.0, hm.1)), open, "{hm:?}");
        }
        let night = Window { start: (22, 0), end: (6, 0) };
        for (hm, open) in [
            ((21, 59), false),
            ((22, 0), true),
            ((23, 30), true),
            ((0, 0), true),
            ((5, 59), true),
            ((6, 0), false),
            ((12, 0), false),
        ] {
            assert_eq!(night.contains(minutes(hm.0, hm.1)), open, "{hm:?}");
        }
    }

    #[test]
    fn le_report_a_l_ouverture_compte_jusqu_au_prochain_debut() {
        let day = Window { start: (9, 0), end: (18, 0) };
        assert_eq!(day.minutes_until_open(minutes(8, 0)), 60);
        assert_eq!(day.minutes_until_open(minutes(8, 59)), 1);
        assert_eq!(day.minutes_until_open(minutes(9, 0)), 0, "ouverte : rien à attendre");
        assert_eq!(day.minutes_until_open(minutes(18, 0)), 900, "le lendemain matin");
        assert_eq!(day.minutes_until_open(minutes(23, 59)), 541);
        let night = Window { start: (22, 0), end: (6, 0) };
        assert_eq!(night.minutes_until_open(minutes(6, 0)), 960);
        assert_eq!(night.minutes_until_open(minutes(12, 0)), 600);
        assert_eq!(night.minutes_until_open(minutes(3, 0)), 0);
    }

    #[test]
    fn un_schedule_exige_un_prompt_et_des_valeurs_valides() {
        let s = Schedule::parse("5m", "résume".into(), Some("09:00-18:00")).unwrap();
        assert_eq!((s.every, s.prompt.as_str()), (Duration::from_secs(300), "résume"));
        assert_eq!(s.window, Some(Window { start: (9, 0), end: (18, 0) }));
        assert_eq!(Schedule::parse("5m", "résume".into(), None).unwrap().window, None);
        assert!(Schedule::parse("5m", "  ".into(), None).unwrap_err().contains("prompt vide"));
        assert!(Schedule::parse("5x", "résume".into(), None).is_err());
        assert!(Schedule::parse("5m", "résume".into(), Some("25:00")).is_err());
    }

    // --- Définition de l'agent ---------------------------------------------

    /// Écrit la définition Claude de l'agent, avec le `schedule` demandé (ou sans).
    fn definition(ws: &TempWs, schedule: Option<Value>) {
        let path = crate::agents::claude_settings_path(&ws.0, "alice");
        let mut doc = json!({ "env": {}, "template": "monitor" });
        if let Some(schedule) = schedule {
            doc["schedule"] = schedule;
        }
        write_json_atomic(&path, &doc).unwrap();
    }

    #[test]
    fn from_definition_lit_le_schedule_de_la_definition() {
        let ws = TempWs::new("objet");
        definition(
            &ws,
            Some(
                json!({ "every": "1h30", "prompt": "Vérifie tes mentions", "window": "09:00-18:00" }),
            ),
        );
        let s = from_definition(&ws.0, Runner::Claude, "alice").expect("schedule lu");
        assert_eq!(s.every, Duration::from_secs(5400));
        assert_eq!(s.prompt, "Vérifie tes mentions");
        assert_eq!(s.window, Some(Window { start: (9, 0), end: (18, 0) }));
        // La fenêtre est optionnelle ; `null` vaut absence.
        definition(&ws, Some(json!({ "every": "30s", "prompt": "résume", "window": null })));
        let s = from_definition(&ws.0, Runner::Claude, "alice").unwrap();
        assert_eq!((s.every, s.window), (Duration::from_secs(30), None));
        // Chaîne simple : « <durée> <prompt> ».
        definition(&ws, Some(json!("5m  Fais le point  ")));
        let s = from_definition(&ws.0, Runner::Claude, "alice").unwrap();
        assert_eq!(
            (s.every, s.prompt.as_str(), s.window),
            (Duration::from_secs(300), "Fais le point", None)
        );
        // Antigravity : même lecture, sur son propre fichier de définition.
        write_json_atomic(
            &crate::agents::antigravity_settings_path(&ws.0, "alice"),
            &json!({ "schedule": { "every": "2h", "prompt": "Rapport du jour" } }),
        )
        .unwrap();
        let s = from_definition(&ws.0, Runner::Antigravity, "alice").unwrap();
        assert_eq!((s.every, s.prompt.as_str()), (Duration::from_secs(7200), "Rapport du jour"));
    }

    #[test]
    fn from_definition_rend_none_sans_schedule() {
        let ws = TempWs::new("sans");
        // Aucune définition du tout, puis définition sans `schedule`, puis `null`.
        assert_eq!(read_definition(&ws.0, Runner::Claude, "alice"), Ok(None));
        definition(&ws, None);
        assert_eq!(read_definition(&ws.0, Runner::Claude, "alice"), Ok(None));
        definition(&ws, Some(Value::Null));
        assert_eq!(read_definition(&ws.0, Runner::Claude, "alice"), Ok(None));
        assert_eq!(from_definition(&ws.0, Runner::Claude, "alice"), None);
    }

    #[test]
    fn un_schedule_mal_forme_est_signale_et_ignore() {
        let ws = TempWs::new("invalide");
        for (field, needle) in [
            (json!({ "every": "5x", "prompt": "résume" }), "durée « 5x »"),
            (json!({ "every": "5m" }), "schedule.prompt"),
            (json!({ "every": 5, "prompt": "résume" }), "schedule.every"),
            (json!({ "every": "5m", "prompt": "résumé", "window": "25:00-18:00" }), "fenêtre"),
            (json!({ "every": "5m", "prompt": "résumé", "window": 9 }), "schedule.window"),
            (json!("5m"), "forme simple"),
            (json!(5), "objet"),
        ] {
            definition(&ws, Some(field));
            let err = read_definition(&ws.0, Runner::Claude, "alice").unwrap_err();
            assert!(err.contains("définition de 'alice'") && err.contains(needle), "{err}");
            assert_eq!(
                from_definition(&ws.0, Runner::Claude, "alice"),
                None,
                "ignoré, sans panique"
            );
        }
    }

    // --- Boucle du planificateur (horloge fausse) ---------------------------

    /// Horloge fausse : le temps avance du montant demandé (ou d'un montant scripté, pour
    /// simuler la veille de la machine) sans dormir ; sonne l'arrêt après `stops_after`
    /// attentes.
    struct FakeClock {
        elapsed: Duration,
        /// Heure locale au départ, en minutes depuis minuit.
        local_start: u16,
        /// Montants réels des prochaines attentes (au-delà : le montant demandé).
        script: VecDeque<Duration>,
        /// Attentes demandées, dans l'ordre.
        requested: Vec<Duration>,
        stops_after: usize,
        /// Heure locale illisible (ni `date` ni `cmd`).
        blind: bool,
    }

    impl FakeClock {
        fn new(local_start: u16) -> Self {
            FakeClock {
                elapsed: Duration::ZERO,
                local_start,
                script: VecDeque::new(),
                requested: Vec::new(),
                stops_after: usize::MAX,
                blind: false,
            }
        }
    }

    impl Clock for FakeClock {
        fn sleep(&mut self, ctx: &Ctx, d: Duration) -> bool {
            if self.requested.len() >= self.stops_after {
                ctx.stopping.store(true, Ordering::SeqCst);
                return false;
            }
            self.requested.push(d);
            self.elapsed += self.script.pop_front().unwrap_or(d);
            !ctx.stopping()
        }

        fn local_minute(&self) -> Option<u16> {
            (!self.blind).then_some(
                (self.local_start + u16::try_from(self.elapsed.as_secs() / 60).unwrap()) % DAY,
            )
        }
    }

    /// Tours vus par le faux fil de travail : (origine, prompt).
    type Seen = Arc<Mutex<Vec<(Option<String>, String)>>>;

    /// Faux fil de travail : retire les tours de la file et les termine tout de suite
    /// (ou les garde `hold`, pour un tour long : tant qu'il vit, sa réponse n'arrive pas),
    /// en notant origine et prompt.
    fn fake_worker(
        ctx: &Arc<Ctx>,
        hold: bool,
    ) -> (Arc<AtomicBool>, Seen, std::thread::JoinHandle<()>) {
        let stop = Arc::new(AtomicBool::new(false));
        let turns: Seen = Arc::new(Mutex::new(Vec::new()));
        let held = Arc::new(Mutex::new(Vec::new()));
        let (ctx, stop2, turns2, held2) =
            (Arc::clone(ctx), Arc::clone(&stop), Arc::clone(&turns), Arc::clone(&held));
        let handle = std::thread::spawn(move || {
            while !stop2.load(Ordering::SeqCst) {
                if let Some(turn) = ctx.inbox.pop(Duration::from_millis(5)) {
                    turns2.lock().unwrap().push((turn.from.clone(), turn.prompt.clone()));
                    if hold {
                        held2.lock().unwrap().push(turn);
                    } else {
                        let queued = turn.queued_ms();
                        turn.complete(RunResult::default(), queued, 0);
                    }
                }
            }
            drop(held2);
        });
        (stop, turns, handle)
    }

    fn schedule(every: Duration, window: Option<Window>) -> Schedule {
        Schedule { every, prompt: "résume".into(), window }
    }

    /// Pas de rafale : avec un tour toutes les 5 min, une veille de 7 h ne rejoue qu'un
    /// seul tour, et le suivant repart de la fin de celui-ci (pas des échéances manquées).
    #[test]
    fn une_longue_veille_ne_rattrape_pas_les_echeances_manquees() {
        let ws = TempWs::new("veille");
        let ctx = ctx("alice", &ws.0);
        let (stop, turns, worker) = fake_worker(&ctx, false);
        let mut clock = FakeClock::new(0);
        let every = Duration::from_secs(300);
        clock.script.extend([every, Duration::from_secs(7 * 3600)]);
        clock.stops_after = 2;
        run_loop(&ctx, &schedule(every, None), &mut clock);
        stop.store(true, Ordering::SeqCst);
        worker.join().unwrap();
        let turns = turns.lock().unwrap();
        assert_eq!(turns.len(), 2, "une échéance, puis UN tour après la veille : {turns:?}");
        assert!(
            turns
                .iter()
                .all(|(from, prompt)| from.as_deref() == Some(ORIGIN) && prompt == "résume"),
            "{turns:?}"
        );
        // La seconde attente demandait encore 5 min : recomptée depuis la fin du tour.
        assert_eq!(clock.requested, [every, every]);
    }

    /// Fenêtre fermée à l'échéance → un seul tour, à l'ouverture ; ouverte → tout de suite.
    #[test]
    fn une_fenetre_fermee_reporte_un_seul_tour_a_l_ouverture() {
        let every = Duration::from_secs(60);
        for (window, local_start, wait_minutes) in [
            (Window { start: (9, 0), end: (18, 0) }, minutes(10, 0), 0),
            (Window { start: (9, 0), end: (18, 0) }, minutes(17, 59), 900),
            (Window { start: (9, 0), end: (18, 0) }, minutes(8, 0), 59),
            (Window { start: (22, 0), end: (6, 0) }, minutes(17, 59), 240),
            (Window { start: (22, 0), end: (6, 0) }, minutes(5, 0), 0),
        ] {
            let ws = TempWs::new("fenetre");
            let ctx = ctx("alice", &ws.0);
            let (stop, turns, worker) = fake_worker(&ctx, false);
            let mut clock = FakeClock::new(local_start);
            clock.stops_after = if wait_minutes == 0 { 1 } else { 2 };
            run_loop(&ctx, &schedule(every, Some(window)), &mut clock);
            stop.store(true, Ordering::SeqCst);
            worker.join().unwrap();
            let mut expected = vec![every];
            if wait_minutes > 0 {
                expected.push(Duration::from_secs(wait_minutes * 60));
            }
            assert_eq!(clock.requested, expected, "fenêtre {window:?} à {local_start} min");
            let turns = turns.lock().unwrap();
            assert_eq!(turns.len(), 1, "un seul tour part à l'ouverture : {turns:?}");
            assert_eq!(turns[0], (Some(ORIGIN.to_string()), "résume".to_string()));
        }
    }

    /// L'arrêt pendant un tour planifié en cours fait ressortir le planificateur sans
    /// attendre la fin de ce tour (tué par l'arrêt) ni en déposer un autre.
    #[test]
    fn l_arret_pendant_un_tour_fait_resortir_le_planificateur() {
        let ws = TempWs::new("arret");
        let ctx = ctx("alice", &ws.0);
        let (stop, turns, worker) = fake_worker(&ctx, true);
        let looper = {
            let ctx = Arc::clone(&ctx);
            let schedule = schedule(Duration::from_secs(60), None);
            std::thread::spawn(move || {
                let mut clock = FakeClock::new(0);
                run_loop(&ctx, &schedule, &mut clock);
                clock.requested
            })
        };
        // Le tour est retiré de la file (donc en cours) : l'arrêt sonne alors.
        let deadline = Instant::now() + Duration::from_secs(5);
        while turns.lock().unwrap().is_empty() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!turns.lock().unwrap().is_empty(), "le tour planifié a été déposé");
        let started = Instant::now();
        ctx.stopping.store(true, Ordering::SeqCst);
        while !looper.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(looper.is_finished(), "le planificateur ne retient pas le tour tué");
        assert!(started.elapsed() < Duration::from_secs(3), "ressorti vite après l'arrêt");
        let requested = looper.join().unwrap();
        stop.store(true, Ordering::SeqCst);
        worker.join().unwrap();
        assert_eq!(requested.len(), 1, "pas de recomptage après l'arrêt : {requested:?}");
        assert_eq!(turns.lock().unwrap().len(), 1, "aucun second tour déposé");
    }

    /// L'arrêt pendant l'attente de l'échéance : plus aucun tour n'est déposé.
    #[test]
    fn l_arret_pendant_l_attente_ne_depose_aucun_tour() {
        let ws = TempWs::new("arret-attente");
        let ctx = ctx("alice", &ws.0);
        let (stop, turns, worker) = fake_worker(&ctx, false);
        let mut clock = FakeClock::new(0);
        clock.stops_after = 0;
        run_loop(&ctx, &schedule(Duration::from_secs(60), None), &mut clock);
        stop.store(true, Ordering::SeqCst);
        worker.join().unwrap();
        assert!(clock.requested.is_empty());
        assert!(turns.lock().unwrap().is_empty());
    }

    /// Heure locale illisible alors qu'une fenêtre est posée : l'échéance est sautée (on ne
    /// suppose pas la fenêtre ouverte), le cycle continue, aucun tour n'est déposé.
    #[test]
    fn une_heure_locale_illisible_saute_l_echeance_sans_deposer_de_tour() {
        let ws = TempWs::new("aveugle");
        let ctx = ctx("alice", &ws.0);
        let (stop, turns, worker) = fake_worker(&ctx, false);
        let every = Duration::from_secs(60);
        let mut clock = FakeClock::new(0);
        clock.blind = true;
        clock.stops_after = 2;
        run_loop(&ctx, &schedule(every, Some(Window { start: (9, 0), end: (18, 0) })), &mut clock);
        stop.store(true, Ordering::SeqCst);
        worker.join().unwrap();
        assert_eq!(clock.requested, [every, every], "deux échéances sautées, cycle continu");
        assert!(turns.lock().unwrap().is_empty());
    }

    // --- Vrai fil ----------------------------------------------------------

    /// Le vrai fil : un tour `from: "schedule"` arrive dans la file après un intervalle
    /// court, puis le fil s'arrête quand `stopping` est sonné.
    #[test]
    fn spawn_depose_un_tour_puis_s_arrete_avec_l_agent() {
        let ws = TempWs::new("spawn");
        let ctx = ctx("alice", &ws.0);
        let zero = Schedule { every: Duration::ZERO, prompt: "résume".into(), window: None };
        assert!(spawn(Arc::clone(&ctx), zero).is_err(), "intervalle nul refusé");
        let schedule =
            Schedule { every: Duration::from_millis(600), prompt: "résume".into(), window: None };
        let handle = spawn(Arc::clone(&ctx), schedule).unwrap();
        let turn = ctx.inbox.pop(Duration::from_secs(5)).expect("tour planifié déposé");
        assert_eq!((turn.from.as_deref(), turn.prompt.as_str()), (Some(ORIGIN), "résume"));
        let queued = turn.queued_ms();
        turn.complete(RunResult::default(), queued, 0);
        ctx.stopping.store(true, Ordering::SeqCst);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !handle.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(handle.is_finished(), "le fil planifié s'arrête avec l'agent");
        handle.join().unwrap();
    }

    // --- Heure locale ------------------------------------------------------

    #[test]
    fn l_heure_de_cmd_et_de_date_se_lisent() {
        assert_eq!(parse_local_minute("09:05\n"), Some(545));
        assert_eq!(parse_local_minute(" 9:05:12,34\r\n"), Some(545));
        assert_eq!(parse_local_minute("09:05:12.45"), Some(545));
        for bad in ["", "abc", "24:00", "09:60", "9", "9:", ":05", "09/05"] {
            assert_eq!(parse_local_minute(bad), None, "{bad}");
        }
    }

    /// L'heure locale du système est lisible et cohérente avec l'UTC (à moins de 14 h).
    #[test]
    fn l_heure_locale_du_systeme_est_lisible() {
        let local = local_minute_of_day().expect("heure locale du système");
        let since =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        let utc = u16::try_from((since / 60) % u64::from(DAY)).unwrap();
        let gap = local.abs_diff(utc);
        assert!(gap <= 840 || gap >= DAY - 840, "locale {local} min, UTC {utc} min");
    }
}
