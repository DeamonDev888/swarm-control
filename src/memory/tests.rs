//! Tests du module : aller-retour, recherche (casse, accents, récence), compaction,
//! filtrage des jetons, oubli, portée `swarm`.

use super::{Hit, MAX_LINES, Memory, Scope, forget, path, read, search, store};
use std::path::{Path, PathBuf};

fn ws(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("swarm-memory-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn memory(id: &str, ts: u64, text: &str, tags: &[&str]) -> Memory {
    Memory {
        id: id.into(),
        ts,
        text: text.into(),
        tags: tags.iter().map(|t| t.to_string()).collect(),
        source: "test".into(),
    }
}

fn texts(hits: &[Hit]) -> Vec<&str> {
    hits.iter().map(|h| h.memory.text.as_str()).collect()
}

fn tags(items: &[&str]) -> Vec<String> {
    items.iter().map(|t| t.to_string()).collect()
}

#[test]
fn store_then_read_round_trips_every_field() {
    let ws = ws("aller-retour");
    let stored = store(
        &ws,
        "claude",
        "alice",
        "  Le port est 23090  ",
        &tags(&["réseau", " Port ", "", "RÉSEAU"]),
        "mcp",
    )
    .unwrap();
    assert_eq!(stored.text, "Le port est 23090", "le texte est paré, pas modifié");
    assert_eq!(stored.tags, ["réseau", "Port"], "vides et doublons (accents, casse) écartés");
    assert_eq!(stored.source, "mcp");
    assert!(stored.ts > 1_600_000_000_000, "horodaté maintenant");
    let file = path(&ws, "claude", "alice");
    assert_eq!(file, ws.join("bridge").join("memory").join("claude-alice.jsonl"));
    assert!(file.is_file(), "le dossier est créé au premier ajout");
    let line = std::fs::read_to_string(&file).unwrap();
    assert!(line.contains("\"id\":\""), "{line}");
    assert!(line.contains("\"text\":\"Le port est 23090\""), "{line}");
    assert!(line.contains("\"tags\":[\"réseau\",\"Port\"]"), "{line}");
    assert!(line.contains("\"source\":\"mcp\""), "{line}");
    assert!(line.ends_with('\n') && line.lines().count() == 1, "une ligne par souvenir");
    assert_eq!(read(&ws, "claude", "alice"), std::slice::from_ref(&stored));
    // Un second souvenir, source différente, sans tags : le champ `tags` est omis.
    let second = store(&ws, "claude", "alice", "Autre chose", &[], "cli").unwrap();
    assert_ne!(stored.id, second.id, "ids distincts");
    let text = std::fs::read_to_string(&file).unwrap();
    assert_eq!(text.lines().count(), 2);
    assert!(!text.lines().nth(1).unwrap().contains("\"tags\""), "{text}");
    assert!(!text.lines().nth(1).unwrap().contains("\"source\":\"mcp\""), "{text}");
    assert_eq!(read(&ws, "claude", "alice"), [stored, second]);
    let _ = std::fs::remove_dir_all(&ws);
}

#[test]
fn a_missing_file_reads_empty_and_search_returns_nothing() {
    let ws = ws("absent");
    assert!(read(&ws, "claude", "alice").is_empty());
    assert!(search(&ws, "claude", "alice", "port", 10, Scope::Own).unwrap().is_empty());
    assert!(!ws.exists(), "une lecture ne crée rien");
    assert!(!forget(&ws, "claude", "alice", "x").unwrap());
    let _ = std::fs::remove_dir_all(&ws);
}

#[test]
fn unreadable_lines_are_skipped_not_fatal() {
    let ws = ws("illisible");
    store(&ws, "claude", "alice", "bon", &[], "mcp").unwrap();
    let file = path(&ws, "claude", "alice");
    let mut text = std::fs::read_to_string(&file).unwrap();
    text.push_str("{tronqué\n");
    text.push_str("{\"ts\": 1, \"text\": \"sans id\"}\n");
    text.push('\n');
    std::fs::write(&file, &text).unwrap();
    store(&ws, "claude", "alice", "après", &[], "mcp").unwrap();
    let back = read(&ws, "claude", "alice");
    assert_eq!(back.iter().map(|m| m.text.as_str()).collect::<Vec<_>>(), ["bon", "après"]);
    assert_eq!(texts(&search(&ws, "claude", "alice", "après", 10, Scope::Own).unwrap()), ["après"]);
    let _ = std::fs::remove_dir_all(&ws);
}

#[test]
fn search_ignores_case_and_accents_on_both_sides() {
    let ws = ws("accents");
    store(
        &ws,
        "claude",
        "alice",
        "Le Réseau de Bob est configuré",
        &tags(&["Infrastructure"]),
        "mcp",
    )
    .unwrap();
    store(&ws, "claude", "alice", "Rien de pertinent", &[], "mcp").unwrap();
    let expected = ["Le Réseau de Bob est configuré"];
    assert_eq!(texts(&search(&ws, "claude", "alice", "réseau", 10, Scope::Own).unwrap()), expected);
    assert_eq!(
        texts(&search(&ws, "claude", "alice", "RESEAUX", 10, Scope::Own).unwrap()),
        expected,
        "préfixe"
    );
    assert_eq!(
        texts(&search(&ws, "claude", "alice", "ÉTÉ configurés", 10, Scope::Own).unwrap()),
        expected,
        "la requête aussi est normalisée"
    );
    assert_eq!(
        texts(&search(&ws, "claude", "alice", "INFRASTRUCTURE", 10, Scope::Own).unwrap()),
        expected,
        "tag"
    );
    let _ = std::fs::remove_dir_all(&ws);
}

#[test]
fn search_ranks_by_score_then_recency_and_honours_limit() {
    let ws = ws("tri");
    let now = crate::store::now_ms();
    let day = 86_400_000;
    let entries = [
        memory("a", now - 10 * day, "Le port du serveur est 23090", &[]),
        memory("b", now - 5 * day, "Le port du serveur est 23090", &[]),
        memory("c", now - 400 * day, "Le serveur écoute sur le port 23090, port ouvert", &[]),
        memory("d", now, "Un tout autre sujet", &[]),
        memory("e", now - 2 * day, "Divers", &["port"]),
    ];
    for e in &entries {
        super::store::append_below(&ws, "claude", "alice", e, u64::MAX, usize::MAX).unwrap();
    }
    // Deux termes (2 + récence) pour a, b, c ; le tag de e vaut 2 et il est récent : en tête.
    let hits = search(&ws, "claude", "alice", "port serveur", 10, Scope::Own).unwrap();
    let ids: Vec<&str> = hits.iter().map(|h| h.memory.id.as_str()).collect();
    assert_eq!(
        ids,
        ["e", "b", "a", "c"],
        "{ids:?} : score décroissant, récence à égalité, d sans terme"
    );
    let scores: Vec<f64> = hits.iter().map(|h| h.score).collect();
    assert!(scores.windows(2).all(|w| w[0] > w[1]), "{scores:?}");
    assert_eq!((hits[0].agent.as_str(), hits[0].runner.as_str()), ("alice", "claude"));
    // Un seul terme : le tag (2) devance le texte (1), la récence départage a, b, c.
    let hits = search(&ws, "claude", "alice", "port", 10, Scope::Own).unwrap();
    let ids: Vec<&str> = hits.iter().map(|h| h.memory.id.as_str()).collect();
    assert_eq!(ids, ["e", "b", "a", "c"], "{ids:?}");
    let two = search(&ws, "claude", "alice", "port", 2, Scope::Own).unwrap();
    assert_eq!(two.iter().map(|h| h.memory.id.as_str()).collect::<Vec<_>>(), ["e", "b"]);
    assert!(search(&ws, "claude", "alice", "port", 0, Scope::Own).unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&ws);
}

#[test]
fn a_fresh_memory_outranks_an_old_one_with_the_same_words() {
    let ws = ws("recence");
    let now = crate::store::now_ms();
    let day = 86_400_000;
    for (id, ts) in [("vieux", now - 400 * day), ("neuf", now - day)] {
        let e = memory(id, ts, "préférence : réponses courtes", &[]);
        super::store::append_below(&ws, "claude", "alice", &e, u64::MAX, usize::MAX).unwrap();
    }
    let hits = search(&ws, "claude", "alice", "préférence", 10, Scope::Own).unwrap();
    assert_eq!(hits.iter().map(|h| h.memory.id.as_str()).collect::<Vec<_>>(), ["neuf", "vieux"]);
    assert!(
        hits[0].score - hits[1].score > 0.6,
        "{:?} : un an d'écart pèse lourd",
        hits.iter().map(|h| h.score).collect::<Vec<_>>()
    );
    let _ = std::fs::remove_dir_all(&ws);
}

#[test]
fn compaction_keeps_the_most_recent_under_the_line_budget() {
    let ws = ws("compaction-lignes");
    // Plafond de 10 lignes : le 12e ajout trouve 11 lignes et compacte (5 gardés + l'ajout).
    for i in 1..=12 {
        let e = memory(&format!("m{i:02}"), i as u64, &format!("souvenir {i}"), &[]);
        super::store::append_below(&ws, "claude", "alice", &e, u64::MAX, 10).unwrap();
    }
    let entries = read(&ws, "claude", "alice");
    let ids: Vec<&str> = entries.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(
        ids,
        ["m07", "m08", "m09", "m10", "m11", "m12"],
        "les plus récents, la moitié du budget + l'ajout"
    );
    let _ = std::fs::remove_dir_all(&ws);
}

#[test]
fn compaction_keeps_the_most_recent_under_the_byte_budget_and_drops_junk() {
    let ws = ws("compaction-octets");
    let file = path(&ws, "claude", "alice");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    // Une ligne illisible en tête : elle disparaît à la compaction, sans masquer les autres.
    std::fs::write(&file, "{pas du json\n").unwrap();
    // Chaque souvenir fait ~265 octets ; plafond 600 : le 4e ajout trouve ~800 et compacte
    // à 300 (le seul dernier tient), puis les deux ajouts suivants tiennent sous 600.
    for i in 1..=5 {
        let e = memory(
            &format!("m{i}"),
            i as u64,
            &format!("souvenir numéro {i} {}", "x".repeat(200)),
            &[],
        );
        super::store::append_below(&ws, "claude", "alice", &e, 600, usize::MAX).unwrap();
    }
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(!text.contains("pas du json"), "{text}");
    let entries = read(&ws, "claude", "alice");
    let ids: Vec<&str> = entries.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, ["m3", "m4", "m5"], "{ids:?}");
    assert_eq!(text.lines().count(), 3);
    let _ = std::fs::remove_dir_all(&ws);
}

