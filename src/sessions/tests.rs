use super::{delete, get, save};
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
