//! Sous-commandes d'agent et d'instance, et `--debug` (PLAN §4.2, TODO §3).
//!
//! Le vrai binaire avec le faux CLI : une sous-commande d'agent parle au serveur de
//! l'agent quand il tourne (annonce LiteMCP vivante), sinon exécute le tour elle-même
//! (bail de run pris puis libéré, même session reprise et sauvegardée).

use crate::harness::{Harness, agent_exe, fake_cli, start, start_agent};
use crate::rpc::ok;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Lance un exécutable swarm-control en sous-commande, dans l'environnement du harnais.
/// Rend `(code, stdout, stderr)`.
fn cli(h: &Harness, exe: &Path, args: &[&str]) -> (i32, String, String) {
    let output = command(h, exe).args(args).output().expect("lancement de la CLI");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// La commande prête : workspace du harnais, faux CLI, `TEMP` isolé (annonces LiteMCP).
fn command(h: &Harness, exe: &Path) -> Command {
    // `root` est privé dans le harnais : le dossier qui contient `ws` EST la racine isolée.
    let root = h.ws.parent().expect("ws sous la racine du harnais").to_path_buf();
    let mut cmd = Command::new(exe);
    cmd.arg("--workspace").arg(&h.ws);
    for (key, value) in [
        ("USERPROFILE", root.clone()),
        ("HOME", root.clone()),
        ("TEMP", root.clone()),
        ("TMP", root.clone()),
        ("TMPDIR", root.clone()),
        ("HERMES_HOME", h.hermes_home.clone()),
        ("CLAUDE_BIN", fake_cli()),
        ("HERMES_BIN", fake_cli()),
        ("AGY_BIN", fake_cli()),
        ("ANTHROPIC_AUTH_TOKEN", PathBuf::from("primary-token")),
    ] {
        cmd.env(key, value);
    }
    cmd.env_remove("SWARM_WORKSPACE").env_remove("OVERMIND_WORKSPACE").env_remove("SWARM_DEBUG");
    cmd
}

/// Sous-commande de l'instance `swarm-control`.
fn control(h: &Harness, args: &[&str]) -> (i32, String, String) {
    cli(h, Path::new(env!("CARGO_BIN_EXE_swarm-control")), args)
}

fn json_out(out: &str) -> Value {
    serde_json::from_str(out).unwrap_or_else(|e| panic!("JSON attendu ({e}): {out}"))
}

/// Le bail de run de l'agent (`bridge/owners/<runner>-<agent>.json`) existe-t-il ?
fn lease_exists(h: &Harness, runner: &str, agent: &str) -> bool {
    h.ws.join("bridge").join("owners").join(format!("{runner}-{agent}.json")).exists()
}

#[test]
fn agent_subcommands_serve_the_running_agent() {
    let h = start(&[]);
    ok(&h, "create_agent", json!({"name": "alice", "prompt": "Tu es Alice."}));
    // Depuis la 0.2.0, seul `--agent NAME` sur le vrai binaire sert un agent : une copie
    // renommée (`alice.exe`) est refusée avant tout parsing, `--agent` compris (voir
    // `a_renamed_binary_is_refused_with_the_migration_message`).
    let _server =
        start_agent(&h, Path::new(env!("CARGO_BIN_EXE_swarm-control")), &["--agent", "alice"]);

    // ask : passe par le serveur de l'agent (réponse = le texte du tour).
    let (code, out, err) = control(&h, &["--agent", "alice", "ask", "bonjour"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("echo:bonjour"), "{out}");

    let (code, _, err) = control(&h, &["--agent", "alice", "ask", "et re ?"]);
    assert_eq!(code, 0, "{err}");

    // Même session au tour suivant : le statut du serveur donne la session gardée.
    let (code, out, err) = control(&h, &["--agent", "alice", "status", "--json"]);
    assert_eq!(code, 0, "{err}");
    let status = json_out(&out);
    assert_eq!(status["state"], json!("idle"), "{status}");
    assert_eq!(status["turnsDone"], json!(2), "{status}");
    let session = status["session"].as_str().expect("session gardée").to_string();

    // L'identifiant de session revient dans la réponse du tour suivant.
    let (_, out, _) = control(&h, &["--agent", "alice", "ask", "encore"]);
    assert!(out.contains(&format!("resumed:{session}")), "{out}");

    // history : les trois tours du journal.
    let (code, out, err) = control(&h, &["--agent", "alice", "history", "-n", "5", "--json"]);
    assert_eq!(code, 0, "{err}");
    let journal = json_out(&out);
    assert_eq!(journal["turns"].as_array().unwrap().len(), 3, "{journal}");
    assert_eq!(journal["agent"], json!("alice"), "{journal}");

    // config : les fichiers de l'agent, secrets masqués.
    let (code, out, err) = control(&h, &["--agent", "alice", "config", "--json"]);
    assert_eq!(code, 0, "{err}");
    let files = json_out(&out);
    assert!(!files.as_object().unwrap().is_empty(), "{files}");

    // schema : le schéma d'entrée de l'outil `ask`, lu sur le serveur qui tourne,
    // et l'erreur claire pour un outil inconnu.
    let (code, out, err) = control(&h, &["--agent", "alice", "schema", "ask"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("prompt"), "{out}");
    let (code, _, err) = control(&h, &["--agent", "alice", "schema", "magie"]);
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("magie"), "{err}");

    // reset : la session est oubliée, le tour suivant repart à neuf.
    let (code, out, err) = control(&h, &["--agent", "alice", "reset"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(json_out(&out), json!({"agent": "alice", "hadSession": true}), "{out}");
    let (code, out, err) = control(&h, &["--agent", "alice", "ask", "on recommence"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("resumed:none"), "{out}");
}

#[test]
fn without_a_server_the_turn_runs_here_with_lease_and_session() {
    let h = start(&[]);
    ok(&h, "create_agent", json!({"name": "bob", "prompt": "Tu es Bob."}));
    // Aucun serveur pour bob : le tour s'exécute dans la CLI elle-même.
    let (code, out, err) = control(&h, &["--agent", "bob", "ask", "salut"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("echo:salut"), "{out}");
    assert!(!lease_exists(&h, "claude", "bob"), "bail de run non libéré");

    // La session est gardée : le tour suivant la reprend.
    let (code, out, err) = control(&h, &["--agent", "bob", "status", "--json"]);
    assert_eq!(code, 0, "{err}");
    let status = json_out(&out);
    assert_eq!(status["resident"], json!(false), "{status}");
    let session = status["session"].as_str().expect("session gardée").to_string();
    let (_, out, _) = control(&h, &["--agent", "bob", "ask", "encore"]);
    assert!(out.contains(&format!("resumed:{session}")), "{out}");

    // Le dernier run est connu du disque, même sans serveur.
    let (code, out, err) = control(&h, &["--agent", "bob", "status", "--json"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(json_out(&out)["lastRun"]["status"], json!("done"), "{out}");

    // Chaque tour local laisse une ligne au journal, comme côté résident : mêmes champs,
    // écrits par la CLI elle-même (from = cette instance, queuedMs = 0).
    let (code, out, err) = control(&h, &["--agent", "bob", "history", "-n", "5", "--json"]);
    assert_eq!(code, 0, "{err}");
    let journal = json_out(&out);
    assert_eq!(journal["agent"], json!("bob"), "{journal}");
    let turns = journal["turns"].as_array().unwrap();
    assert_eq!(turns.len(), 2, "{journal}");
    let salut = turns.iter().find(|t| t["prompt"] == json!("salut")).expect("tour « salut »");
    assert!(salut["reply"].as_str().unwrap().contains("echo:salut"), "{journal}");
    assert!(salut["error"].is_null(), "{journal}");
    assert_eq!(salut["sessionId"], json!(session), "{journal}");
    assert_eq!(salut["queuedMs"], json!(0), "{journal}");
    assert!(salut["durationMs"].as_u64().is_some(), "{journal}");
    assert_eq!(salut["runner"], json!("claude"), "{journal}");
    assert_eq!(salut["agent"], json!("bob"), "{journal}");
    assert!(salut["from"].as_str().unwrap().starts_with("swarm-control@"), "{journal}");
}

#[test]
fn debug_prints_one_line_per_event() {
    let h = start(&[]);
    ok(&h, "create_agent", json!({"name": "carl", "prompt": "Tu es Carl."}));

    // Tour local : bail pris, commande lancée (sans le jeton), session neuve.
    let (code, _, err) = control(&h, &["--agent", "carl", "ask", "un", "--debug"]);
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("[debug] bail pris"), "{err}");
    assert!(err.contains("[debug] commande"), "{err}");
    assert!(err.contains("[debug] run carl (claude): session neuve"), "{err}");
    assert!(!err.contains("primary-token"), "jeton imprimé : {err}");

    // Deuxième tour : la session est reprise, une ligne le dit.
    let (code, _, err) = control(&h, &["--agent", "carl", "ask", "deux", "--debug"]);
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("reprise de la session"), "{err}");

    // Tour via le serveur : une ligne par requête MCP côté client…
    let _server =
        start_agent(&h, Path::new(env!("CARGO_BIN_EXE_swarm-control")), &["--agent", "carl"]);
    let (code, _, err) = control(&h, &["--agent", "carl", "ask", "via serveur", "--debug"]);
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("[debug] mcp → tools/call ask"), "{err}");
    // …et une ligne par requête reçue, dans le journal du serveur (non lu ici).
}

#[test]
fn instance_subcommands_report_and_run() {
    let h = start(&[]);
    ok(&h, "create_agent", json!({"name": "dana", "prompt": "Tu es Dana."}));

    // agents : la liste du disque, sans résident.
    let (code, out, err) = control(&h, &["agents", "--json"]);
    assert_eq!(code, 0, "{err}");
    let listed = json_out(&out);
    assert_eq!(listed[0]["agent"], json!("dana"), "{listed}");
    assert_eq!(listed[0]["resident"], json!(false), "{listed}");

    // run : un tour complet pour un agent nommé.
    let (code, out, err) = control(&h, &["run", "dana", "coucou"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("echo:coucou"), "{out}");

    // runs : le dernier run de dana, lu dans son fichier d'état.
    let (code, out, err) = control(&h, &["runs", "--json"]);
    assert_eq!(code, 0, "{err}");
    let runs = json_out(&out);
    let dana = runs.as_array().unwrap().iter().find(|d| d["agent"] == json!("dana")).unwrap();
    assert_eq!(dana["lastRun"]["status"], json!("done"), "{dana}");

    // doctor : workspace sain → tout vert, code 0.
    let (code, out, err) = control(&h, &["doctor"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("✓ workspace"), "{out}");
    assert!(out.contains("0 ✗"), "{out}");
}

#[test]
fn bad_invocations_fail_with_clear_messages() {
    let h = start(&[]);
    ok(&h, "create_agent", json!({"name": "erin", "prompt": "Tu es Erin."}));
    let exe = Path::new(env!("CARGO_BIN_EXE_swarm-control"));

    // Une commande d'agent sans agent possible : refus d'usage (code 2).
    let (code, _, err) = cli(&h, exe, &["ask", "salut"]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("--agent"), "{err}");

    // Commande inconnue, run incomplet, runner inconnu : usage (code 2).
    let (code, _, err) = cli(&h, exe, &["voler"]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("commande inconnue"), "{err}");
    let (code, _, err) = cli(&h, exe, &["run", "erin"]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("run attend un agent et un prompt"), "{err}");
    let (code, _, err) = cli(&h, exe, &["--runner", "toto", "status"]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("runner"), "{err}");

    // Un agent introuvable : erreur d'exécution (code 1).
    let (code, _, err) = cli(&h, exe, &["--agent", "fantome", "ask", "bof"]);
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("fantome"), "{err}");
    // schema sans serveur annoncé : erreur d'exécution (code 1).
    let (code, _, err) = control(&h, &["--agent", "erin", "schema", "magie"]);
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("non annoncé"), "{err}");

    // L'aide sort en 0 et documente les commandes.
    let (code, out, _) = cli(&h, exe, &["--help"]);
    assert_eq!(code, 0);
    assert!(out.contains("Commandes d'agent"), "{out}");
    assert!(out.contains("Commandes d'instance"), "{out}");
    assert!(out.contains("--debug"), "{out}");
    assert!(out.contains("--headless"), "{out}");
}

#[test]
fn a_renamed_binary_is_refused_with_the_migration_message() {
    let h = start(&[]);
    ok(&h, "create_agent", json!({"name": "discord", "prompt": "Tu es Discord."}));
    let real = Path::new(env!("CARGO_BIN_EXE_swarm-control"));
    // Une vraie copie renommée, comme on le faisait avant la 0.2.0.
    let copy = agent_exe(&h, "discord");
    std::fs::create_dir_all(copy.parent().unwrap()).unwrap();
    std::fs::copy(real, &copy).unwrap();

    // Lancée seule : refusée, code 2, avec le message de migration.
    let (code, _, err) = cli(&h, &copy, &[]);
    assert_eq!(code, 2, "{err}");
    for part in ["exécutable « discord »", "0.2.0", "swarm-control build <agent>", "--agent NAME"]
    {
        assert!(err.contains(part), "« {part} » absent de : {err}");
    }

    // Toujours refusée avec `--agent discord` : `cli::run` examine `argv[0]` avant les
    // arguments. Le nom de fichier ne choisit plus l'agent, il ne fait plus que refuser.
    let (code, _, err) = cli(&h, &copy, &["--agent", "discord"]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("exécutable « discord »"), "{err}");

    // C'est le vrai binaire qui sert discord, par `--agent` : l'instance le voit résident.
    let agent = start_agent(&h, real, &["--agent", "discord"]);
    let seen = ok(&h, "list_agents", json!({}));
    let entry = seen["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] == json!("discord"))
        .expect("discord listé par l'instance");
    assert!(
        entry["resident"]["url"].as_str().unwrap().contains(&agent.port.to_string()),
        "{entry}"
    );
}