#[test]
fn compact_alone_keeps_at_least_the_newest_even_if_oversized() {
    let big = memory("big", 1, &"x".repeat(500), &[]);
    let kept = super::store::compact(vec![memory("old", 0, "petit", &[]), big.clone()], 100, 10);
    assert_eq!(kept, [big], "le plus récent reste, même seul au-dessus du budget");
    assert!(super::store::compact(Vec::new(), 100, 10).is_empty());
}

#[test]
fn the_default_budgets_are_the_documented_ones() {
    assert_eq!(super::MAX_BYTES, 1024 * 1024);
    assert_eq!(MAX_LINES, 2000);
    assert_eq!(super::MAX_LIMIT, 100);
    assert_eq!(super::DEFAULT_LIMIT, 10);
    const {
        assert!(super::DEFAULT_LIMIT <= super::MAX_LIMIT);
    };
}

#[test]
fn a_secret_is_refused_with_a_clear_message_and_nothing_is_written() {
    let ws = ws("jetons");
    for text in [
        "ma clé : sk-abc123def456ghi789jkl",
        "API_KEY=abc123def456ghi789",
        "le jeton eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.abcdefghijklmnopqrst",
        "blob QmFzZTY0IGJsb2IgZGUgZG9sbGVzIGV0IGRlIGNoYXRz",
    ] {
        let err = store(&ws, "claude", "alice", text, &[], "mcp").unwrap_err();
        assert!(err.starts_with("SECRET:"), "{err}");
        assert!(err.contains("refusé"), "{err}");
        assert!(
            !err.contains("sk-abc123") && !err.contains("QmFzZTY0"),
            "le jeton n'est jamais répété : {err}"
        );
    }
    // Un jeton dans un tag, pas dans le texte : refusé aussi.
    let err = store(
        &ws,
        "claude",
        "alice",
        "connexion",
        &tags(&["ghp_16C7e42F292c6912E7710c838347Ae178B4a"]),
        "mcp",
    )
    .unwrap_err();
    assert!(err.starts_with("SECRET:"), "{err}");
    assert!(!path(&ws, "claude", "alice").exists(), "rien n'a été écrit");
    // Une référence de variable passe : ce n'est pas un secret.
    store(&ws, "claude", "alice", "API_KEY=$ANTHROPIC_API_KEY dans le .env", &[], "mcp").unwrap();
    assert_eq!(read(&ws, "claude", "alice").len(), 1);
    let _ = std::fs::remove_dir_all(&ws);
}

