//! Ce que l'en-tête montre : identité de l'agent, session abrégée, pont HTTP.
//!
//! Le pont n'est pas exposé par [`Ctx`] (il vit dans `residents::bridge`) : on le relit dans la
//! ligne de commande du process, qui l'a reçue (`--bridge N`, `--host H`). Absent, l'en-tête
//! affiche un tiret.

use crate::residents::Ctx;

/// Identité figée au démarrage de la TUI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub agent: String,
    pub runner: String,
    pub instance: String,
    pub workspace: String,
    /// `http://hôte:port` du pont HTTP, si `--bridge` était demandé.
    pub bridge: Option<String>,
}

pub fn identify(ctx: &Ctx) -> Identity {
    Identity {
        agent: ctx.agent.clone(),
        runner: ctx.runner.as_str().to_string(),
        instance: ctx.instance.clone(),
        workspace: ctx.workspace.display().to_string(),
        bridge: bridge_from_args(std::env::args()),
    }
}

/// `http://<hôte>:<port>` du pont, d'après la ligne de commande.
pub fn bridge_from_args<I: IntoIterator<Item = String>>(args: I) -> Option<String> {
    let mut host = "127.0.0.1".to_string();
    let mut port: Option<u16> = None;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--bridge" | "-b" => port = args.next().and_then(|p| p.parse().ok()),
            "--host" => host = args.next().unwrap_or(host),
            _ => {}
        }
    }
    port.map(|p| format!("http://{host}:{p}"))
}

/// Début de l'identifiant de session : assez pour la reconnaître, assez court pour l'en-tête.
pub fn short_session(id: &str) -> String {
    let mut short: String = id.chars().take(8).collect();
    if id.chars().count() > 8 {
        short.push('…');
    }
    short
}

/// Durée lisible : `420 ms`, `1,4 s`, `2 min 05 s`.
pub fn duration_label(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms} ms")
    } else if ms < 60_000 {
        format!("{:.1} s", ms as f64 / 1000.0).replace('.', ",")
    } else {
        let secs = ms / 1000;
        format!("{} min {:02} s", secs / 60, secs % 60)
    }
}

/// Âge lisible d'un horodatage (ms depuis l'époque), sans dépendre du fuseau local.
pub fn age_label(now: u64, at: u64) -> String {
    let age = now.saturating_sub(at) / 1000;
    match age {
        0..=1 => "à l'instant".to_string(),
        2..=59 => format!("il y a {age} s"),
        60..=3599 => format!("il y a {} min", age / 60),
        3600..=86_399 => format!("il y a {} h", age / 3600),
        _ => format!("il y a {} j", age / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lit_le_pont_dans_la_ligne_de_commande() {
        let args = ["swarm-control", "--agent", "alice", "--bridge", "3001"].map(String::from);
        assert_eq!(bridge_from_args(args).as_deref(), Some("http://127.0.0.1:3001"));
        let args = ["swarm-control", "--host", "0.0.0.0", "-b", "9"].map(String::from);
        assert_eq!(bridge_from_args(args).as_deref(), Some("http://0.0.0.0:9"));
        assert_eq!(bridge_from_args(Vec::new()), None);
        let bad = ["swarm-control", "--bridge", "x"].map(String::from);
        assert_eq!(bridge_from_args(bad), None);
    }

    #[test]
    fn abrege_la_session_et_formate_les_durees() {
        assert_eq!(short_session("3f2a9c1e-4b5d"), "3f2a9c1e…");
        assert_eq!(short_session("court"), "court");
        assert_eq!(duration_label(420), "420 ms");
        assert_eq!(duration_label(1400), "1,4 s");
        assert_eq!(duration_label(125_000), "2 min 05 s");
    }

    #[test]
    fn donne_l_age_en_francais() {
        let now = 1_000_000_000;
        assert_eq!(age_label(now, now - 500), "à l'instant");
        assert_eq!(age_label(now, now - 12_000), "il y a 12 s");
        assert_eq!(age_label(now, now - 120_000), "il y a 2 min");
        assert_eq!(age_label(now, now - 7_200_000), "il y a 2 h");
        assert_eq!(age_label(now, 0), "il y a 11 j");
    }
}
