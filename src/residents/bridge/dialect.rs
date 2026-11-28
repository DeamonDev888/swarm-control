//! Les dialectes du pont : comment un envoi `/send` devient un prompt pour l'agent, et une
//! réponse brute devient le texte rendu au client.
//!
//! | dialecte   | prompt                                               | réponse                        |
//! |------------|------------------------------------------------------|--------------------------------|
//! | `discord`  | bloc `[DISCORD CONTEXT]` (`discord::context`)         | vidée si déjà posté, sans traces |
//! | `whatsapp` | bloc `[WHATSAPP CONTEXT]` (chat, expéditeur, groupe)  | même règle que Discord         |
//! | `raw`      | message transmis tel quel, aucune injection           | jamais vidée, transmise telle quelle |

use super::discord;
use serde_json::Value;

/// Le dialecte d'un pont (`--bridge-dialect`), défaut [`Dialect::Discord`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Dialect {
    #[default]
    Discord,
    Whatsapp,
    Raw,
}

impl Dialect {
    /// Le dialecte nommé par la CLI (`--bridge-dialect`).
    pub fn parse(name: &str) -> Result<Dialect, String> {
        Ok(match name.trim().to_ascii_lowercase().as_str() {
            "discord" => Dialect::Discord,
            "whatsapp" => Dialect::Whatsapp,
            "raw" => Dialect::Raw,
            other => {
                return Err(format!(
                    "dialecte inconnu « {other} » : discord, whatsapp ou raw attendus"
                ));
            }
        })
    }

    /// Prompt envoyé à l'agent : le message, habillé selon le dialecte avec les champs
    /// utiles du corps `/send`.
    pub fn prompt(&self, body: &Value, message: &str) -> String {
        match self {
            Dialect::Discord => discord::context(
                message,
                text(body.get("channelId")).as_deref(),
                text(body.get("username")).as_deref(),
                text(body.get("userId")).as_deref(),
            ),
            Dialect::Whatsapp => whatsapp_prompt(
                message,
                text(body.get("chat")).as_deref(),
                text(body.get("sender")).as_deref(),
                text(body.get("group")).as_deref(),
            ),
            // Aucune injection : le corps du message est transmis tel quel.
            Dialect::Raw => message.to_string(),
        }
    }

    /// Qui demande, pour la traçabilité du journal (`from` du tour).
    pub fn origin(&self, body: &Value) -> Option<String> {
        match self {
            Dialect::Discord => {
                let who = text(body.get("username")).unwrap_or_else(|| "?".into());
                Some(format!("discord:{who}"))
            }
            Dialect::Whatsapp => {
                let who = text(body.get("sender")).unwrap_or_else(|| "?".into());
                Some(format!("whatsapp:{who}"))
            }
            // raw ne porte pas d'identité de pair.
            Dialect::Raw => None,
        }
    }

    /// Réponse rendue au client. Discord et WhatsApp appliquent la même règle : vidée si
    /// l'agent a déjà posté par ses outils, sinon sans les traces d'outils. `raw` transmet
    /// la réponse telle quelle, jamais vidée.
    pub fn reply(&self, raw: &str) -> String {
        match self {
            Dialect::Discord | Dialect::Whatsapp => discord::reply_text(raw),
            Dialect::Raw => raw.to_string(),
        }
    }
}

/// Prompt WhatsApp, miroir du format Discord : les SOUL.md lisent `CHAT`/`SENDER`/`GROUP`
/// dans ce bloc pour savoir à qui répondre. Sans aucun champ, le message reste tel quel
/// (même règle que Discord sans `channelId` ni `username`).
fn whatsapp_prompt(
    message: &str,
    chat: Option<&str>,
    sender: Option<&str>,
    group: Option<&str>,
) -> String {
    if chat.is_none() && sender.is_none() && group.is_none() {
        return message.to_string();
    }
    [
        "[WHATSAPP CONTEXT]".to_string(),
        format!("CHAT: {}", chat.unwrap_or("unknown")),
        format!("SENDER: {}", sender.unwrap_or("unknown")),
        format!("GROUP: {}", group.unwrap_or("unknown")),
        "=======================".to_string(),
        format!("MESSAGE: {message}"),
    ]
    .join("\n")
}

/// Préfixes des lignes de journal écrites par les runners, jamais destinées à l'utilisateur.
const LOG_PREFIXES: [&str; 3] = ["warning:", "[hermes:", "session_id:"];

