//! Mises en forme réutilisées par les panneaux de la TUI : durées, troncatures, ports.
//!
//! Fonctions pures, testées sans ratatui ; `ui.rs` ne fait qu'appliquer les couleurs.

/// Durée lisible : « 450 ms », « 12 s », « 2m13s », « 1h04m ».
pub fn human_ms(ms: u64) -> String {
    match ms {
        0..=999 => format!("{ms} ms"),
        1_000..=59_999 => format!("{} s", ms / 1000),
        60_000..=3_599_999 => format!("{}m{:02}s", ms / 60_000, (ms % 60_000) / 1000),
        _ => format!("{}h{:02}m", ms / 3_600_000, (ms % 3_600_000) / 60_000),
    }
}

/// Raccourcit une chaîne par caractères (`…` final), sans couper au milieu d'un caractère.
pub fn short(text: &str, max: usize) -> String {
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    if text.chars().count() > max && max > 0 {
        out.push('…');
    }
    out
}

/// Une entrée de journal sur une ligne : retours chariot remplacés, longueur bornée.
pub fn clip(text: &str) -> String {
    let flat = text.lines().collect::<Vec<_>>().join(" ⏎ ");
    let flat = if text.starts_with('\n') || text.starts_with("\r\n") {
        format!(" ⏎{flat}")
    } else {
        flat
    };
    short(&flat, MAX_CLIP)
}

/// Fin d'un texte (jusqu'à `max` octets), coupé sur une frontière de caractère.
pub fn tail(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut start = text.len() - max;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    text[start..].to_string()
}

/// Coupure d'un texte en lignes d'au plus `width` caractères (emballage naïf par
/// caractères : suffisant pour des pan­neaux de diagnostic, sans dépendance externe).
pub fn wrap_chars(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut out = Vec::new();
    for line in text.lines() {
        let chars: Vec<char> = line.chars().collect();
        if chars.is_empty() {
            out.push(String::new());
            continue;
        }
        for chunk in chars.chunks(width) {
            out.push(chunk.iter().collect());
        }
    }
    if text.ends_with('\n') {
        out.push(String::new());
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

/// Port d'une URL `http://hôte:port/chemin` (`None` si l'URL n'en porte pas).
pub fn port_of(url: &str) -> Option<u16> {
    let rest = url.strip_prefix("http://")?;
    let authority = rest.split('/').next()?;
    let port = authority.rsplit_once(':')?.1.parse().ok()?;
    (port > 0).then_some(port)
}

/// Longueur maximale d'un prompt / réponse / erreur affichés dans la conversation.
const MAX_CLIP: usize = 256;
