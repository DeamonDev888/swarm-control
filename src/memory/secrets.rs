//! Détection des jetons avant écriture : aucun secret n'entre dans la mémoire.
//!
//! Quatre motifs, cherchés sur le texte puis sur chaque tag, sans expression régulière
//! (aucune dépendance) : préfixes de clés connus (`sk-…`, `ghp_…`, `AKIA…`), jetons JWT
//! (`eyJ…` en trois segments), affectations (`API_KEY=…`, `token: …`, `Authorization:
//! Bearer …`) et chaînes longues mêlant majuscules, minuscules et chiffres. Les empreintes
//! hexadécimales (sha1, sha256, UUID) passent : ce sont des identifiants, pas des secrets.
//! Dans le doute on refuse ; le message de refus ne montre jamais le jeton.

/// Nature du jeton trouvé (jamais le jeton lui-même).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Leak {
    /// Clé ou token reconnaissable à son préfixe (`sk-…`, `ghp_…`, `AKIA…`).
    PrefixedKey,
    /// Jeton JWT (`eyJ…` en trois segments).
    Jwt,
    /// Affectation `API_KEY=…`, `token: …`, `Authorization: Bearer …`.
    Assignment,
    /// Chaîne longue et mélangée (majuscules, minuscules, chiffres).
    Opaque,
}

impl Leak {
    /// Nature du jeton, pour le message de refus.
    pub fn label(self) -> &'static str {
        match self {
            Leak::PrefixedKey => {
                "une clé ou un jeton reconnaissable à son préfixe (`sk-…`, `ghp_…`, `AKIA…`)"
            }
            Leak::Jwt => "un jeton JWT (`eyJ…`)",
            Leak::Assignment => {
                "une affectation de secret (`API_KEY=…`, `token: …`, `Authorization: Bearer …`)"
            }
            Leak::Opaque => "une chaîne longue mêlant majuscules, minuscules et chiffres",
        }
    }
}

/// Le premier jeton repéré dans `text`, ou `None` s'il n'en contient pas.
pub fn find_secret(text: &str) -> Option<Leak> {
    prefixed(text).or_else(|| jwt(text)).or_else(|| assignment(text)).or_else(|| opaque(text))
}

/// Préfixes sans ambiguïté : suivis de 8 caractères de jeton, c'en est un.
const LONG_PREFIXES: &[&str] = &[
    "sk-ant-",
    "sk-proj-",
    "sk-or-",
    "sk-live-",
    "github_pat_",
    "ghp_",
    "gho_",
    "ghu_",
    "ghs_",
    "ghr_",
    "glpat-",
    "xoxb-",
    "xoxp-",
    "xoxa-",
    "xapp-",
    "hooks.slack.com/services/",
    "ya29.",
    "gocsp-",
    "sq0at-",
    "shpat_",
    "shpca_",
    "shpss_",
];

/// Préfixes de clés AWS et Google, toujours en cette casse : suivis de 16 caractères de
/// jeton (`asia-pacific` en minuscules ne compte pas).
const CASED_PREFIXES: &[&str] = &["AKIA", "ASIA", "ABIA", "ACZA", "AIza"];

/// Préfixes courts qui sont aussi des mots : il faut 16 caractères de jeton au moins après
/// eux, dont deux chiffres, et une majuscule (ou 20 caractères) — `sk-` dans `task-12345678`
/// ne compte pas.
const SHORT_PREFIXES: &[&str] = &["sk-", "npm_", "sg."];

/// Mots qui introduisent une affectation de secret (`<mot> : = <valeur>`).
const KEYWORDS: &[&str] = &[
    "api_key",
    "api key",
    "apikey",
    "token",
    "secret",
    "password",
    "passwd",
    "pwd",
    "private_key",
    "private key",
    "access_key",
    "access key",
    "credential",
    "authorization",
    "clé",
    "cle",
    "mot de passe",
];

