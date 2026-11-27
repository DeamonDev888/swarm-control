//! Arguments et environnement vus par le faux CLI.

pub struct Args {
    pub list: Vec<String>,
    pub hermes_home: String,
}

impl Args {
    pub fn from_env() -> Self {
        Args {
            list: std::env::args().skip(1).collect(),
            hermes_home: std::env::var("HERMES_HOME").unwrap_or_default(),
        }
    }

    pub fn has(&self, word: &str) -> bool {
        self.list.iter().any(|a| a == word)
    }

    /// Valeur qui suit `flag`.
    pub fn after(&self, flag: &str) -> Option<String> {
        let i = self.list.iter().position(|a| a == flag)?;
        self.list.get(i + 1).cloned()
    }

    pub fn is_hermes(&self) -> bool {
        self.has("chat")
    }

    pub fn is_antigravity(&self) -> bool {
        std::env::var("SWARM_RUNNER").as_deref() == Ok("antigravity")
    }
}
