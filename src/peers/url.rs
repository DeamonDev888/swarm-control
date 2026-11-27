//! Découpe d'une URL `http://hôte:port/chemin`.

pub struct Target {
    pub host: String,
    pub port: u16,
    pub path: String,
}

pub fn parse(url: &str) -> Option<Target> {
    let rest = url.strip_prefix("http://")?;
    let (authority, path) = match rest.split_once('/') {
        Some((a, p)) => (a, format!("/{p}")),
        None => (rest, "/".to_string()),
    };
    let (host, port) = authority.rsplit_once(':')?;
    Some(Target { host: host.to_string(), port: port.parse().ok()?, path })
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn splits_host_port_and_path() {
        let t = parse("http://127.0.0.1:23082/mcp").unwrap();
        assert_eq!((t.host.as_str(), t.port, t.path.as_str()), ("127.0.0.1", 23082, "/mcp"));
        assert!(parse("https://x:1/mcp").is_none());
        assert!(parse("http://x/mcp").is_none());
    }
}