fn is_token(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

/// Longueur (octets) de la suite de caractères de jeton commençant en `at`.
fn run_len(text: &str, at: usize) -> usize {
    text.as_bytes()[at..].iter().take_while(|b| is_token(**b)).count()
}

/// `at` commence-t-il un mot (rien de « jeton » juste avant) ?
fn word_start(bytes: &[u8], at: usize) -> bool {
    at == 0 || !is_token(bytes[at - 1])
}

fn digits_in(s: &str) -> usize {
    s.bytes().filter(|b| b.is_ascii_digit()).count()
}

/// Clés à préfixe connu. Les préfixes insensibles à la casse se comparent sur une copie en
/// minuscules ASCII, qui garde les mêmes positions d'octets que le texte.
fn prefixed(text: &str) -> Option<Leak> {
    let lower = text.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    for prefix in LONG_PREFIXES {
        let mut at = 0;
        while let Some(pos) = lower[at..].find(prefix) {
            let start = at + pos;
            at = start + prefix.len();
            if word_start(bytes, start) && run_len(&lower, at) >= 8 {
                return Some(Leak::PrefixedKey);
            }
        }
    }
    for prefix in CASED_PREFIXES {
        let mut at = 0;
        while let Some(pos) = text[at..].find(prefix) {
            let start = at + pos;
            at = start + prefix.len();
            if word_start(text.as_bytes(), start) && run_len(text, at) >= 16 {
                return Some(Leak::PrefixedKey);
            }
        }
    }
    for prefix in SHORT_PREFIXES {
        let mut at = 0;
        while let Some(pos) = lower[at..].find(prefix) {
            let start = at + pos;
            at = start + prefix.len();
            let len = run_len(&lower, at);
            let tail = &lower[at..at + len];
            let mixed =
                len >= 16 && digits_in(tail) >= 2 && (tail != &text[at..at + len] || len >= 20);
            if word_start(bytes, start) && mixed {
                return Some(Leak::PrefixedKey);
            }
        }
    }
    None
}

/// Jeton JWT : `eyJ…` en tête d'un mot, trois segments base64url d'au moins 8 caractères.
fn jwt(text: &str) -> Option<Leak> {
    let lower = text.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut at = 0;
    while let Some(pos) = lower[at..].find("eyj") {
        let start = at + pos;
        at = start + 3;
        if !word_start(bytes, start) {
            continue;
        }
        let len = lower[start..].bytes().take_while(|b| is_token(*b) || *b == b'.').count();
        let parts: Vec<&str> = lower[start..start + len].split('.').collect();
        if parts.len() == 3 && parts.iter().all(|p| p.len() >= 8) {
            return Some(Leak::Jwt);
        }
    }
    None
}

/// Affectation `<mot-clé> : = <valeur>` (guillemets facultatifs) ou en-tête `Bearer <jeton>`.
fn assignment(text: &str) -> Option<Leak> {
    let lower = text.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    for key in KEYWORDS {
        let mut at = 0;
        while let Some(pos) = lower[at..].find(key) {
            let start = at + pos;
            at = start + key.len();
            let whole_word = word_start(bytes, start)
                && (at == bytes.len() || !bytes[at].is_ascii_alphanumeric() && bytes[at] != b'_');
            if !whole_word {
                continue;
            }
            let rest = lower[at..].trim_start();
            let Some(rest) = rest.strip_prefix(':').or_else(|| rest.strip_prefix('=')) else {
                continue;
            };
            let rest = rest.trim_start().trim_start_matches(['"', '\'', '`']);
            let value = &rest[..value_len(rest)];
            if secret_value(value) {
                return Some(Leak::Assignment);
            }
        }
    }
    let mut at = 0;
    while let Some(pos) = lower[at..].find("bearer") {
        let start = at + pos;
        at = start + 6;
        if !word_start(bytes, start) || (at < bytes.len() && is_token(bytes[at])) {
            continue;
        }
        let rest = lower[at..].trim_start();
        let len = run_len(rest, 0);
        if len >= 16 && digits_in(&rest[..len]) >= 1 {
            return Some(Leak::Assignment);
        }
    }
    None
}

/// Longueur (octets) d'une valeur affectée : jusqu'à une espace, un guillemet ou une
/// ponctuation d'entourage (`,` `;` `(` `)`…) ; `Tr0ub4dor&3` reste entier.
fn value_len(text: &str) -> usize {
    let mut len = 0;
    for c in text.chars() {
        if c.is_whitespace()
            || matches!(
                c,
                '"' | '\''
                    | '`'
                    | ','
                    | ';'
                    | '<'
                    | '>'
                    | '('
                    | ')'
                    | '['
                    | ']'
                    | '{'
                    | '}'
                    | '|'
                    | '\\'
            )
        {
            break;
        }
        len += c.len_utf8();
    }
    len
}

/// Une valeur affectée à un mot-clé ressemble-t-elle à un secret ? Les références (`$VAR`,
/// `<votre-clé>`, `***`) et les mots de passe courts passent : un refus abusif coûte plus
/// qu'un doute.
fn secret_value(value: &str) -> bool {
    let all_same = value.bytes().all(|b| b == value.as_bytes().first().copied().unwrap_or(0));
    !value.starts_with(['$', '*', '<', '{'])
        && value.len() >= 12
        && !all_same
        && (digits_in(value) > 0 || value.len() >= 24)
}

/// Chaîne longue et mélangée : ni hexadécimale (empreinte, UUID), ni tout en chiffres.
fn opaque(text: &str) -> Option<Leak> {
    let bytes = text.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        if !is_opaque_char(bytes[at]) {
            at += 1;
            continue;
        }
        let start = at;
        while at < bytes.len() && is_opaque_char(bytes[at]) {
            at += 1;
        }
        let run = &text[start..at];
        let hexish = run.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-');
        if run.len() >= 32
            && !hexish
            && run.bytes().any(|b| b.is_ascii_uppercase())
            && run.bytes().any(|b| b.is_ascii_lowercase())
            && run.bytes().any(|b| b.is_ascii_digit())
        {
            return Some(Leak::Opaque);
        }
    }
    None
}

fn is_opaque_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-')
}

#[cfg(test)]
mod tests {
    use super::{Leak, find_secret};

    fn leak(text: &str) -> Option<Leak> {
        find_secret(text)
    }

