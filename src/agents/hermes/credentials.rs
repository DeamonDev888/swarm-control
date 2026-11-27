//! Les credentials Hermes sont PAR PROFIL : détection heuristique.

use crate::store::read_json;
use std::path::Path;

/// Le profil a-t-il une credential pour `provider` ? (pool de `auth.json`,
/// entrée `providers`, ou un `.env` non vide.)
pub fn has_credentials(profile_dir: &Path, provider: &str) -> bool {
    in_auth_file(profile_dir, provider) || has_env_entries(profile_dir)
}

fn in_auth_file(profile_dir: &Path, provider: &str) -> bool {
    let Some(auth) = read_json(&profile_dir.join("auth.json")) else {
        return false;
    };
    let in_pool = auth
        .pointer(&format!("/credential_pool/{provider}"))
        .and_then(serde_json::Value::as_array)
        .is_some_and(|l| !l.is_empty());
    in_pool || auth.pointer(&format!("/providers/{provider}")).is_some()
}

fn has_env_entries(profile_dir: &Path) -> bool {
    std::fs::read_to_string(profile_dir.join(".env"))
        .map(|c| c.lines().any(|l| l.contains('=') && !l.trim_start().starts_with('#')))
        .unwrap_or(false)
}
