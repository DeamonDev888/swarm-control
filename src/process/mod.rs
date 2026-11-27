//! Lancement et surveillance des processus d'agents (sans runtime async).
//!
//! Chaque flux est lu par un fil dédié ; le fil appelant attend la fin du
//! process en surveillant délai, demande d'arrêt et annulation de la requête.

mod alive;
mod command;
mod kill;
mod outcome;
mod readers;
mod run;
mod stdin;
mod utf8;
mod wait;

pub use alive::{pid_alive, pids_alive};
pub use command::command;
pub use kill::kill_tree;
pub use outcome::{End, ProcOutput};
pub use run::run;
