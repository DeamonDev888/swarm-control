//! Erreur correspondant à une fin anormale de process.

use crate::process::{End, ProcOutput};
use std::time::Duration;

/// `None` si le process s'est terminé de lui-même.
pub fn end_error(out: &ProcOutput, timeout: Duration) -> Option<String> {
    match out.end {
        End::Exited => None,
        End::TimedOut => Some(format!("TIMEOUT: arrêté après {}s.", timeout.as_secs())),
        End::Killed => Some("KILLED: arrêté par agent_control.".into()),
        End::Cancelled => Some("CANCELLED: requête annulée par le client.".into()),
    }
}