#[test]
fn empty_too_long_or_badly_tagged_texts_are_refused() {
    let ws = ws("bornes");
    assert!(store(&ws, "claude", "alice", "   ", &[], "mcp").unwrap_err().contains("vide"));
    let err =
        store(&ws, "claude", "alice", &"é".repeat(super::MAX_TEXT + 1), &[], "mcp").unwrap_err();
    assert!(err.contains("trop long"), "{err}");
    store(&ws, "claude", "alice", &"é".repeat(super::MAX_TEXT), &[], "mcp").unwrap();
    let err = store(&ws, "claude", "alice", "ok", &tags(&[&"x".repeat(65)]), "mcp").unwrap_err();
    assert!(err.contains("tag trop long"), "{err}");
    let many: Vec<String> = (0..super::MAX_TAGS + 1).map(|i| format!("t{i}")).collect();
    assert!(
        store(&ws, "claude", "alice", "ok", &many, "mcp").unwrap_err().contains("trop de tags")
    );
    assert_eq!(read(&ws, "claude", "alice").len(), 1, "seul le texte à la limite exacte est passé");
    let _ = std::fs::remove_dir_all(&ws);
}

#[test]
fn forget_removes_only_the_named_memory_and_rewrites_atomically() {
    let ws = ws("oubli");
    let first = store(&ws, "claude", "alice", "un", &[], "mcp").unwrap();
    let second = store(&ws, "claude", "alice", "deux", &[], "cli").unwrap();
    assert!(!forget(&ws, "claude", "alice", "inconnu").unwrap(), "id absent : ok = false");
    assert!(forget(&ws, "claude", "alice", &first.id).unwrap());
    assert_eq!(read(&ws, "claude", "alice"), [second]);
    assert_eq!(std::fs::read_to_string(path(&ws, "claude", "alice")).unwrap().lines().count(), 1);
    let dir = std::fs::read_dir(path(&ws, "claude", "alice").parent().unwrap()).unwrap();
    assert_eq!(dir.count(), 1, "aucun temporaire laissé derrière");
    let _ = std::fs::remove_dir_all(&ws);
}

