//! Réservation d'un numéro d'instance.

use crate::owners::{self, Busy, Holder};
use std::path::{Path, PathBuf};
use std::time::Duration;

const FIRST_FREE_LIMIT: u32 = 999;
const BEAT: Duration = Duration::from_secs(30);

fn dir() -> PathBuf {
    std::env::temp_dir().join("swarm-control-instances")
}

/// Réserve un numéro (`requested`, ou le premier libre), nomme cette instance et lance le
/// battement de cœur. Rend le numéro.
pub fn claim(requested: Option<u32>) -> Result<u32, String> {
    let number = claim_in(&dir(), requested)?;
    owners::set_instance_name(super::name(number));
    Ok(number)
}

fn claim_in(dir: &Path, requested: Option<u32>) -> Result<u32, String> {
    let candidates: Vec<u32> = match requested {
        Some(n) => vec![n],
        None => (1..=FIRST_FREE_LIMIT).collect(),
    };
    for n in candidates {
        let file = dir.join(format!("{n}.json"));
        match owners::acquire_at(dir, &file, Holder::current("instance")) {
            Ok(lease) => {
                std::thread::spawn(move || {
                    loop {
                        std::thread::sleep(BEAT);
                        lease.touch();
                    }
                });
                return Ok(n);
            }
            Err(Busy::Held(h)) if requested.is_some() => {
                return Err(format!(
                    "--instance {n}: déjà prise par {} (pid {})",
                    h.instance, h.pid
                ));
            }
            Err(Busy::Io(e)) => return Err(format!("numéro d'instance: {e}")),
            Err(_) => {}
        }
    }
    Err(format!("aucun numéro d'instance libre (1..={FIRST_FREE_LIMIT})"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("swarm-inst-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn numbers_are_handed_out_in_order_and_explicit_ones_are_exclusive() {
        let d = temp("order");
        assert_eq!(claim_in(&d, None), Ok(1));
        assert_eq!(claim_in(&d, None), Ok(2));
        assert!(claim_in(&d, Some(2)).unwrap_err().contains("déjà prise"));
        assert_eq!(claim_in(&d, Some(7)), Ok(7));
        assert_eq!(claim_in(&d, None), Ok(3));
        let _ = std::fs::remove_dir_all(&d);
    }
}
