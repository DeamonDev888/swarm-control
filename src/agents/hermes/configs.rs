//! Lecture des fichiers d'un profil Hermes.

use crate::agents::files::read_or_missing;
use crate::agents::paths::hermes_profile_dir;
use crate::agents::secrets::mask_env;

/// Les secrets du `.env` sont masqués : seuls les noms de variables sont exposés.
pub fn read(name: &str) -> Vec<(String, String)> {
    let dir = hermes_profile_dir(name);
    vec![
        ("settings.json".into(), read_or_missing(&dir.join("config.yaml"))),
        ("prompt.md".into(), read_or_missing(&dir.join("SOUL.md"))),
        (".mcp.json".into(), read_or_missing(&dir.join("mcp.json"))),
        (".env".into(), mask_env(&read_or_missing(&dir.join(".env")))),
    ]
}
