//! Score d'un souvenir pour une requête : termes trouvés (pondérés) + récence.
//!
//! Les termes de la requête sont normalisés (voir [`super::normalize`]) et dédupliqués.
//! Chaque terme présent rapporte son poids le plus élevé : dans un tag (double), dans le
//! texte, ou comme préfixe d'un mot du texte (moitié : pluriel, conjugaison). Un souvenir
//! sans aucun terme trouvé n'est jamais rendu. La récence, dans `]0 ; 1]`, ne départage
//! que des souvenirs qui trouvent les mêmes termes.

use super::Memory;
use super::normalize::tokens;
use super::path;
use serde_json::{Value, json};
use std::path::Path;

/// Poids d'un terme trouvé dans le texte.
const TEXT_WEIGHT: f64 = 1.0;
/// Poids d'un terme trouvé dans un tag.
const TAG_WEIGHT: f64 = 2.0;
/// Poids d'un terme qui est le préfixe d'un mot du texte.
const PREFIX_WEIGHT: f64 = 0.5;
/// Taille minimale d'un terme pour compter comme préfixe (`résea` → `réseaux`).
const PREFIX_MIN: usize = 4;
/// Demi-vie de la récence, en jours : un souvenir d'un mois pèse moitié moins.
const HALF_LIFE_DAYS: f64 = 30.0;
const MS_PER_DAY: f64 = 86_400_000.0;

/// Portée d'une recherche.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// La mémoire de l'agent lui-même.
    Own,
    /// Lecture seule des mémoires des autres agents du même workspace.
    Swarm,
}

impl Scope {
    /// Nom de la portée dans les outils MCP et la CLI.
    pub fn as_str(self) -> &'static str {
        match self {
            Scope::Own => "self",
            Scope::Swarm => "swarm",
        }
    }

    /// Portée nommée comme dans les outils MCP (`self`, `swarm`) ; autre nom → `None`.
    pub fn parse(name: &str) -> Option<Scope> {
        match name {
            "self" => Some(Scope::Own),
            "swarm" => Some(Scope::Swarm),
            _ => None,
        }
    }
}

/// Un souvenir trouvé : le souvenir, son score et l'agent qui le possède (lui-même en
/// portée [`Scope::Own`], un voisin en portée [`Scope::Swarm`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub memory: Memory,
    pub score: f64,
    /// Runner de l'agent propriétaire.
    pub runner: String,
    /// Agent propriétaire du souvenir.
    pub agent: String,
}

impl Hit {
    /// Rendu d'un résultat (outil `memory_search`, CLI) : `{id, ts, text, tags, score, agent,
    /// runner}`, le score arrondi au millième.
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.memory.id,
            "ts": self.memory.ts,
            "text": self.memory.text,
            "tags": self.memory.tags,
            "score": (self.score * 1000.0).round() / 1000.0,
            "agent": self.agent,
            "runner": self.runner,
        })
    }
}

/// Cherche dans la mémoire de `runner`-`agent` (portée [`Scope::Own`]) ou dans celle des
/// autres agents du workspace ([`Scope::Swarm`], lecture seule), les `limit` meilleurs.
///
/// Une requête sans aucun terme exploitable est une erreur explicite plutôt qu'une liste
/// vide. Les fichiers illisibles ou absents rendent simplement rien.
pub fn search(
    ws: &Path,
    runner: &str,
    agent: &str,
    query: &str,
    limit: usize,
    scope: Scope,
) -> Result<Vec<Hit>, String> {
    let terms = tokens(query);
    if terms.is_empty() {
        return Err("requête sans terme exploitable (ni lettres, ni chiffres)".into());
    }
    let now = crate::store::now_ms();
    let mut hits = Vec::new();
    match scope {
        Scope::Own => {
            let entries = super::store::read_file(&path::path(ws, runner, agent));
            rank(&entries, runner, agent, &terms, now, &mut hits);
        }
        Scope::Swarm => {
            for (runner, agent, file) in path::neighbours(ws, runner, agent) {
                let entries = super::store::read_file(&file);
                rank(&entries, &runner, &agent, &terms, now, &mut hits);
            }
        }
    }
    hits.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then(b.memory.ts.cmp(&a.memory.ts))
            .then(a.memory.id.cmp(&b.memory.id))
    });
    hits.truncate(limit);
    Ok(hits)
}

/// Ajoute à `hits` les souvenirs de `entries` qui trouvent au moins un terme.
pub(super) fn rank(
    entries: &[Memory],
    runner: &str,
    agent: &str,
    terms: &[String],
    now: u64,
    hits: &mut Vec<Hit>,
) {
    for memory in entries {
        if let Some(score) = score(memory, terms, now) {
            hits.push(Hit {
                memory: memory.clone(),
                score,
                runner: runner.to_string(),
                agent: agent.to_string(),
            });
        }
    }
}

/// Score d'un souvenir pour des termes déjà normalisés : `None` s'il n'en contient aucun.
pub fn score(memory: &Memory, terms: &[String], now: u64) -> Option<f64> {
    let text = tokens(&memory.text);
    let tags: Vec<String> = memory.tags.iter().flat_map(|t| tokens(t)).collect();
    let mut found = 0.0;
    for term in terms {
        let as_str = term.as_str();
        found += if tags.iter().any(|t| t == term) {
            TAG_WEIGHT
        } else if text.iter().any(|t| t == term) {
            TEXT_WEIGHT
        } else if as_str.chars().count() >= PREFIX_MIN
            && text.iter().any(|t| {
                t.starts_with(as_str)
                    || (t.chars().count() >= PREFIX_MIN && as_str.starts_with(t.as_str()))
            })
        {
            PREFIX_WEIGHT
        } else {
            0.0
        };
    }
    (found > 0.0).then_some(found + recency(memory.ts, now))
}