#[test]
fn swarm_scope_reads_neighbours_read_only_and_excludes_self() {
    let ws = ws("essaim");
    store(&ws, "claude", "alice", "alice sait que le port est 23090", &[], "mcp").unwrap();
    store(&ws, "hermes", "bob", "bob note que le port 23091 répond", &tags(&["port"]), "mcp")
        .unwrap();
    store(&ws, "claude", "carol", "carol : le port 23092 est libre", &[], "cli").unwrap();
    store(&ws, "hermes", "alice", "le port 23093 aussi", &[], "mcp").unwrap();
    // Fichier sans rapport dans le dossier : ignoré.
    std::fs::write(
        path(&ws, "claude", "alice").parent().unwrap().join("notes.jsonl"),
        "du texte\n",
    )
    .unwrap();

    let own = search(&ws, "claude", "alice", "port", 10, Scope::Own).unwrap();
    assert_eq!(
        texts(&own),
        ["alice sait que le port est 23090"],
        "la portée propre ne lit que son fichier"
    );

    let swarm = search(&ws, "claude", "alice", "port", 10, Scope::Swarm).unwrap();
    let who: Vec<(&str, &str)> =
        swarm.iter().map(|h| (h.agent.as_str(), h.memory.text.as_str())).collect();
    assert_eq!(who.len(), 3, "{who:?}");
    assert_eq!(
        who[0],
        ("bob", "bob note que le port 23091 répond"),
        "le tag compte double : bob en tête"
    );
    assert!(who.iter().any(|(a, t)| *a == "carol" && t.contains("23092")));
    assert!(
        who.iter().any(|(a, t)| *a == "alice" && t.contains("23093")),
        "l'autre runner du même nom est un voisin"
    );
    assert!(
        swarm.iter().all(|h| h.memory.text != "alice sait que le port est 23090"),
        "jamais son propre fichier"
    );
    assert_eq!(swarm[0].runner, "hermes");
    // Lecture seule : rien n'a changé sur disque.
    assert_eq!(read(&ws, "claude", "alice").len(), 1);
    assert_eq!(read(&ws, "hermes", "bob").len(), 1);
    assert!(!path(&ws, "claude", "bob").exists());
    // Un agent sans fichier mémoire ne se lit pas lui-même : ses voisins, et rien d'autre.
    let nobody = search(&ws, "claude", "personne", "port", 10, Scope::Swarm).unwrap();
    assert!(
        nobody.iter().all(|h| h.agent != "personne"),
        "pas de ligne de l'agent demandeur, qui n'a pas de fichier"
    );
    assert_eq!(nobody.len(), 4, "les quatre autres fichiers sont lus, en lecture seule");
    assert!(!path(&ws, "claude", "personne").exists(), "rien d'écrit pour lui");
    let _ = std::fs::remove_dir_all(&ws);
}

#[test]
fn path_of_a_memory_file_is_under_bridge_memory() {
    assert_eq!(
        super::path(Path::new("ws"), "antigravity", "trade_2"),
        Path::new("ws").join("bridge/memory/antigravity-trade_2.jsonl")
    );
}
