//! Lecture des flux stdout / stderr dans des fils dédiés.

use super::utf8::take_utf8;
use crate::registry::LiveRun;
use std::io::Read;
use std::process::Child;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Plafond du tampon final par flux (fin de flux conservée).
const MAX_CAPTURE: usize = 10 * 1024 * 1024;
/// Un petit-enfant détaché peut garder un pipe ouvert : attente bornée.
const JOIN_GRACE: Duration = Duration::from_secs(3);

pub type Capture = Arc<Mutex<Vec<u8>>>;

pub fn new_capture() -> Capture {
    Arc::new(Mutex::new(Vec::new()))
}

/// Démarre les lecteurs des deux flux du process.
pub fn start(
    child: &mut Child,
    live: &Arc<LiveRun>,
    out: &Capture,
    err: &Capture,
) -> Vec<JoinHandle<()>> {
    [
        child.stdout.take().map(|s| spawn(s, Arc::clone(live), Arc::clone(out))),
        child.stderr.take().map(|s| spawn(s, Arc::clone(live), Arc::clone(err))),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// Attend la fin des lecteurs, au plus [`JOIN_GRACE`].
pub fn join_all(handles: Vec<JoinHandle<()>>) {
    let deadline = Instant::now() + JOIN_GRACE;
    for handle in handles {
        while !handle.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        if handle.is_finished() {
            let _ = handle.join();
        }
    }
}

pub fn text(capture: &Capture) -> String {
    let bytes = capture.lock().unwrap_or_else(|e| e.into_inner());
    String::from_utf8_lossy(&bytes).into_owned()
}

fn spawn<R: Read + Send + 'static>(
    mut src: R,
    live: Arc<LiveRun>,
    sink: Capture,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        let mut carry: Vec<u8> = Vec::new();
        loop {
            let n = match src.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            append_capped(&sink, &buf[..n]);
            carry.extend_from_slice(&buf[..n]);
            live.push_output(&take_utf8(&mut carry));
        }
        if !carry.is_empty() {
            live.push_output(&String::from_utf8_lossy(&carry));
        }
    })
}

fn append_capped(sink: &Capture, chunk: &[u8]) {
    let mut s = sink.lock().unwrap_or_else(|e| e.into_inner());
    s.extend_from_slice(chunk);
    if s.len() > MAX_CAPTURE {
        let excess = s.len() - MAX_CAPTURE;
        s.drain(..excess);
    }
}
