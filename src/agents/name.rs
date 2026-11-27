//! Règles de nommage des agents.

/// Nom d'agent : `[a-zA-Z0-9_-]`, 1 à 128 caractères (anti traversée de chemin).
pub fn validate_name(name: &str) -> Result<(), String> {
    let ok = !name.is_empty()
        && name.len() <= 128
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    if ok {
        Ok(())
    } else {
        Err(format!(
            "Nom d'agent invalide '{name}' : seuls [a-zA-Z0-9_-] sont autorisés (1 à 128 caractères)."
        ))
    }
}

/// Règle des profils Hermes (`PROFILE_ID_RE` côté Hermes) : minuscules,
/// chiffres, `_` et `-`, 64 caractères max, premier caractère alphanumérique.
pub fn validate_hermes_name(name: &str) -> Result<(), String> {
    let b = name.as_bytes();
    let ok = !b.is_empty()
        && b.len() <= 64
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_' || *c == b'-');
    if ok {
        Ok(())
    } else {
        Err(format!(
            "Nom de profil Hermes invalide '{name}' : minuscules/chiffres/_/- uniquement, 64 max, \
             sans _ ou - au début."
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{validate_hermes_name, validate_name};

    #[test]
    fn generic_names() {
        assert!(validate_name("expert_python-2").is_ok());
        assert!(validate_name("../x").is_err());
        assert!(validate_name("").is_err());
        assert!(validate_name("a b").is_err());
    }

    #[test]
    fn hermes_names_are_lowercase_only() {
        assert!(validate_hermes_name("nexus_master").is_ok());
        assert!(validate_hermes_name("Hera").is_err());
        assert!(validate_hermes_name("_x").is_err());
        assert!(validate_hermes_name(&"a".repeat(65)).is_err());
    }
}
