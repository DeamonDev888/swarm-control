use super::entry::TTL_MS;
use super::{delete, get, legacy, save};
use crate::store::{now_ms, write_json_atomic};
use serde_json::json;
use std::path::PathBuf;

fn temp_ws(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("swarm-control-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn save_get_delete_roundtrip() {
    let ws = temp_ws("sessions");
    assert_eq!(get(&ws, "claude", "a"), None);
    save(&ws, "claude", "a", "s1").unwrap();
    save(&ws, "hermes", "a", "h1").unwrap();
    assert_eq!(get(&ws, "claude", "a").as_deref(), Some("s1"));
    assert_eq!(get(&ws, "hermes", "a").as_deref(), Some("h1"));
    assert!(delete(&ws, "claude", "a"));
    assert_eq!(get(&ws, "claude", "a"), None);
    assert_eq!(get(&ws, "hermes", "a").as_deref(), Some("h1"));
    assert!(!delete(&ws, "claude", "a"));
    let _ = std::fs::remove_dir_all(&ws);
}

#[test]
fn reads_the_legacy_file_and_skips_expired_entries() {
    let ws = temp_ws("legacy");
    let old = now_ms() - TTL_MS - 1;
    write_json_atomic(
        &legacy::path(&ws),
        &json!({ "bare": "legacy-id", "claude:old": { "id": "x", "ts": old } }),
    )
    .unwrap();
    assert_eq!(get(&ws, "claude", "bare").as_deref(), Some("legacy-id"));
    assert_eq!(get(&ws, "claude", "old"), None);
    let _ = std::fs::remove_dir_all(&ws);
}

#[test]
fn state_file_wins_over_the_legacy_file_and_legacy_is_never_written_by_save() {
    let ws = temp_ws("priority");
    write_json_atomic(&legacy::path(&ws), &json!({ "claude:a": { "id": "old", "ts": now_ms() } }))
        .unwrap();
    assert_eq!(get(&ws, "claude", "a").as_deref(), Some("old"));
    save(&ws, "claude", "a", "new").unwrap();
    assert_eq!(get(&ws, "claude", "a").as_deref(), Some("new"));
    let untouched = std::fs::read_to_string(legacy::path(&ws)).unwrap();
    assert!(untouched.contains("\"old\"") && !untouched.contains("\"new\""));
    // L'oubli retire aussi l'entrée héritée : la session ne « ressuscite » pas.
    assert!(delete(&ws, "claude", "a"));
    assert_eq!(get(&ws, "claude", "a"), None);
    let _ = std::fs::remove_dir_all(&ws);
}
