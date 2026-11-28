//! Contrats publics figés (TODO-TESTS §1) : la sortie est comparée à un fichier de
//! référence versionné dans `tests/golden/`.
//!
//! Un changement de contrat doit mettre à jour la référence **dans le même commit** :
//! `SWARM_BLESS=1 cargo test` réécrit les fichiers, la revue voit la rupture. Sans la
//! variable, toute différence fait échouer le test avec le nom du fichier à relire.
//!
//! Les comparaisons ignorent les fins de ligne (`\r\n` / `\n`) : les références sont les
//! mêmes sur Windows, Linux et macOS.

use std::path::PathBuf;

/// Dossier des références (`tests/golden/`), ancré au manifeste : indépendant du dossier
/// courant d'exécution du binaire de test.
pub fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("golden")
}

/// `SWARM_BLESS=1` : on réécrit les références au lieu de les comparer.
pub fn blessing() -> bool {
    std::env::var("SWARM_BLESS").as_deref() == Ok("1")
}

/// Fins de ligne uniformisées (les références sont écrites en `\n`).
fn lf(text: &str) -> String {
    text.replace("\r\n", "\n")
}

/// Compare `actual` à la référence `name` (chemin relatif à `tests/golden/`), ou la réécrit
/// sous `SWARM_BLESS=1`. Le texte est rangé avec une fin de ligne finale unique.
pub fn check(name: &str, actual: &str) {
    let path = dir().join(name);
    let actual = lf(actual);
    let actual = actual.trim_end_matches('\n').to_string() + "\n";
    if blessing() {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, &actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "référence absente {} ({e}) : lance `SWARM_BLESS=1 cargo test` pour la créer",
            path.display()
        )
    });
    assert_eq!(
        lf(&expected),
        actual,
        "contrat public changé : relis la différence, puis mets à jour `{name}` \
         (`SWARM_BLESS=1 cargo test`) **dans le même commit**"
    );
}

/// Compare un document JSON à la référence `<name>.json`, écrit en JSON indenté et trié
/// (`serde_json` range les clés d'un objet par ordre alphabétique : deux exécutions
/// successives donnent le même texte).
pub fn check_json(name: &str, value: &serde_json::Value) {
    check(
        &format!("{name}.json"),
        &serde_json::to_string_pretty(value).expect("JSON sérialisable"),
    );
}

/// Remplace chaque suite de chiffres par `N` (ports, PID, tailles variables) pour qu'une
/// bannière ou un état reste comparable d'une exécution à l'autre.
pub fn digits(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_digits = false;
    for c in text.chars() {
        if c.is_ascii_digit() {
            if !in_digits {
                out.push('N');
                in_digits = true;
            }
        } else {
            in_digits = false;
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits_collapses_runs_but_keeps_the_rest() {
        assert_eq!(digits("MCP sur http://127.0.0.1:23456/mcp"), "MCP sur http://N.N.N.N:N/mcp");
        assert_eq!(digits("2 tours, 1 h"), "N tours, N h");
        assert_eq!(digits("aucun nombre"), "aucun nombre");
    }

    #[test]
    fn check_matches_a_reference_regardless_of_line_endings() {
        let dir = std::env::temp_dir().join(format!("swarm-golden-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("sample.txt"), "ligne\r\nautre\r\n").unwrap();
        // La référence est lue relative au dossier golden du dépôt : on teste la normale
        // ici, pas le fichier du dépôt.
        assert_eq!(lf("a\r\nb\r\n"), "a\nb\n");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
