//! Garde anti-panique d'un run.

use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};

/// Exécute `f` ; une panique devient `Err(message)` au lieu de tuer le fil.
///
/// Sans cela, un run qui panique resterait « running » à jamais — et son bail,
/// renouvelé par le balayeur, bloquerait l'agent pour toutes les instances.
pub fn catch<T>(f: impl FnOnce() -> T) -> Result<T, String> {
    catch_unwind(AssertUnwindSafe(f)).map_err(|payload| message(payload.as_ref()))
}

fn message(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "panique inconnue".to_string())
}

#[cfg(test)]
mod tests {
    use super::catch;

    #[test]
    fn a_normal_value_passes_through() {
        assert_eq!(catch(|| 7), Ok(7));
    }

    #[test]
    fn a_panic_becomes_an_error_with_its_message() {
        let err = catch(|| -> i32 { panic!("boum {}", 42) }).unwrap_err();
        assert!(err.contains("boum 42"), "{err}");
        let err = catch(|| -> i32 { panic!("statique") }).unwrap_err();
        assert!(err.contains("statique"), "{err}");
    }
}
