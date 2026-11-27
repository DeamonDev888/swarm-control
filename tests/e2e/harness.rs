//! Démarrage de serveurs jetables (un, ou plusieurs sur le même workspace).

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

static COUNTER: AtomicU32 = AtomicU32::new(0);

pub struct Harness {
    child: Child,
    pub port: u16,
    root: PathBuf,
    pub ws: PathBuf,
    pub hermes_home: PathBuf,
    /// Seule la première instance nettoie le dossier partagé.
    owns_root: bool,
}

impl Harness {
    /// Annonce LiteMCP d'un serveur de ce harnais (`TEMP` est isolé par test).
    pub fn announce_file(&self, name: &str) -> PathBuf {
        self.root.join("litemcp-registry").join(format!("{name}.json"))
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if self.owns_root {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
}

pub fn fake_cli() -> PathBuf {
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_swarm-control"));
    let name = if cfg!(windows) { "fake_cli.exe" } else { "fake_cli" };
    let path = exe.parent().unwrap().join("examples").join(name);
    assert!(path.exists(), "faux CLI absent ({}): lance `cargo test`", path.display());
    path
}

/// Démarre un serveur dans un workspace neuf.
pub fn start(extra_env: &[(&str, &str)]) -> Harness {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let root = std::env::temp_dir().join(format!("swarm-e2e-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let ws = root.join("ws");
    let hermes_home = root.join("hermes");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::create_dir_all(hermes_home.join("profiles")).unwrap();
    // Serveurs MCP connus du workspace.
    std::fs::write(
        ws.join(".mcp.json"),
        r#"{"mcpServers":{"memory":{"url":"http://127.0.0.1:3099/mcp"},"db":{"command":"node","args":["db.js"]}}}"#,
    )
    .unwrap();
    launch(root, ws, hermes_home, extra_env, &[], true)
}

/// Démarre une **autre instance** sur le workspace de `first` (même agents, mêmes fichiers).
pub fn start_sharing(first: &Harness, extra_env: &[(&str, &str)]) -> Harness {
    start_sharing_with_args(first, extra_env, &[])
}

/// Comme [`start_sharing`], avec des arguments de ligne de commande (ex. `--resident alice`).
pub fn start_sharing_with_args(
    first: &Harness,
    extra_env: &[(&str, &str)],
    args: &[&str],
) -> Harness {
    launch(first.root.clone(), first.ws.clone(), first.hermes_home.clone(), extra_env, args, false)
}

/// Commande `swarm-control` avec l'environnement isolé du harnais.
pub fn command(root: &Path, ws: &Path, hermes_home: &Path) -> Command {
    let cli = fake_cli();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_swarm-control"));
    cmd.args(["--port", "0", "--workspace"])
        .arg(ws)
        .env("USERPROFILE", root)
        .env("HOME", root)
        // Le registre d'annonces LiteMCP vit sous TEMP : isolé, il ne mélange pas les tests.
        .env("TEMP", root)
        .env("TMP", root)
        .env("TMPDIR", root)
        .env("HERMES_HOME", hermes_home)
        .env("CLAUDE_BIN", &cli)
        .env("HERMES_BIN", &cli)
        .env("ANTHROPIC_AUTH_TOKEN", "primary-token")
        .env_remove("SWARM_WORKSPACE")
        .env_remove("OVERMIND_WORKSPACE");
    cmd
}

fn launch(
    root: PathBuf,
    ws: PathBuf,
    hermes_home: PathBuf,
    extra_env: &[(&str, &str)],
    args: &[&str],
    owns_root: bool,
) -> Harness {
    let mut cmd = command(&root, &ws, &hermes_home);
    cmd.args(args).stdout(Stdio::null()).stderr(Stdio::piped());
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().expect("lancement du serveur");
    let mut reader = BufReader::new(child.stderr.take().unwrap());
    let port = read_port(&mut reader);
    // Vide stderr en arrière-plan pour que le serveur ne bloque jamais dessus.
    std::thread::spawn(move || {
        let mut sink = Vec::new();
        let _ = reader.read_to_end(&mut sink);
    });
    Harness { child, port, root, ws, hermes_home, owns_root }
}

/// Port du serveur d'administration, lu dans sa bannière (`… MCP sur http://127.0.0.1:PORT/mcp …`).
/// Les lignes « agent résident … → http://… » qui la précèdent ne sont pas la bannière.
fn read_port(reader: &mut impl BufRead) -> u16 {
    const MARK: &str = "MCP sur http://127.0.0.1:";
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut line = String::new();
    while Instant::now() < deadline {
        line.clear();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            break;
        }
        if let Some(rest) = line.split(MARK).nth(1) {
            return rest.split('/').next().unwrap().parse().unwrap();
        }
    }
    panic!("le serveur n'a pas annoncé son port");
}