/// Retire d'une réponse les lignes de trace : préfixes de journal et marqueurs d'outils
/// (`traces`), insensibles à la casse. La réponse est ensuite rognée en tête et en queue.
///
/// Sans appelant pour l'instant : c'est la règle de nettoyage des dialectes `whatsapp` et
/// `raw` (PLAN §4.2), à brancher dans la réponse du pont.
#[allow(dead_code)]
pub(super) fn clean(raw: &str, traces: &[&str]) -> String {
    raw.lines()
        .filter(|line| {
            let lower = line.trim().to_lowercase();
            !LOG_PREFIXES.iter().any(|prefix| lower.starts_with(prefix))
                && !traces.iter().any(|marker| lower.contains(&marker.to_lowercase()))
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

/// Une ligne confirme-t-elle un post ? Elle doit porter une phrase de confirmation **et**
/// un identifiant reconnu par `has_id`, ou bien un lien reconnu par `has_link` — un simple
/// nom d'outil ne suffit pas.
///
/// Sans appelant pour l'instant : « réponse vide si déjà posté » des dialectes `whatsapp`
/// et `raw` (PLAN §4.2), à brancher dans la réponse du pont.
#[allow(dead_code)]
pub(super) fn confirms_post(
    line: &str,
    confirmations: &[&str],
    has_id: fn(&str) -> bool,
    has_link: fn(&str) -> bool,
) -> bool {
    let lower = line.to_lowercase();
    let confirmed = confirmations.iter().any(|phrase| lower.contains(phrase));
    (confirmed && has_id(line)) || has_link(line)
}

/// Une chaîne, ou un nombre écrit en chaîne (les IDs Discord et WhatsApp arrivent sous les
/// deux formes).
pub(super) fn text(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Number(n)) => Some(n.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const DEJA_POSTE: &str = "Message envoyé sur le canal, ID: 1531826331875147857";

    #[test]
    fn le_dialecte_se_parse_depuis_la_cli() {
        assert_eq!(Dialect::parse("discord"), Ok(Dialect::Discord));
        assert_eq!(Dialect::parse(" whatsapp "), Ok(Dialect::Whatsapp));
        assert_eq!(Dialect::parse("RAW"), Ok(Dialect::Raw));
        assert_eq!(Dialect::default(), Dialect::Discord, "défaut : discord");
        let err = Dialect::parse("signal").unwrap_err();
        assert!(err.contains("discord, whatsapp ou raw"), "{err}");
    }

    #[test]
    fn discord_garde_le_format_et_la_regle_actuels() {
        let body = json!({"channelId": 1442317829998383235u64, "username": "bob", "userId": "42"});
        assert_eq!(
            Dialect::Discord.prompt(&body, "analyse AAPL"),
            "[DISCORD CONTEXT]\nCHANNEL_ID: 1442317829998383235\nUSER_NAME: bob\nUSER_ID: 42\n\
             =======================\nMESSAGE: analyse AAPL"
        );
        assert_eq!(
            Dialect::Discord.origin(&body),
            Some("discord:bob".into()),
            "origine journalisée"
        );
        assert_eq!(Dialect::Discord.reply(DEJA_POSTE), "", "déjà posté : réponse vidée");
    }

    #[test]
    fn whatsapp_injecte_son_bloc_et_applique_la_meme_regle_de_reponse() {
        let body = json!({"chat": "Salon du matin", "sender": "bob", "group": "AAPL"});
        assert_eq!(
            Dialect::Whatsapp.prompt(&body, "analyse AAPL"),
            "[WHATSAPP CONTEXT]\nCHAT: Salon du matin\nSENDER: bob\nGROUP: AAPL\n\
             =======================\nMESSAGE: analyse AAPL"
        );
        // Champs absents : le message reste tel quel, comme Discord sans contexte.
        assert_eq!(Dialect::Whatsapp.prompt(&json!({}), "brut"), "brut");
        // Champs partiels : les manquants valent `unknown`.
        assert_eq!(
            Dialect::Whatsapp.prompt(&json!({"chat": "salon"}), "coucou"),
            "[WHATSAPP CONTEXT]\nCHAT: salon\nSENDER: unknown\nGROUP: unknown\n\
             =======================\nMESSAGE: coucou"
        );
        assert_eq!(Dialect::Whatsapp.origin(&body), Some("whatsapp:bob".into()));
        // Même règle de réponse que Discord : vidée si déjà posté, traces filtrées sinon.
        assert_eq!(Dialect::Whatsapp.reply(DEJA_POSTE), "");
        assert_eq!(Dialect::Whatsapp.reply("Warning: lent\nRSI à 71."), "RSI à 71.");
    }

    #[test]
    fn raw_transmet_le_message_et_la_reponse_tels_quels() {
        let body = json!({"chat": "x", "username": "bob", "channelId": "1"});
        // Aucune injection de contexte, aucun rognage : le message part tel quel.
        assert_eq!(Dialect::Raw.prompt(&body, "  tel quel  "), "  tel quel  ");
        assert_eq!(Dialect::Raw.origin(&body), None, "raw ne porte pas d'identité");
        // Jamais vidée, même quand l'agent annonce avoir posté ; traces comprises.
        assert_eq!(Dialect::Raw.reply(DEJA_POSTE), DEJA_POSTE);
        assert_eq!(
            Dialect::Raw.reply("Warning: lent\nSESSION_ID: abc"),
            "Warning: lent\nSESSION_ID: abc"
        );
    }
}
