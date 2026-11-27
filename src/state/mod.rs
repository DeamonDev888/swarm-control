//! État persistant **par agent** : `<workspace>/bridge/state/<runner>-<agent>.json`.
//!
//! Un fichier par agent (et non un JSON global partagé) : deux agents ne se
//! réécrivent jamais l'un l'autre, même hébergés par des instances différentes.
//! Contenu : `{ runner, agent, session: {id, ts}, lastRun: {…} }`.

mod doc;
mod path;

pub use doc::{list, read, remove, update};
pub use path::dir;