    #[test]
    fn les_cles_a_prefixe_sont_reconnuesquelle_que_soit_la_casse() {
        assert_eq!(leak("ma clé : sk-abc123def456ghi789jkl"), Some(Leak::PrefixedKey));
        assert_eq!(leak("sk-ant-api03-AbCdEf1234567890-xyz"), Some(Leak::PrefixedKey));
        assert_eq!(
            leak("token github: ghp_16C7e42F292c6912E7710c838347Ae178B4a"),
            Some(Leak::PrefixedKey)
        );
        assert_eq!(leak("AKIAIOSFODNN7EXAMPLE"), Some(Leak::PrefixedKey));
        assert_eq!(leak("slack xoxb-123456789012-abcdef"), Some(Leak::PrefixedKey));
        assert_eq!(leak("AIzaSyA1234567890abcdefghijklmnopq"), Some(Leak::PrefixedKey));
        assert_eq!(
            leak("envoyer via SG.xM3yJ7QpR9tK2vL8nB4cD6fG1h.abc123def456"),
            Some(Leak::PrefixedKey)
        );
        assert_eq!(
            leak("hooks.slack.com/services/T00000000/B00000000/XXXXXXXXXXXXXXXXXXXXXXXX"),
            Some(Leak::PrefixedKey)
        );
    }

    #[test]
    fn un_mot_qui_ressemble_a_un_prefixe_passe() {
        assert_eq!(leak("la tâche task-12345678 est faite"), None, "`sk-` dans `task-`");
        assert_eq!(leak("région asia-pacific-1"), None, "minuscules : pas une clé AWS");
        assert_eq!(leak("npm_ install puis npm_ run build"), None);
        assert_eq!(leak("hf_mistral_7b_instruct_v01"), None, "un nom de modèle");
        assert_eq!(leak("sk-le-programme-du-jour"), None, "aucun chiffre");
        assert_eq!(leak("npm_ run build avec node 20.10.0 et npm 10.2.3"), None);
    }

    #[test]
    fn un_jwt_est_reconnu() {
        let jwt = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N0XgL0n3I9PlFUP0THsR8U";
        assert_eq!(leak(&format!("Authorization: Bearer {jwt}")), Some(Leak::Jwt));
        assert_eq!(leak("eyJ.ab.cd.ef"), None, "segments trop courts");
        assert_eq!(leak("le mot eyjough n'est rien"), None);
    }

    #[test]
    fn une_affectation_de_secret_est_refusee_une_reference_passe() {
        assert_eq!(leak("API_KEY=abc123def456ghi789"), Some(Leak::Assignment));
        assert_eq!(leak("le token : \"abcdef1234567890abcdef\" du bot"), Some(Leak::Assignment));
        assert_eq!(leak("password = CorrectHorseBatteryStaple"), Some(Leak::Assignment));
        assert_eq!(leak("mot de passe: Tr0ub4dor&3Extensible"), Some(Leak::Assignment));
        assert_eq!(leak("Authorization: Bearer abcdef1234567890abcdef"), Some(Leak::Assignment));
        assert_eq!(leak("API_KEY=$ANTHROPIC_API_KEY"), None, "référence, pas valeur");
        assert_eq!(leak("api_key = <votre-clé>"), None);
        assert_eq!(leak("token: ***"), None);
        assert_eq!(leak("la clé du problème = la solution"), None, "pas un secret");
        assert_eq!(leak("password: hunter2"), None, "trop court pour être sûr");
        assert_eq!(leak("les tokens d'accès coûtent 0,002 $"), None);
        assert_eq!(leak("secret: xxxxxxxxxxxx"), None, "placeholder");
    }

    #[test]
    fn une_chaine_melongee_est_refusee_une_empreinte_passe() {
        assert_eq!(leak("blob QmFzZTY0IGJsb2IgZGUgZG9sbGVzIGV0IGRlIGNoYXRz"), Some(Leak::Opaque));
        assert_eq!(leak("Le commit a1b2c3d4e5f60718293a4b5c6d7e8f9012345678 est là"), None, "sha1");
        assert_eq!(
            leak("sha256 6f1ed0029559f12b1c9a0d9a8ffb2c4e7d8b8f1c3a5e7b9d1f3c5a7e9b1d3f5a"),
            None
        );
        assert_eq!(leak("uuid 550e8400-e29b-41d4-a716-446655440000"), None);
        assert_eq!(leak("SWARM_AGENT_PORT=23090 et --port 23080"), None, "trop court");
        assert_eq!(
            leak("binaire swarm-control-v0.1.2-x86_64-unknown-linux-gnu.tar.gz dans target/debug"),
            None,
            "un nom de fichier"
        );
        assert_eq!(leak("https://github.com/DeamonDev888/swarm-control/releases/tag/v0.1.2"), None);
    }

    #[test]
    fn un_texte_courant_passe() {
        assert_eq!(
            leak(
                "Le serveur MCP de bob écoute sur le port 23081 ; préférence : réponses en français."
            ),
            None
        );
        assert_eq!(leak(""), None);
        assert_eq!(leak("12345678901234567890123456789012"), None, "que des chiffres");
    }
}
