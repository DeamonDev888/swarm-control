//! Mémoire native des agents (PLAN §4.6) : `memory_store`, `memory_search`,
//! `memory_forget`, sans base de données ni serveur externe.
//!
//! Un souvenir par ligne JSON dans `<workspace>/bridge/memory/<runner>-<agent>.jsonl`
//! (id, ts, texte, tags, source), écrit en ajout. Les lignes illisibles sont ignorées à la
//! lecture (un fichier tronqué par un arrêt brutal ne masque jamais le reste) et disparaissent
//! à la compaction.
//!
//! - **Recherche** : mots-clés normalisés (minuscules, sans accents) ; score = termes trouvés
//!   (les tags comptent double, un préfixe de mot compte moitié) + récence (demi-vie de 30
//!   jours) ; tri par score décroissant, les plus récents en tête à égalité.
//! - **Plafond** : au-delà de [`MAX_BYTES`] ou [`MAX_LINES`], le fichier est compacté et ne
//!   garde que les plus récents dans la moitié du budget.
//! - **Portée `swarm`** : lecture seule des fichiers voisins (les autres agents du même
//!   workspace), jamais d'écriture.
//! - **Secrets** : un texte qui contient une clé, un jeton ou un mot de passe est refusé à
//!   l'écriture avec un message clair ([`find_secret`]).
//!
//! | module      | rôle                                                     |
//! |-------------|----------------------------------------------------------|
//! | `entry`     | la ligne : [`Memory`] (id, ts, texte, tags, source)      |
//! | `path`      | emplacement du fichier, fichiers voisins (portée swarm)  |
//! | `normalize` | minuscules, sans accents, découpe en termes              |
//! | `score`     | score d'un souvenir pour une requête, tri                |
//! | `secrets`   | détection des jetons à l'écriture                        |
//! | `store`     | ajout, lecture tolérante, oubli, compaction              |

mod entry;
mod normalize;
mod path;
mod score;
mod secrets;
mod store;

#[cfg(test)]
mod tests;

pub use entry::Memory;
pub use path::path;
pub use score::{Hit, Scope, search};
/// Tous les souvenirs d'un agent, dans l'ordre du fichier : pour le panneau mémoire de la
/// TUI (PLAN §4.6) ; aucune interface ne l'affiche encore.
#[allow(dead_code, unused_imports)]
pub use store::read;
pub use store::{forget, store};

/// Taille au-delà de laquelle le fichier est compacté (1 Mio).
pub const MAX_BYTES: u64 = 1024 * 1024;
/// Nombre de lignes au-delà duquel le fichier est compacté.
pub const MAX_LINES: usize = 2000;
/// Longueur maximale d'un texte (caractères), imposée aussi par le schéma MCP.
pub const MAX_TEXT: usize = 4096;
/// Nombre de tags au plus par souvenir.
pub const MAX_TAGS: usize = 32;
/// Longueur maximale d'un tag (caractères).
pub const MAX_TAG: usize = 64;
/// Nombre de résultats par défaut d'une recherche.
pub const DEFAULT_LIMIT: usize = 10;
/// Plafond de `limit` (schéma MCP et CLI).
pub const MAX_LIMIT: usize = 100;
