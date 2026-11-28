//! Horloge murale en millisecondes Unix.

use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_ms() -> u64 {
    millis_since(SystemTime::now())
}

/// Écart en millisecondes entre `t` et l'époque Unix (0 si l'horloge est avant 1970).
fn millis_since(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{millis_since, now_ms};
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn now_ms_is_a_millisecond_unix_timestamp() {
        // Ordre de grandeur : entre 2020 et 2100 (en ms), jamais en secondes ni en µs.
        let now = now_ms();
        assert!(now > 1_577_836_800_000, "trop petit (des secondes ?) : {now}");
        assert!(now < 4_102_444_800_000, "trop grand (des microsecondes ?) : {now}");
    }

    #[test]
    fn now_ms_is_monotonic_across_calls() {
        let first = now_ms();
        let mut previous = first;
        for _ in 0..50 {
            let current = now_ms();
            assert!(current >= previous, "l'horloge a reculé : {previous} → {current}");
            previous = current;
        }
        assert!(previous - first < 5_000, "50 appels ont pris plus de 5 s");
    }

    #[test]
    fn millis_since_converts_each_precision() {
        assert_eq!(millis_since(UNIX_EPOCH), 0);
        assert_eq!(millis_since(UNIX_EPOCH + Duration::from_millis(1)), 1);
        assert_eq!(millis_since(UNIX_EPOCH + Duration::from_secs(3)), 3_000);
        // Les sous-millisecondes sont tronquées, pas arrondies.
        assert_eq!(millis_since(UNIX_EPOCH + Duration::from_micros(1_999)), 1);
        assert_eq!(millis_since(UNIX_EPOCH + Duration::from_nanos(999_999)), 0);
        // Avant l'époque : pas de panique, 0.
        assert_eq!(millis_since(UNIX_EPOCH - Duration::from_secs(1)), 0);
    }
}
