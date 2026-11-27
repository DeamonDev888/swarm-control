//! Envoi du prompt sur l'entrée standard du process.

use std::io::Write;
use std::process::ChildStdin;

/// Écrit `data` dans un fil dédié puis ferme stdin : un prompt plus gros que
/// le tampon du pipe ne doit pas bloquer la surveillance.
pub fn feed(stdin: Option<ChildStdin>, data: Option<&str>) {
    let (Some(mut stdin), Some(data)) = (stdin, data) else {
        return;
    };
    let data = data.to_owned();
    std::thread::spawn(move || {
        let _ = stdin.write_all(data.as_bytes());
        // stdin fermé ici : le CLI sait que le prompt est complet.
    });
}
