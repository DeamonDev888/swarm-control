//! Tests du journal : écriture, relecture, filtre et rotation.

use super::{Entry, append, fingerprint, path, read};
use std::path::PathBuf;

fn ws(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("swarm-history-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn entry(ts: u64, prompt: &str) -> Entry {
    Entry {
        ts,
        prompt: prompt.into(),
        reply: format!("echo:{prompt}"),
        session_id: Some("s1".into()),
        duration_ms: 5,
        queued_ms: 1,
        runner: "claude".into(),
        agent: "alice".into(),
        ..Default::default()
    }
}

#[test]
fn appends_and_reads_back_in_order() {
    let ws = ws("ordre");
    for (i, p) in ["un", "deux", "trois"].iter().enumerate() {
        append(&ws, "claude", "alice", &entry(i as u64 + 1, p)).unwrap();
    }
    let back = read(&ws, "claude", "alice", 10, None).unwrap();
    let prompts: Vec<&str> = back.iter().map(|e| e.prompt.as_str()).collect();
    assert_eq!(prompts, ["un", "deux", "trois"]);
    assert_eq!(back[2].session_id.as_deref(), Some("s1"));
    assert!(path(&ws, "claude", "alice").is_file());
    let _ = std::fs::remove_dir_all(&ws);
}

#[test]
fn limit_keeps_the_most_recent_and_before_pages_backwards() {
    let ws = ws("limite");
    for i in 1..=5u64 {
        append(&ws, "claude", "alice", &entry(i, &format!("t{i}"))).unwrap();
    }
    let last2 = read(&ws, "claude", "alice", 2, None).unwrap();
    assert_eq!(last2.iter().map(|e| e.ts).collect::<Vec<_>>(), [4, 5]);
    let before = read(&ws, "claude", "alice", 10, Some(4)).unwrap();
    assert_eq!(before.iter().map(|e| e.ts).collect::<Vec<_>>(), [1, 2, 3]);
    let _ = std::fs::remove_dir_all(&ws);
}

#[test]
fn a_corrupt_line_does_not_hide_the_others() {
    let ws = ws("corrompu");
    append(&ws, "claude", "alice", &entry(1, "bon")).unwrap();
    let file = path(&ws, "claude", "alice");
    let mut text = std::fs::read_to_string(&file).unwrap();
    text.push_str("{tronqué\n");
    std::fs::write(&file, text).unwrap();
    append(&ws, "claude", "alice", &entry(2, "encore")).unwrap();
    let back = read(&ws, "claude", "alice", 10, None).unwrap();
    assert_eq!(back.len(), 2);
    assert_eq!(back[1].prompt, "encore");
    let _ = std::fs::remove_dir_all(&ws);
}

#[test]
fn rotation_keeps_one_previous_generation() {
    let ws = ws("rotation");
    let file = path(&ws, "claude", "alice");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    // Un journal au-delà de la limite (1 Kio pour ce test) est tourné avant l'ajout.
    let big = format!("{}\n", "x".repeat(2048));
    std::fs::write(&file, &big).unwrap();
    assert_eq!(std::fs::metadata(&file).unwrap().len(), 2049);
    super::write::append_below(&ws, "claude", "alice", &entry(9, "après"), 1024).unwrap();
    let rotated = file.with_extension("jsonl.1");
    assert!(rotated.is_file(), "génération précédente absente");
    assert!(std::fs::metadata(&file).unwrap().len() < 2048);
    let back = read(&ws, "claude", "alice", 10, None).unwrap();
    assert_eq!(back.last().unwrap().prompt, "après");
    let _ = std::fs::remove_dir_all(&ws);
}

#[test]
fn a_missing_journal_reads_empty_and_fingerprints_to_zero() {
    let ws = ws("absent");
    assert!(read(&ws, "claude", "alice", 10, None).unwrap().is_empty());
    assert_eq!(fingerprint(&ws, "claude", "alice"), (0, 0));
    let _ = std::fs::remove_dir_all(&ws);
}

#[test]
fn the_fingerprint_follows_the_file() {
    let ws = ws("empreinte");
    append(&ws, "claude", "alice", &entry(1, "un")).unwrap();
    let (len, mtime) = fingerprint(&ws, "claude", "alice");
    assert!(len > 0, "taille {len}");
    assert!(mtime > 0, "mtime {mtime}");
    assert_eq!(len, path(&ws, "claude", "alice").metadata().unwrap().len());
    let _ = std::fs::remove_dir_all(&ws);
}

#[test]
fn empty_optional_fields_are_omitted_and_errors_round_trip() {
    let ws = ws("champs");
    let failed =
        Entry { error: Some("API Error: 401".into()), session_id: None, ..entry(1, "raté") };
    assert!(!failed.ok());
    append(&ws, "claude", "alice", &failed).unwrap();
    let line = std::fs::read_to_string(path(&ws, "claude", "alice")).unwrap();
    // camelCase et champs vides absents : pas de `from`/`via`/`sessionId` dans la ligne.
    assert!(line.contains("\"error\":\"API Error: 401\""), "{line}");
    assert!(line.contains("\"durationMs\":"), "{line}");
    assert!(!line.contains("\"from\""), "{line}");
    assert!(!line.contains("\"via\""), "{line}");
    assert!(!line.contains("\"sessionId\""), "{line}");
    let back = read(&ws, "claude", "alice", 10, None).unwrap();
    assert_eq!(back[0].error.as_deref(), Some("API Error: 401"));
    assert_eq!(back[0].from, None);
    assert!(back[0].via.is_empty());
    assert_eq!(back[0].session_id, None);
    let _ = std::fs::remove_dir_all(&ws);
}

#[test]
fn rotation_preserves_the_previous_generation_across_reads() {
    let ws = ws("générations");
    // Deux tours, puis dépassement : la rotation doit conserver la ligne tournée.
    append(&ws, "claude", "alice", &entry(1, "premier")).unwrap();
    append(&ws, "claude", "alice", &entry(2, "deuxième")).unwrap();
    super::write::append_below(&ws, "claude", "alice", &entry(3, "troisième"), 1).unwrap();
    let rotated = path(&ws, "claude", "alice").with_extension("jsonl.1");
    let rotated_text = std::fs::read_to_string(&rotated).unwrap();
    assert!(
        rotated_text.contains("premier") && rotated_text.contains("deuxième"),
        "{rotated_text}"
    );
    // La lecture traverse les deux générations, du plus ancien au plus récent.
    let back = read(&ws, "claude", "alice", 10, None).unwrap();
    let prompts: Vec<&str> = back.iter().map(|e| e.prompt.as_str()).collect();
    assert_eq!(prompts, ["premier", "deuxième", "troisième"]);
    // Le filtre `before` s'applique aux deux générations.
    let before = read(&ws, "claude", "alice", 10, Some(2)).unwrap();
    assert_eq!(before.iter().map(|e| e.ts).collect::<Vec<_>>(), [1]);
    let _ = std::fs::remove_dir_all(&ws);
}
