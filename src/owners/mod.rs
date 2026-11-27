//! Baux d'un agent : un seul process fait tourner (ou héberge) un agent à la fois.
//!
//! Deux sortes de bail, même mécanique :
//! - **bail de run** `<ws>/bridge/owners/<runner>-<agent>.json` : pris par chaque run,
//!   libéré à sa fin ;
//! - **bail d'hébergement** `<ws>/bridge/hosts/<runner>-<agent>.json` : pris par un agent
//!   résident pour toute sa vie, avec l'URL de son serveur.
//!
//! Un bail est un fichier créé de façon **exclusive** (`create_new`, atomique) portant
//! `{instance, pid, run_id, ts}` ; son détenteur rafraîchit `ts` toutes les 30 s. Il est
//! **périmé** si son PID est mort ou si son `ts` a plus de 2 minutes (un PID recyclé ne
//! bat pas du cœur). La reprise d'un bail périmé est sérialisée par un dossier de garde,
//! pour que deux repreneurs ne gagnent pas.

mod acquire;
mod holder;
mod inspect;
mod lease;
mod path;
mod sweep;
mod takeover;

#[cfg(test)]
mod tests;

pub use acquire::{Busy, acquire, acquire_at, acquire_host};
pub use holder::{Holder, instance_name, set_instance_name};
pub use inspect::{holder_of, host_of};
pub use lease::Lease;
pub use sweep::sweep;
