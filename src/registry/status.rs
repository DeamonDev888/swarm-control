//! Statut d'un run.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Running,
    Done,
    Failed,
    Killed,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Running => "running",
            Status::Done => "done",
            Status::Failed => "failed",
            Status::Killed => "killed",
        }
    }

    /// Valeur écrite sur disque (vocabulaire TS : pas de `killed`).
    pub(super) fn disk_str(self) -> &'static str {
        match self {
            Status::Killed => "failed",
            other => other.as_str(),
        }
    }
}
