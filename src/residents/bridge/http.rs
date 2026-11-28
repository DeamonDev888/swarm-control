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
    /// En-têtes, noms en minuscules.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    /// Valeur d'un en-tête (nom insensible à la casse).
    pub fn header(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        self.headers.iter().find(|(n, _)| *n == name).map(|(_, v)| v.as_str())
    }
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
    let mut headers = Vec::new();
    for line in lines {
        let Some((name, value)) = line.split_once(':') else { continue };
        let name = name.trim().to_ascii_lowercase();
        headers.push((name.clone(), value.trim().to_string()));
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
    Ok(Request { method, path, headers, body })
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
        202 => "Accepted",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        411 => "Length Required",
        413 => "Payload Too Large",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Internal Server Error",
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Le client a-t-il fermé la connexion ? Lecture non bloquante d'un octet (sans le consommer) :
/// `Ok(0)` = fin de flux. Ne touche pas au délai de lecture courant.
pub fn client_gone(stream: &mut TcpStream) -> bool {
    let previous = stream.read_timeout().ok().flatten();
    if stream.set_nonblocking(true).is_err() {
        return false;
    }
    let mut byte = [0u8; 1];
    let gone = match stream.peek(&mut byte) {
        Ok(0) => true,
        Ok(_) => false,
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => false,
        Err(_) => true,
    };
    let _ = stream.set_nonblocking(false);
    if let Some(delay) = previous {
        let _ = stream.set_read_timeout(Some(delay));
    }
    gone
}

#[cfg(test)]
mod tests {
    use super::{client_gone, reason, respond};
    use serde_json::json;
    use std::io::Read;
    use std::net::{TcpListener, TcpStream};
    use std::time::{Duration, Instant};

    /// Les statuts du pont ont tous une raison HTTP ; l'accusé async et la file pleine inclus.
    #[test]
    fn every_status_of_the_bridge_has_its_reason() {
        assert_eq!(reason(202), "Accepted");
        assert_eq!(reason(429), "Too Many Requests");
        for status in [200, 202, 400, 401, 404, 405, 411, 413, 429, 431, 503, 504] {
            assert_ne!(reason(status), "Internal Server Error", "statut {status}");
        }
        assert_eq!(reason(418), "Internal Server Error", "inconnu : raison générique");
    }

    /// Une réponse s'écrit en entier : ligne de statut, en-têtes, corps JSON, puis la connexion
    /// est fermée (le client lit jusqu'à la fin de flux).
    #[test]
    fn a_response_is_written_in_full_then_the_connection_closes() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        respond(&mut server, 429, &json!({"error": "file pleine"}));
        drop(server);
        let mut reponse = String::new();
        client.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        client.read_to_string(&mut reponse).unwrap();
        assert!(reponse.starts_with("HTTP/1.1 429 Too Many Requests\r\n"), "{reponse}");
        assert!(reponse.contains("Content-Type: application/json"), "{reponse}");
        assert!(reponse.ends_with("{\"error\":\"file pleine\"}"), "{reponse}");
    }

    #[test]
    fn sees_a_client_close_and_keeps_a_live_one() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let client = TcpStream::connect(addr).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        assert!(!client_gone(&mut server), "client connecté");
        drop(client);
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if client_gone(&mut server) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("fermeture du client non détectée");
    }
}
