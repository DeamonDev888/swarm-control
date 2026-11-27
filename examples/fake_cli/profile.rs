//! Gestion des profils Hermes (`profile create|delete`, `config set`).

use crate::args::Args;
use std::path::{Path, PathBuf};

/// Traite les sous-commandes d'administration ; `true` si la commande est finie.
pub fn handle(args: &Args) -> bool {
    if args.has("profile") {
        profile_command(args);
        return true;
    }
    if args.has("config") && args.has("set") {
        config_set(args);
        return true;
    }
    false
}

fn profile_dir(args: &Args, name: &str) -> PathBuf {
    Path::new(&args.hermes_home).join("profiles").join(name)
}

fn profile_command(args: &Args) {
    let skip = ["profile", "create", "delete", "show"];
    let name = args
        .list
        .iter()
        .find(|a| !a.starts_with('-') && !skip.contains(&a.as_str()))
        .cloned()
        .unwrap_or_default();
    let dir = profile_dir(args, &name);
    if args.has("create") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.yaml"), "model:\n  default: none\n").unwrap();
        clone_env(args, &dir);
    } else if args.has("delete") {
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// `--clone-from SRC` copie le `.env` (là où vivent les clés d'un profil).
fn clone_env(args: &Args, dir: &Path) {
    let Some(src) = args.after("--clone-from") else { return };
    let from = profile_dir(args, &src).join(".env");
    if from.is_file() {
        std::fs::copy(from, dir.join(".env")).unwrap();
    }
}

fn config_set(args: &Args) {
    let pos = args.list.iter().position(|a| a == "set").unwrap();
    let (key, value) = (&args.list[pos + 1], &args.list[pos + 2]);
    let name = args.after("-p").unwrap_or_default();
    let path = profile_dir(args, &name).join("config.yaml");
    let mut content = std::fs::read_to_string(&path).unwrap_or_default();
    content.push_str(&format!("# {key}={value}\n"));
    if key == "model.default" {
        content = format!("model:\n  default: {value}\n");
    }
    std::fs::write(path, content).unwrap();
}
