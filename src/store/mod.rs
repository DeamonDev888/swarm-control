//! Persistance de bas niveau : fichiers JSON atomiques et horloge.

mod clock;
mod json_file;

pub use clock::now_ms;
pub use json_file::{read_json, write_json_atomic};
