//! HTTP/1.1 minimal : lecture d'une requête, écriture d'une réponse JSON.

use serde_json::Value;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

/// Plafonds anti-abus : en-têtes et corps.
const MAX_HEAD: usize = 64 * 1024;
const MAX_BODY: usize = 1024 * 1024;
const READ_TIMEOUT: Duration = Duration::from_secs(30);

pub struct Request {
    pub method: String,
    /// Chemin sans la query (`/send`).
    pub path: String,
    pub body: Vec<u8>,
}

/// Lit une requête. L'erreur porte le statut HTTP à renvoyer et son message.
pub fn read(stream: &mut TcpStream) -> Result<Request, (u16, String)> {
    let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(i) = find(&buf, b"\r\n\r\n") {
            break i;
        }
        if buf.len() > MAX_HEAD {
            return Err((431, "en-têtes trop longs".into()));
        }
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return Err((400, "requête incomplète".into())),
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut first = lines.next().unwrap_or_default().split_whitespace();
    let (method, target) = match (first.next(), first.next()) {
        (Some(m), Some(t)) => (m.to_string(), t),
        _ => return Err((400, "ligne de requête invalide".into())),
    };
    let path = target.split('?').next().unwrap_or_default().to_string();

    let mut length = 0usize;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else { continue };
        let name = name.trim().to_ascii_lowercase();
        if name == "content-length" {
            length =
                value.trim().parse().map_err(|_| (400, "Content-Length invalide".to_string()))?;
        } else if name == "transfer-encoding" {
            return Err((411, "Content-Length requis (pas de transfert par morceaux)".into()));
        }
    }
    if length > MAX_BODY {
        return Err((413, "corps trop volumineux".into()));
    }
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < length {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return Err((400, "corps incomplet".into())),
            Ok(n) => body.extend_from_slice(&chunk[..n]),
        }
    }
    body.truncate(length);
    Ok(Request { method, path, body })
}

/// Écrit une réponse JSON et ferme la connexion (`Connection: close`).
pub fn respond(stream: &mut TcpStream, status: u16, body: &Value) {
    let payload = body.to_string();
    let head = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: application/json; charset=utf-8\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n",
        reason(status),
        payload.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(payload.as_bytes());
    let _ = stream.flush();
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        411 => "Length Required",
        413 => "Payload Too Large",
        431 => "Request Header Fields Too Large",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Internal Server Error",
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}
