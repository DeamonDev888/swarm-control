//! Une ligne du fichier mémoire : un souvenir.

use serde::{Deserialize, Serialize};

/// Un souvenir d'un agent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Memory {
    /// Identifiant court, unique par fichier (`<ts en hexa>-<compteur>`), sert à `forget`.
    pub id: String,
    /// Horodatage (ms depuis l'époque).
    pub ts: u64,
    /// Le souvenir, en clair (jamais un jeton : filtré à l'écriture).
    pub text: String,
    /// Mots-clés, tels que donnés (comptent double dans la recherche).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Qui a écrit le souvenir (`mcp`, `cli`…).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source: String,
}
