//! Résultat d'un process terminé.

/// Pourquoi l'attente s'est terminée.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum End {
    Exited,
    TimedOut,
    Killed,
    Cancelled,
}

#[derive(Debug)]
pub struct ProcOutput {
    pub stdout: String,
    pub stderr: String,
    pub code: Option<i32>,
    pub end: End,
}
