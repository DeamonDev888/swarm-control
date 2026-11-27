//! Création d'un profil Hermes (via le CLI) et de son `SOUL.md`.

use super::cli;
use super::credentials::has_credentials;
use super::mcp::set_hermes_mcp;
use super::provider::detect_provider;
use crate::agents::files::io;
use crate::agents::lookup::hermes_profile_exists;
use crate::agents::mcp::mcp_server_names;
use crate::agents::name::validate_hermes_name;
use crate::agents::paths::hermes_profile_dir;
use crate::agents::{CreateArgs, Created};
use std::path::Path;

const DEFAULT_MODEL: &str = "MiniMax-M3";

pub fn create(ws: &Path, args: &CreateArgs) -> Result<Created, String> {
    validate_hermes_name(args.name)?;
    check_clone_source(args.clone_from)?;
    let model = args.model.unwrap_or(DEFAULT_MODEL);
    let provider = detect_provider(model);

    create_profile(args)?;
    // Ordre important : `model.default` puis `model.provider` (provider en dernier).
    cli::run(&["-p", args.name, "config", "set", "model.default", model])?;
    cli::run(&["-p", args.name, "config", "set", "model.provider", provider])?;

    let dir = hermes_profile_dir(args.name);
    if !dir.is_dir() {
        return Err(format!(
            "Profil Hermes introuvable après création ({}). HERMES_HOME pointe-t-il au bon endroit ?",
            dir.display()
        ));
    }
    let soul = dir.join("SOUL.md");
    std::fs::write(&soul, args.prompt).map_err(io)?;
    let servers = mcp_server_names(ws);
    if !servers.is_empty() {
        let _ = set_hermes_mcp(ws, &dir, &servers);
    }
    Ok(Created {
        prompt_path: soul,
        config_path: dir.join("config.yaml"),
        warnings: credential_warnings(&dir, provider, args.name),
    })
}

fn check_clone_source(clone_from: Option<&str>) -> Result<(), String> {
    let Some(src) = clone_from else { return Ok(()) };
    validate_hermes_name(src)?;
    if hermes_profile_exists(src) {
        Ok(())
    } else {
        Err(format!("cloneFrom: le profil Hermes '{src}' n'existe pas."))
    }
}

/// `hermes profile create`, avec `--clone-from` si demandé ; « déjà existant » toléré.
fn create_profile(args: &CreateArgs) -> Result<(), String> {
    let description = format!("swarm-control agent: {}", args.name);
    let mut cmd: Vec<&str> =
        vec!["profile", "create", args.name, "--no-alias", "--description", &description];
    if let Some(src) = args.clone_from {
        cmd.extend(["--clone-from", src]);
    }
    match cli::run(&cmd) {
        Err(e) if !e.contains("already exists") => {
            Err(format!("hermes profile create a échoué: {e}"))
        }
        _ => Ok(()),
    }
}

fn credential_warnings(dir: &Path, provider: &str, name: &str) -> Vec<String> {
    if has_credentials(dir, provider) {
        return Vec::new();
    }
    vec![format!(
        "Aucune credential détectée pour le provider '{provider}' dans ce profil (les credentials \
         Hermes sont PAR PROFIL) : les runs échoueront avec « No usable credentials found ». \
         Ajoute-en une : `hermes -p {name} auth add {provider}`, ou recrée l'agent avec \
         cloneFrom:<profil qui a déjà ses clés>."
    )]
}
