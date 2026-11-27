//! Routage d'un run vers l'agent résident qui l'héberge (n'importe quelle instance).
//!
//! `run_agent` d'un agent hébergé n'est pas lancé ici : il est transmis (`ask`) à son hôte, dont
//! la file garantit un tour à la fois sur la même session. Sans hôte vivant, ou si l'hôte est
//! injoignable, l'appelant lance le run **à la demande**, comme sans résident.

mod eligible;
mod forward;

pub use eligible::host_of;
pub use forward::{Forwarded, forward};
