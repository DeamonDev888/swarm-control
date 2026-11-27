//! File d'attente (FIFO) des tours d'un agent.

use super::turn::Turn;
use std::collections::VecDeque;
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::Duration;

#[derive(Default)]
pub struct Inbox {
    queue: Mutex<VecDeque<Turn>>,
    wake: Condvar,
}

impl Inbox {
    fn lock(&self) -> MutexGuard<'_, VecDeque<Turn>> {
        self.queue.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn push(&self, turn: Turn) {
        self.lock().push_back(turn);
        self.wake.notify_one();
    }

    /// Prochain tour ; attend au plus `wait`. `None` si rien n'est arrivé (ou réveil d'arrêt).
    pub fn pop(&self, wait: Duration) -> Option<Turn> {
        let mut queue = self.lock();
        if queue.is_empty() {
            queue = self.wake.wait_timeout(queue, wait).unwrap_or_else(|e| e.into_inner()).0;
        }
        queue.pop_front()
    }

    /// Vide la file et rend les tours retirés.
    pub fn drain(&self) -> Vec<Turn> {
        self.lock().drain(..).collect()
    }

    pub fn len(&self) -> usize {
        self.lock().len()
    }

    /// Réveille le fil de travail (arrêt).
    pub fn wake_all(&self) {
        self.wake.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(text: &str) -> Turn {
        Turn::new(text.into(), None, None).0
    }

    #[test]
    fn is_first_in_first_out() {
        let inbox = Inbox::default();
        inbox.push(turn("a"));
        inbox.push(turn("b"));
        assert_eq!(inbox.len(), 2);
        assert_eq!(inbox.pop(Duration::from_millis(1)).unwrap().prompt, "a");
        assert_eq!(inbox.pop(Duration::from_millis(1)).unwrap().prompt, "b");
        assert!(inbox.pop(Duration::from_millis(1)).is_none());
    }

    #[test]
    fn pop_wakes_up_when_a_turn_arrives() {
        let inbox = std::sync::Arc::new(Inbox::default());
        let producer = std::sync::Arc::clone(&inbox);
        let handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            producer.push(turn("tard"));
        });
        let got = inbox.pop(Duration::from_secs(5)).expect("réveillé par le push");
        assert_eq!(got.prompt, "tard");
        handle.join().unwrap();
    }

    #[test]
    fn drain_empties_the_queue() {
        let inbox = Inbox::default();
        inbox.push(turn("a"));
        inbox.push(turn("b"));
        assert_eq!(inbox.drain().len(), 2);
        assert_eq!(inbox.len(), 0);
    }
}
