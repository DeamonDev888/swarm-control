//! Analyse des arguments de la ligne de commande.

use std::path::PathBuf;

pub struct Options {
    /// Port d'écoute ; `None` = premier port libre de 23080..=23099.
    pub port: Option<u16>,
    pub host: String,
    pub workspace: Option<PathBuf>,
    /// Agents à rendre résidents au démarrage (`--resident alice,bob`, répétable).
    pub residents: Vec<String>,
    /// Numéro d'instance (`--instance N`) ; `None` = premier libre.
    pub instance: Option<u32>,
}

pub enum Parsed {
    Serve(Options),
    Help,
    Version,
}

pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Parsed, String> {
    let mut options = Options {
        port: None,
        host: "127.0.0.1".into(),
        workspace: None,
        residents: Vec::new(),
        instance: None,
    };
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--port" | "-p" => options.port = Some(parse_port(&value(&mut args, "--port")?)?),
            "--host" => options.host = value(&mut args, "--host")?,
            "--workspace" | "-w" => {
                options.workspace = Some(PathBuf::from(value(&mut args, "--workspace")?));
            }
            "--resident" | "-r" => {
                options.residents.extend(names(&value(&mut args, "--resident")?))
            }
            "--instance" | "-i" => {
                options.instance = Some(
                    value(&mut args, "--instance")?
                        .parse::<u32>()
                        .ok()
                        .filter(|n| *n > 0)
                        .ok_or("--instance: entier ≥ 1 attendu")?,
                );
            }
            "--help" | "-h" => return Ok(Parsed::Help),
            "--version" | "-V" => return Ok(Parsed::Version),
            other => return Err(format!("option inconnue: {other} (voir --help)")),
        }
    }
    Ok(Parsed::Serve(options))
}

fn value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    args.next().ok_or_else(|| format!("{flag} attend une valeur"))
}

fn parse_port(raw: &str) -> Result<u16, String> {
    raw.parse().map_err(|_| "--port: nombre 0-65535 attendu".to_string())
}

/// `alice,bob` → `["alice", "bob"]` (vides ignorés).
fn names(raw: &str) -> Vec<String> {
    raw.split(',').map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect()
}

#[cfg(test)]
mod tests {
    use super::{Parsed, parse};

    fn run(args: &[&str]) -> Result<Parsed, String> {
        parse(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn defaults_to_loopback_without_port() {
        let Ok(Parsed::Serve(o)) = run(&[]) else { panic!("serve attendu") };
        assert_eq!((o.host.as_str(), o.port), ("127.0.0.1", None));
        assert!(o.residents.is_empty());
    }

    #[test]
    fn reads_port_host_and_workspace() {
        let Ok(Parsed::Serve(o)) = run(&["-p", "0", "--host", "0.0.0.0", "-w", "ws"]) else {
            panic!("serve attendu")
        };
        assert_eq!((o.port, o.host.as_str()), (Some(0), "0.0.0.0"));
        assert_eq!(o.workspace.unwrap().to_str(), Some("ws"));
    }

    #[test]
    fn resident_accepts_commas_and_repetition() {
        let Ok(Parsed::Serve(o)) = run(&["--resident", "alice, bob", "-r", "carol,,"]) else {
            panic!("serve attendu")
        };
        assert_eq!(o.residents, ["alice", "bob", "carol"]);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(run(&["--port"]).is_err());
        assert!(run(&["--port", "abc"]).is_err());
        assert!(run(&["--resident"]).is_err());
        assert!(run(&["--nope"]).is_err());
        assert!(run(&["--instance", "0"]).is_err());
        assert!(run(&["--instance", "x"]).is_err());
        let Ok(Parsed::Serve(o)) = run(&["-i", "3"]) else { panic!("serve attendu") };
        assert_eq!(o.instance, Some(3));
        assert!(matches!(run(&["-V"]), Ok(Parsed::Version)));
        assert!(matches!(run(&["--help"]), Ok(Parsed::Help)));
    }
}
