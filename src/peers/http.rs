//! POST HTTP/1.1 brut, une requête par connexion.

use super::url::Target;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

pub enum PostError {
    /// Connexion impossible : rien n'a été envoyé, l'appelant peut se rabattre sur autre chose.
    Unreachable(String),
    /// Échec après l'envoi : la demande a peut-être été traitée.
    Failed(String),
}

pub fn post(
    target: &Target,
    headers: &[(&str, &str)],
    body: &str,
    wait: Duration,
) -> Result<String, PostError> {
    let mut stream = TcpStream::connect((target.host.as_str(), target.port))
        .map_err(|e| PostError::Unreachable(e.to_string()))?;
    let failed = |e: std::io::Error| PostError::Failed(e.to_string());
    stream.set_read_timeout(Some(wait)).map_err(failed)?;

    let mut head = format!(
        "POST {} HTTP/1.1\r\nHost: {}:{}\r\nContent-Type: application/json\r\nAccept: application/json\r\n",
        target.path, target.host, target.port
    );
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    if let Some(token) = crate::server::token() {
        head.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    head.push_str(&format!("Content-Length: {}\r\nConnection: close\r\n\r\n", body.len()));
    stream.write_all(head.as_bytes()).map_err(failed)?;
    stream.write_all(body.as_bytes()).map_err(failed)?;

    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).map_err(failed)?;
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, payload) =
        text.split_once("\r\n\r\n").ok_or_else(|| PostError::Failed("réponse HTTP vide".into()))?;
    Ok(if head.to_lowercase().contains("transfer-encoding: chunked") {
        dechunk(payload)
    } else {
        payload.to_string()
    })
}

fn dechunk(mut s: &str) -> String {
    let mut out = String::new();
    while let Some((size, rest)) = s.split_once("\r\n") {
        let n = usize::from_str_radix(size.trim(), 16).unwrap_or(0);
        if n == 0 || rest.len() < n {
            break;
        }
        out.push_str(&rest[..n]);
        s = rest[n..].trim_start_matches("\r\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::dechunk;

    #[test]
    fn joins_chunks() {
        assert_eq!(dechunk("3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n"), "abcde");
    }
}
