//! Fin de la sortie d'un run, par morceaux horodatés.

use std::collections::VecDeque;

/// Sortie gardée en mémoire par run (fin de flux).
const MAX_BYTES: usize = 256 * 1024;

#[derive(Default)]
pub struct OutputBuffer {
    chunks: VecDeque<(u64, String)>,
    bytes: usize,
}

impl OutputBuffer {
    pub fn push(&mut self, timestamp: u64, chunk: &str) {
        self.bytes += chunk.len();
        self.chunks.push_back((timestamp, chunk.to_string()));
        while self.bytes > MAX_BYTES && self.chunks.len() > 1 {
            if let Some((_, old)) = self.chunks.pop_front() {
                self.bytes -= old.len();
            }
        }
    }

    /// Sortie accumulée après `since_ms` (toute la fenêtre si `None`).
    pub fn since(&self, since_ms: Option<u64>) -> String {
        self.chunks
            .iter()
            .filter(|(ts, _)| since_ms.is_none_or(|since| *ts > since))
            .map(|(_, c)| c.as_str())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::OutputBuffer;

    #[test]
    fn filters_by_timestamp() {
        let mut b = OutputBuffer::default();
        b.push(10, "a");
        b.push(20, "b");
        assert_eq!(b.since(None), "ab");
        assert_eq!(b.since(Some(10)), "b");
    }

    #[test]
    fn drops_oldest_chunks_over_the_cap() {
        let mut b = OutputBuffer::default();
        let big = "x".repeat(200 * 1024);
        b.push(1, &big);
        b.push(2, &big);
        assert_eq!(b.since(None).len(), big.len());
        assert_eq!(b.since(Some(1)).len(), big.len());
    }
}
