//! Lecture/écriture minimale du `config.yaml` Hermes (sans parseur YAML).

fn is_top_level(line: &str) -> bool {
    !line.starts_with(' ') && !line.starts_with('\t') && !line.trim().is_empty()
}

/// `model.default` du `config.yaml`.
pub fn yaml_model(yaml: &str) -> Option<String> {
    let mut in_model = false;
    for line in yaml.lines() {
        if !line.starts_with(' ') && !line.starts_with('\t') {
            in_model = line.trim_end() == "model:";
            continue;
        }
        if in_model {
            if let Some(v) = line.trim().strip_prefix("default:") {
                return Some(v.trim().trim_matches(['"', '\'']).to_string());
            }
        }
    }
    None
}

/// Noms des serveurs de la section `mcp_servers:`.
pub fn yaml_mcp_servers(yaml: &str) -> Vec<String> {
    let mut in_block = false;
    let mut out = Vec::new();
    for line in yaml.lines() {
        if is_top_level(line) {
            in_block = line.trim_end() == "mcp_servers:";
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let t = line.trim();
        if in_block && indent == 2 && t.ends_with(':') && !t.starts_with('-') {
            out.push(t.trim_end_matches(':').to_string());
        }
    }
    out
}

/// Le document sans sa section `mcp_servers:` (ligne d'en-tête et bloc indenté).
pub fn strip_mcp_section(yaml: &str) -> String {
    let mut kept = Vec::new();
    let mut skipping = false;
    for line in yaml.lines() {
        if is_top_level(line) {
            skipping = line.trim_end() == "mcp_servers:";
        }
        if !skipping {
            kept.push(line);
        }
    }
    kept.join("\n").trim_end().to_string()
}

/// Scalaire YAML sûr : guillemets doubles dès qu'un caractère spécial apparaît.
pub fn yaml_scalar(s: &str) -> String {
    let plain = !s.is_empty()
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"/._-:@+=%\\".contains(&b))
        && !s.starts_with(['-', ':', '@', '%']);
    if plain {
        s.to_string()
    } else {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "model:\n  default: glm-5.3-flash\n  provider: zai\nmcp_servers:\n  memory:\n    url: http://x\n  db:\n    command: node\nagent:\n  max_turns: 4\n";

    #[test]
    fn reads_model_and_servers() {
        assert_eq!(yaml_model(SAMPLE).as_deref(), Some("glm-5.3-flash"));
        assert_eq!(yaml_mcp_servers(SAMPLE), vec!["memory".to_string(), "db".to_string()]);
    }

    #[test]
    fn strips_only_the_mcp_section() {
        let stripped = strip_mcp_section(SAMPLE);
        assert!(!stripped.contains("memory"));
        assert!(stripped.contains("agent:\n  max_turns: 4"));
        assert!(stripped.contains("provider: zai"));
    }

    #[test]
    fn quotes_special_scalars() {
        assert_eq!(yaml_scalar("http://localhost:3099/mcp"), "http://localhost:3099/mcp");
        assert_eq!(yaml_scalar("a b"), "\"a b\"");
    }
}
