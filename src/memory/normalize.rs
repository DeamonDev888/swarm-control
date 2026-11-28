//! Normalisation de recherche : minuscules, sans accents, découpé en termes.
//!
//! Sans dépendance : les lettres latines accentuées (Latin-1 complément et Latin étendu A)
//! sont ramenées à leur lettre de base par deux tables, le reste est mis en minuscules. Tout
//! ce qui n'est ni lettre ni chiffre sépare les termes.

/// Une lettre latine (ou un chiffre) → sa forme minuscule ; sinon un séparateur.
enum Fold {
    Keep(char),
    Word(&'static str),
    Space,
}

/// Latin-1 complément, `U+00C0`–`U+00FF` : lettre de base minuscule, `?` pour les cas traités
/// à part (Æ, ×, Þ, ß, æ, ÷, þ).
const LATIN_1: &str = concat!(
    "aaaaaa", "?", "c", "eeee", "iiii", "d", "n", "ooooo", "?", "o", "uuuu", "y", "??", "aaaaaa",
    "?", "c", "eeee", "iiii", "d", "n", "ooooo", "?", "o", "uuuu", "y", "?", "y",
);

/// Latin étendu A, `U+0100`–`U+017F` : même convention (`?` pour Ĳ, ĳ, Œ, œ).
const LATIN_EXTENDED_A: &str = concat!(
    "aaaaaa",
    "cccccccc",
    "dddd",
    "eeeeeeeeee",
    "gggggggg",
    "hhhh",
    "iiiiiiiiii",
    "??",
    "jj",
    "kkk",
    "llllllllll",
    "nnnnnnnnn",
    "oooooo",
    "??",
    "rrrrrr",
    "ssssssss",
    "tttttt",
    "uuuuuuuuuuuu",
    "ww",
    "yyy",
    "zzzzzzz",
);

/// Texte normalisé : minuscules, sans accents, lettres et chiffres seulement, les termes
/// séparés par une seule espace (`"École, Noël !"` → `"ecole noel"`).
pub fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_space = false;
    for c in text.chars() {
        match fold(c) {
            Fold::Keep(c) => {
                if pending_space && !out.is_empty() {
                    out.push(' ');
                }
                pending_space = false;
                out.push(c);
            }
            Fold::Word(word) => {
                if pending_space && !out.is_empty() {
                    out.push(' ');
                }
                pending_space = false;
                out.push_str(word);
            }
            Fold::Space => pending_space = true,
        }
    }
    out
}

/// Mots vides ignorés par la recherche (français et anglais) : les termes de requête
/// comme les termes indexés, pour qu'une question courante ne pèse pas sur le score.
const STOPWORDS: &[&str] = &[
    "le", "la", "les", "de", "des", "du", "un", "une", "et", "ou", "au", "aux", "en", "dans",
    "pour", "sur", "par", "avec", "ce", "cet", "cette", "ces", "que", "qui", "quoi", "dont", "est",
    "sont", "ete", "etre", "the", "an", "of", "and", "or", "to", "in", "on", "for", "with", "is",
    "are", "be",
];

/// Les termes normalisés d'un texte, dans l'ordre, sans doublon ni mot vide.
pub fn tokens(text: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    normalize(text)
        .split(' ')
        .filter(|t| !t.is_empty())
        .filter(|t| !STOPWORDS.contains(t))
        .filter(|t| seen.insert(t.to_string()))
        .map(String::from)
        .collect()
}

fn fold(c: char) -> Fold {
    if c.is_ascii_alphanumeric() {
        return Fold::Keep(c.to_ascii_lowercase());
    }
    match c {
        'Æ' | 'æ' => Fold::Word("ae"),
        'Œ' | 'œ' => Fold::Word("oe"),
        'Þ' | 'þ' => Fold::Word("th"),
        'ß' => Fold::Word("ss"),
        'Ĳ' | 'ĳ' => Fold::Word("ij"),
        '×' | '÷' => Fold::Space,
        '\u{C0}'..='\u{FF}' => table(LATIN_1, 0xC0, c),
        '\u{100}'..='\u{17F}' => table(LATIN_EXTENDED_A, 0x100, c),
        // Lettre non latine (grec, cyrillique…) : gardée, en minuscules.
        other => {
            let mut lower = other.to_lowercase();
            match lower.next() {
                Some(first) if first.is_alphanumeric() => Fold::Keep(first),
                _ => Fold::Space,
            }
        }
    }
}

fn table(entries: &str, base: u32, c: char) -> Fold {
    match entries.as_bytes()[(c as u32 - base) as usize] {
        b'?' => Fold::Space,
        base => Fold::Keep(base as char),
    }
}

#[cfg(test)]
mod tests {
    use super::{LATIN_1, LATIN_EXTENDED_A, normalize, tokens};

    #[test]
    fn les_tables_couvrent_exactement_leurs_plages() {
        assert_eq!(LATIN_1.len(), 0x100 - 0xC0);
        assert_eq!(LATIN_EXTENDED_A.len(), 0x180 - 0x100);
    }

    #[test]
    fn minuscules_sans_accents_et_sans_ponctuation() {
        assert_eq!(normalize("École, Noël !"), "ecole noel");
        assert_eq!(normalize("ÀÁÂÃÄÅ ÇÑ Ø Ÿ"), "aaaaaa cn o y");
        assert_eq!(normalize("L'été indien — déjà vu…"), "l ete indien deja vu");
        assert_eq!(
            normalize("détête ĹŲœuvre Æsop Đđ Łł Ĳ ĳ Þ þ ß"),
            "detete luoeuvre aesop dd ll ij ij th th ss"
        );
        assert_eq!(normalize(""), "");
        assert_eq!(normalize("  ,,  "), "");
        assert_eq!(normalize("GPT-4o_api_key.v2"), "gpt 4o api key v2");
        assert_eq!(normalize("Δελτα ΑΒΓ"), "δελτα αβγ");
    }

    #[test]
    fn les_termes_sont_uniques_et_dans_l_ordre() {
        assert_eq!(tokens("Réseau, réseau et RÉSEAUX !"), ["reseau", "reseaux"]);
        assert_eq!(tokens("---"), Vec::<String>::new());
    }
}
