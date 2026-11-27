//! Registre des runs d'agents.
//!
//! Trois couches :
//! - **mémoire** : chaque run vivant (`LiveRun`) avec PID, statut, session et
//!   sortie horodatée — lu par `agent_control` ;
//! - **disque** : le dernier run de chaque agent dans son fichier d'état
//!   (`bridge/state/`), pour le retrouver après redémarrage ou depuis une autre instance ;
//! - **bail** (`owners`) : un run nommé détient le bail de son agent jusqu'à sa fin,
//!   ce qui interdit à un autre process de le lancer en même temps.
//!
//! Un balayeur passe toutes les 30 s : battement de cœur des baux, PID morts →
//! `orphaned`, baux périmés retirés, runs terminés depuis plus d'une heure oubliés.

mod debris;
mod disk;
mod index;
mod lifecycle;
mod live_run;
mod outcome;
mod output;
mod output_buffer;
mod snapshot;
mod state;
mod status;
mod sweeper;

pub use disk::{disk_entry, mark_killed};
pub use index::{all, begin, find, find_by_run_id};
pub use live_run::LiveRun;
pub use outcome::Outcome;
pub use snapshot::Snapshot;
pub use status::Status;
pub use sweeper::start_sweeper;
