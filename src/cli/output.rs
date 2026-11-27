//! Affichage : JSON brut (`--json`) et résumés lisibles.
//!
//! Tout ce qui est purement calculatoire (tableau, dates, durées, troncature) vit ici, sous forme
//! de fonctions testables ; les commandes se contentent d'imprimer le résultat.

use serde_json::Value;

/// Imprime un document JSON indenté sur stdout.
pub fn json(value: &Value) {
    match serde_json::to_string_pretty(value) {
        Ok(text) => println!("{text}"),
        Err(e) => eprintln!("swarm-control: JSON illisible ({e})"),
    }
}

/// Tableau en colonnes alignées (deux espaces entre colonnes, sans espace final).
pub fn table(headers: &[&str], rows: &[Vec<String>]) -> String {
    let columns = headers.iter().count().max(rows.iter().map(Vec::len).max().unwrap_or(0));
    let mut widths: Vec<usize> = vec![0; columns];
    for (i, header) in headers.iter().enumerate() {
        widths[i] = header.chars().count();
    }
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(cell.chars().count());
        }
    }
    let mut out = String::new();
    let head: Vec<String> =
        (0..columns).map(|i| headers.get(i).copied().unwrap_or_default().to_string()).collect();
    push_row(&mut out, &head, &widths);
    for row in rows {
        push_row(&mut out, row, &widths);
    }
    out
}

fn push_row(out: &mut String, cells: &[String], widths: &[usize]) {
    let mut line = String::new();
    for (i, width) in widths.iter().enumerate() {
        if i > 0 {
            line.push_str("  ");
        }
        let cell = cells.get(i).map(String::as_str).unwrap_or_default();
        line.push_str(cell);
        let pad = width.saturating_sub(cell.chars().count());
        line.extend(std::iter::repeat_n(' ', pad));
    }
    out.push_str(line.trim_end());
    out.push('\n');
}

/// Horodatage lisible en UTC : `2026-11-27 14:03:12Z` (sans dépendance de date).
pub fn stamp(ms: u64) -> String {
    let secs = ms / 1000;
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Jours depuis l'époque → (année, mois, jour), calendrier grégorien (algorithme de Hinnant).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// Durée lisible : `950 ms`, `1,5 s`, `2 min 05 s`.
pub fn duration(ms: u64) -> String {
    if ms < 1_000 {
        format!("{ms} ms")
    } else if ms < 60_000 {
        format!("{},{:01} s", ms / 1_000, (ms % 1_000) / 100)
    } else {
        format!("{} min {:02} s", ms / 60_000, (ms % 60_000) / 1_000)
    }
}

/// Une ligne : espaces compactés, tronquée à `max` caractères (ellipse comprise).
pub fn clip(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if max == 0 {
        return String::new();
    }
    if flat.chars().count() <= max {
        return flat;
    }
    let keep: String = flat.chars().take(max - 1).collect();
    format!("{keep}…")
}

#[cfg(test)]
mod tests {
    use super::{clip, duration, stamp, table};

    #[test]
    fn tables_align_columns_and_skip_trailing_spaces() {
        let rows = vec![vec!["x".to_string(), "y".to_string()], vec!["long".into(), "z".into()]];
        assert_eq!(table(&["A", "BB"], &rows), "A     BB\nx     y\nlong  z\n");
        assert_eq!(table(&["SEUL"], &[]), "SEUL\n");
        // Lignes plus courtes que l'en-tête : pas de panique, colonnes manquantes vides.
        assert_eq!(table(&["A", "B"], &[vec!["1".into()]]), "A  B\n1\n");
    }

    #[test]
    fn stamps_are_utc_and_stable() {
        assert_eq!(stamp(0), "1970-01-01 00:00:00Z");
        assert_eq!(stamp(1_600_000_000_000), "2020-09-13 12:26:40Z");
        assert_eq!(stamp(1_700_000_000_000), "2023-11-14 22:13:20Z");
        // Une date bissextile.
        assert_eq!(stamp(1_709_164_800_000), "2024-02-29 00:00:00Z");
    }

    #[test]
    fn durations_read_naturally() {
        assert_eq!(duration(0), "0 ms");
        assert_eq!(duration(950), "950 ms");
        assert_eq!(duration(1_500), "1,5 s");
        assert_eq!(duration(65_000), "1 min 05 s");
    }

    #[test]
    fn clips_flatten_and_shorten() {
        assert_eq!(clip("a\n\n  b", 10), "a b");
        assert_eq!(clip("abcdef", 10), "abcdef");
        assert_eq!(clip("abcdef", 4), "abc…");
        assert_eq!(clip("abcdef", 0), "");
    }
}
