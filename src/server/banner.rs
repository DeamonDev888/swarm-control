//! Annonce de démarrage (stderr).

use std::path::Path;

/// Une ligne contenant l'URL MCP : lue par les tests et par les humains.
pub fn announce(name: &str, addr: &str, workspace: &Path) {
    eprintln!(
        "{name} {} — MCP sur http://{addr}/mcp  (workspace: {})",
        env!("CARGO_PKG_VERSION"),
        workspace.display()
    );
}