/// Bonus de récence : 1 aujourd'hui, ½ à 30 jours, ⅓ à 60 jours…
fn recency(ts: u64, now: u64) -> f64 {
    let age_days = now.saturating_sub(ts) as f64 / MS_PER_DAY;
    1.0 / (1.0 + age_days / HALF_LIFE_DAYS)
}

#[cfg(test)]
mod tests {
    use super::{Hit, Scope, recency, score, search};
    use crate::memory::Memory;
    use serde_json::json;
    use std::path::Path;

    fn memory(id: &str, ts: u64, text: &str, tags: &[&str]) -> Memory {
        Memory {
            id: id.into(),
            ts,
            text: text.into(),
            tags: tags.iter().map(|t| t.to_string()).collect(),
            source: String::new(),
        }
    }

    fn terms(query: &str) -> Vec<String> {
        super::tokens(query)
    }

    #[test]
    fn un_terme_trouve_rapporte_son_poids_le_plus_fort() {
        let now = 1_700_000_000_000;
        let text = memory("a", now, "Le port du serveur est 23090", &[]);
        let tag = memory("b", now, "Divers", &["port"]);
        let both = memory("c", now, "Le port est ouvert", &["port"]);
        let none = memory("d", now, "Rien à voir", &[]);
        assert_eq!(score(&text, &terms("port"), now), Some(2.0), "1 + récence 1");
        assert_eq!(score(&tag, &terms("port"), now), Some(3.0), "tag = double");
        assert_eq!(score(&both, &terms("port"), now), Some(3.0), "pas compté deux fois");
        assert_eq!(score(&none, &terms("port"), now), None);
    }

    #[test]
    fn plusieurs_termes_s_additionnent_et_les_doublons_ne_comptent_pas() {
        let now = 1_700_000_000_000;
        let m = memory("a", now, "Le port du serveur est 23090", &[]);
        assert_eq!(score(&m, &terms("port serveur"), now), Some(3.0));
        assert_eq!(score(&m, &terms("port PORT Port"), now), Some(2.0), "termes dédupliqués");
        assert_eq!(score(&m, &terms("port_absent"), now), Some(2.0), "`_` sépare : port compte");
        assert_eq!(score(&m, &terms("zorglub"), now), None);
    }

    #[test]
    fn un_prefixe_long_compte_moitie() {
        let now = 1_700_000_000_000;
        let m = memory("a", now, "Les réseaux sont configurés", &[]);
        assert_eq!(score(&m, &terms("résea"), now), Some(1.5));
        assert_eq!(score(&m, &terms("ré"), now), None, "préfixe trop court");
        assert_eq!(score(&m, &terms("configuration"), now), None, "pas de suffixe");
        // Un préfixe de tag ne compte pas : les tags s'écrivent en entier.
        let t = memory("b", now, "x", &["réseaux"]);
        assert_eq!(score(&t, &terms("résea"), now), None);
    }

    #[test]
    fn la_recence_decroit_avec_l_age_et_reste_positive() {
        let day = 86_400_000u64;
        assert_eq!(recency(100, 100), 1.0);
        assert_eq!(recency(100, 100 + 30 * day), 0.5);
        assert!((recency(100, 100 + 60 * day) - 1.0 / 3.0).abs() < 1e-9);
        assert!(recency(100, 100 + 3650 * day) > 0.0);
        assert_eq!(recency(200, 100), 1.0, "une horloge en avance n'invente pas un âge négatif");
    }

    #[test]
    fn le_score_est_termes_plus_recence() {
        let now = 1_700_000_000_000;
        let fresh = memory("a", now, "port", &[]);
        let old = memory("b", now - 30 * 86_400_000, "port", &[]);
        assert_eq!(score(&fresh, &terms("port"), now), Some(2.0));
        assert_eq!(score(&old, &terms("port"), now), Some(1.5));
        // Un terme de plus l'emporte toujours sur la récence (max 1).
        let old_two = memory("c", now - 30 * 86_400_000, "port serveur", &[]);
        assert_eq!(score(&old_two, &terms("port serveur"), now), Some(2.5));
    }

    #[test]
    fn scope_names_round_trip() {
        assert_eq!(Scope::Own.as_str(), "self");
        assert_eq!(Scope::Swarm.as_str(), "swarm");
        assert_eq!(Scope::parse("swarm"), Some(Scope::Swarm));
        assert_eq!(Scope::parse("self"), Some(Scope::Own));
        assert_eq!(Scope::parse("autre"), None);
    }

    #[test]
    fn hit_json_carries_the_owner_and_a_rounded_score() {
        let hit = Hit {
            memory: memory("1a-0001", 42, "le port", &["réseau"]),
            score: 2.3333333,
            runner: "claude".into(),
            agent: "bob".into(),
        };
        assert_eq!(
            hit.to_json(),
            json!({
                "id": "1a-0001", "ts": 42, "text": "le port", "tags": ["réseau"],
                "score": 2.333, "agent": "bob", "runner": "claude",
            })
        );
    }

    #[test]
    fn une_requete_sans_terme_est_une_erreur() {
        let dir = std::env::temp_dir().join(format!("swarm-memory-score-{}", std::process::id()));
        let err = search(Path::new(&dir), "claude", "alice", "  !!  ", 10, Scope::Own).unwrap_err();
        assert!(err.contains("sans terme"), "{err}");
    }
}
