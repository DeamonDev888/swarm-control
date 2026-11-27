//! Mise en forme de sorties de process.

use crate::process::ProcOutput;

/// Fin de `s`, tronquée à `max` octets sur une frontière de caractère.
pub fn tail(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut start = s.len() - max;
    while !s.is_char_boundary(start) {
        start += 1;
    }
    format!("…{}", &s[start..])
}

/// Sortie brute pour le diagnostic : fin de stdout, puis fin de stderr.
pub fn join_raw(out: &ProcOutput) -> String {
    let mut s = tail(&out.stdout, 20_000);
    if !out.stderr.trim().is_empty() {
        s.push_str("\n\n--- STDERR ---\n");
        s.push_str(&tail(&out.stderr, 8_000));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::tail;

    #[test]
    fn tail_respects_char_boundaries() {
        assert_eq!(tail("abc", 10), "abc");
        assert_eq!(tail("aéé", 3), "…é");
    }
}
