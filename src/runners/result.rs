//! Résultat d'un run.

#[derive(Debug, Clone, Default)]
pub struct RunResult {
    pub result: String,
    pub session_id: Option<String>,
    pub error: Option<String>,
    /// Sortie brute (fin de flux) — diagnostic en cas d'échec.
    pub raw: String,
    pub model: Option<String>,
}

impl RunResult {
    pub fn err(error: impl Into<String>) -> Self {
        RunResult { error: Some(error.into()), ..Default::default() }
    }
}
