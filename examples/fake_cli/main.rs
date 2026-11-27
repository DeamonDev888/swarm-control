//! Faux CLI pour les tests d'intégration : imite `claude -p` et `hermes`.
//!
//! - Claude (`-p`) : prompt sur stdin, enveloppe JSON `{result, session_id, is_error}` ;
//!   `--resume X` conserve la session.
//! - Hermes (`chat`) : prompt sur stdin (`--query-file -`), réponse sur stdout,
//!   `session_id: …` sur stderr ; gère aussi `profile create|delete` et `config set`.
//!
//! Déclencheurs dans le prompt : `SLEEP` (dort 60 s), `DEADSESSION` (échoue si
//! une session est reprise), `FAIL401` (401 tant que le jeton n'est pas celui
//! de secours), `PROVIDERFAIL` / `NOCRED` (erreurs fournisseur Hermes).

mod args;
mod profile;
mod reply;

use std::io::Read;

fn main() {
    let args = args::Args::from_env();
    if profile::handle(&args) {
        return;
    }
    let prompt = if args.is_antigravity() {
        args.after("-p").unwrap_or_default()
    } else {
        let mut buf = String::new();
        let _ = std::io::stdin().read_to_string(&mut buf);
        buf
    };
    reply::answer(&args, &prompt);
}
