//! Les runners pris en charge.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Runner {
    Claude,
    Hermes,
}

impl Runner {
    pub fn as_str(self) -> &'static str {
        match self {
            Runner::Claude => "claude",
            Runner::Hermes => "hermes",
        }
    }

    pub fn parse(s: &str) -> Option<Runner> {
        match s {
            "claude" => Some(Runner::Claude),
            "hermes" => Some(Runner::Hermes),
            _ => None,
        }
    }
}
