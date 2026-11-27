//! Journal des tours d'un agent : `<workspace>/bridge/history/<runner>-<agent>.jsonl`.
//!
//! Une ligne JSON par tour, écrite par le fil de travail résident (donc dans l'ordre) ou,
//! pour un tour local, par la CLI elle-même :
//! `{ts, from, via, prompt, reply, error, sessionId, durationMs, queuedMs, runner, agent}`.
//! Ce journal est **à nous** : il ne dépend ni du format de session de Claude (JSONL) ni de
//! celui de Hermes (SQLite), et il nourrit `history`, la CLI et la TUI.
//!
//! Rotation par taille : au-delà de [`MAX_BYTES`] (5 Mio), le fichier courant devient
//! `<…>.jsonl.1` (l'ancien `.1` est remplacé) et un fichier neuf est ouvert. Deux générations
//! suffisent : le journal sert au diagnostic et à la conversation récente, pas à l'archivage.

mod entry;
mod path;
mod read;
mod write;

#[cfg(test)]
mod tests;

pub use entry::Entry;
pub use path::path;
#[cfg(any(test, feature = "tui"))]
pub use read::fingerprint;
pub use read::read;
#[cfg(test)]
pub use write::append;
pub use write::record;
