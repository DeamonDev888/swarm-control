//! Le dialecte Discord de l'`overmind-bridge` : contexte du message et nettoyage de la réponse.

/// Prompt envoyé à l'agent, au format de `formatDiscordContext` (OverMind) : les SOUL.md
/// existantes lisent `CHANNEL_ID` dans ce bloc pour savoir où répondre.
pub fn context(
    message: &str,
    channel_id: Option<&str>,
    username: Option<&str>,
    user_id: Option<&str>,
) -> String {
    if channel_id.is_none() && username.is_none() {
        return message.to_string();
    }
    [
        "[DISCORD CONTEXT]".to_string(),
        format!("CHANNEL_ID: {}", channel_id.unwrap_or("unknown")),
        format!("USER_NAME: {}", username.unwrap_or("unknown")),
        format!("USER_ID: {}", user_id.unwrap_or("unknown")),
        "=======================".to_string(),
        format!("MESSAGE: {message}"),
    ]
    .join("\n")
}

/// Texte rendu au routeur. Vide si l'agent a déjà posté sur Discord par ses outils MCP (le
/// routeur affiche alors « livré via MCP ») ; sinon la réponse sans les traces d'outils.
pub fn reply_text(raw: &str) -> String {
    if already_posted(raw) {
        return String::new();
    }
    raw.lines()
        .filter(|line| !is_trace(line.trim()))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

/// Lignes de trace d'outils ou de journal, jamais destinées à l'utilisateur.
fn is_trace(line: &str) -> bool {
    const PREFIXES: [&str; 3] = ["Warning:", "[Hermes:", "SESSION_ID:"];
    const MARKERS: [&str; 10] = [
        "Message envoyé",
        "Embed publié",
        "Embed envoyé",
        "Embed créé",
        "envoyé sur Discord",
        "envoyé sur le canal",
        "creer_embed",
        "envoyer_message",
        "code_preview",
        "uploader_fichier",
    ];
    PREFIXES.iter().any(|p| line.starts_with(p)) || MARKERS.iter().any(|m| line.contains(m))
}

/// L'agent a-t-il **vraiment** posté ? Une confirmation qui porte un ID Discord (17 à 19
/// chiffres) ou un lien `discord.com/channels/…/…/<id>` — pas un simple nom d'outil.
fn already_posted(raw: &str) -> bool {
    raw.lines().any(|line| {
        let lower = line.to_lowercase();
        let confirmed = ["message envoyé", "message envoyè", "embed publié", "embed publiè"]
            .iter()
            .any(|phrase| lower.contains(phrase));
        (confirmed && lower.contains("id") && has_snowflake(line)) || has_message_link(line)
    })
}

/// Une suite de 17 à 19 chiffres (un identifiant Discord).
fn has_snowflake(text: &str) -> bool {
    digit_runs(text).any(|run| (17..=19).contains(&run.len()))
}

/// `discord.com/channels/<serveur>/<salon>/<message>` avec un ID de message valide.
fn has_message_link(text: &str) -> bool {
    text.split("discord.com/channels/").skip(1).any(|rest| {
        let parts: Vec<&str> = rest.splitn(3, '/').collect();
        parts.len() == 3
            && parts[..2].iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
            && digit_runs(parts[2])
                .next()
                .is_some_and(|id| parts[2].starts_with(id) && (17..=19).contains(&id.len()))
    })
}

fn digit_runs(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !c.is_ascii_digit()).filter(|run| !run.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_matches_the_overmind_format() {
        let prompt = context("analyse AAPL", Some("1442317829998383235"), Some("bob"), Some("42"));
        assert_eq!(
            prompt,
            "[DISCORD CONTEXT]\nCHANNEL_ID: 1442317829998383235\nUSER_NAME: bob\nUSER_ID: 42\n\
             =======================\nMESSAGE: analyse AAPL"
        );
        assert_eq!(context("brut", None, None, Some("42")), "brut");
        assert!(context("m", Some("1"), None, None).contains("USER_NAME: unknown"));
    }

    #[test]
    fn a_confirmed_post_silences_the_reply() {
        assert_eq!(reply_text("Message envoyé sur le canal, ID: 1531826331875147857"), "");
        assert_eq!(reply_text("voir https://discord.com/channels/1/22/1531826331875147857"), "");
        // Un nom d'outil sans ID n'est pas une preuve : la réponse reste.
        assert_eq!(
            reply_text("J'ai appelé envoyer_message\nAAPL est haussier."),
            "AAPL est haussier."
        );
        // Un nombre trop court n'est pas un ID Discord.
        assert_eq!(reply_text("Message envoyé, id 12345"), "");
        assert!(!already_posted("Message envoyé, id 12345"));
    }

    #[test]
    fn traces_are_removed_from_the_reply() {
        let raw =
            "Warning: lent\n[Hermes: tool]\nSESSION_ID: abc\nRSI à 71.\n  Embed créé  \nSurachat.";
        assert_eq!(reply_text(raw), "RSI à 71.\nSurachat.");
    }
}
