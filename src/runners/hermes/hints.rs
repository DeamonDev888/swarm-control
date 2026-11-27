//! Indices actionnables pour les échecs Hermes les plus courants.

pub fn hint_for(text: &str, agent: Option<&str>) -> Option<String> {
    let lower = text.to_lowercase();
    if lower.contains("no usable credentials found") {
        let who = agent.unwrap_or("<agent>");
        return Some(format!(
            "les credentials Hermes sont par profil : `hermes -p {who} auth add <provider>` \
             (ou recrée l'agent avec cloneFrom)."
        ));
    }
    if lower.contains("billing or credits exhausted") || lower.contains("insufficient balance") {
        return Some(
            "crédits du fournisseur épuisés : change de modèle/provider \
             (update_agent_config model) ou recharge."
                .into(),
        );
    }
    None
}

#[cfg(test)]
mod tests {
    use super::hint_for;

    #[test]
    fn explains_missing_credentials() {
        let hint = hint_for("No usable credentials found for provider 'x'", Some("hugo")).unwrap();
        assert!(hint.contains("hermes -p hugo auth add"));
    }

    #[test]
    fn explains_exhausted_credits() {
        assert!(
            hint_for("Billing or credits exhausted: HTTP 429", None).unwrap().contains("crédits")
        );
        assert!(hint_for("autre erreur", None).is_none());
    }
}
