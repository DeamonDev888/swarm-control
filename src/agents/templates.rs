//! Modèles d'agents (PLAN §4.1) : persona, serveurs MCP attendus, variables requises,
//! défauts `executable` / `unit` / port de pont.
//!
//! Les modèles intégrés vivent dans `assets/templates/*.json` (persona dans le `.md`
//! voisin) et sont embarqués par `include_str!` ; un workspace peut les surcharger ou en
//! ajouter via `<workspace>/templates/<nom>.json` (+ `<nom>.md` pour la persona). Les
//! chaînes de la persona sont interpolées (`$VAR` / `${VAR}`) avec
//! `config::interpolate_json`, et les variables requises sont vérifiées avant création.
//!
//! Consommateurs : `create_agent {template}` (instanciation), `list_templates` et
//! `swarm-control templates` (liste), `doctor --agent X` (prérequis).

use super::files::io;
use super::mcp::mcp_server_names;
use super::paths::{antigravity_settings_path, claude_settings_path, hermes_profile_dir};
use super::runner::Runner;
use crate::config;
use crate::store::{read_json, write_json_atomic};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::Path;

/// Modèles intégrés : `(nom, JSON, persona Markdown)`, embarqués dans le binaire.
const BUILT_INS: &[(&str, &str, &str)] = &[
    (
        "browser",
        include_str!("../../assets/templates/browser.json"),
        include_str!("../../assets/templates/browser.md"),
    ),
    (
        "coder",
        include_str!("../../assets/templates/coder.json"),
        include_str!("../../assets/templates/coder.md"),
    ),
    (
        "coordinator",
        include_str!("../../assets/templates/coordinator.json"),
        include_str!("../../assets/templates/coordinator.md"),
    ),
    (
        "custom",
        include_str!("../../assets/templates/custom.json"),
        include_str!("../../assets/templates/custom.md"),
    ),
    (
        "discord",
        include_str!("../../assets/templates/discord.json"),
        include_str!("../../assets/templates/discord.md"),
    ),
    (
        "explorer",
        include_str!("../../assets/templates/explorer.json"),
        include_str!("../../assets/templates/explorer.md"),
    ),
    (
        "google",
        include_str!("../../assets/templates/google.json"),
        include_str!("../../assets/templates/google.md"),
    ),
    (
        "monitor",
        include_str!("../../assets/templates/monitor.json"),
        include_str!("../../assets/templates/monitor.md"),
    ),
    (
        "verifier",
        include_str!("../../assets/templates/verifier.json"),
        include_str!("../../assets/templates/verifier.md"),
    ),
    (
        "whatsapp",
        include_str!("../../assets/templates/whatsapp.json"),
        include_str!("../../assets/templates/whatsapp.md"),
    ),
    (
        "x",
        include_str!("../../assets/templates/x.json"),
        include_str!("../../assets/templates/x.md"),
    ),
];

/// Un modèle d'agent, tel qu'écrit dans son JSON.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Template {
    /// Identifiant du modèle : doit valoir le nom du fichier (`<nom>.json`).
    pub name: String,
    pub description: String,
    /// Classe affichable (logo/archétype), publiée plus tard par `status` / `list_agents`.
    #[serde(default)]
    pub class: Option<String>,
    /// Persona : fichier Markdown voisin du JSON (défaut `<nom>.md`).
    #[serde(default)]
    pub prompt_file: Option<String>,
    /// Serveurs MCP attendus (noms seulement, jamais de secret).
    #[serde(default)]
    pub expected_mcp_servers: Vec<String>,
    /// Variables d'environnement requises (env système ou `.env` du workspace).
    #[serde(default)]
    pub required_vars: Vec<String>,
    /// Défauts appliqués à la création quand l'appel ne les précise pas.
    #[serde(default)]
    pub defaults: Defaults,
}

/// Défauts d'un modèle : `executable` / `unit` / port de pont, runner par défaut.
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Defaults {
    /// Runner utilisé si l'appel n'en précise pas (à défaut : claude).
    #[serde(default)]
    pub runner: Option<String>,
    #[serde(default)]
    pub executable: bool,
    #[serde(default)]
    pub unit: bool,
    /// Port du pont HTTP (`--bridge`) conseillé pour cet agent.
    #[serde(default)]
    pub bridge_port: Option<u16>,
}

/// D'où vient un modèle chargé.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    /// Embarqué dans le binaire (`assets/templates/`).
    BuiltIn,
    /// Surcharge ou ajout du workspace (`<workspace>/templates/`).
    Workspace,
}

impl Source {
    /// Étiquette stable, telle que renvoyée dans les JSON d'outils.
    pub fn as_str(self) -> &'static str {
        match self {
            Source::BuiltIn => "builtin",
            Source::Workspace => "workspace",
        }
    }

    /// Étiquette lisible, pour la CLI.
    pub fn label(self) -> &'static str {
        match self {
            Source::BuiltIn => "intégré",
            Source::Workspace => "surcharge",
        }
    }
}

/// Modèle chargé : définition validée et persona interpolée, prêtes à instancier.
pub struct Loaded {
    pub template: Template,
    /// Runner par défaut du modèle, déjà validé (`None` → claude côté appelant).
    pub runner: Option<Runner>,
    /// Persona (`promptFile`), variables `$VAR` / `${VAR}` interpolées.
    pub prompt: String,
    pub source: Source,
}

impl Loaded {
    /// Classe du modèle (`None` : pas de classe posée — dérivable plus tard).
    pub fn class(&self) -> Option<&str> {
        self.template.class.as_deref()
    }
}

/// Entrée de liste : un modèle sans sa persona.
pub struct TemplateInfo {
    pub name: String,
    pub description: String,
    pub class: Option<String>,
    pub required_vars: Vec<String>,
    pub expected_mcp_servers: Vec<String>,
    pub defaults: Defaults,
    pub source: Source,
}

impl TemplateInfo {
    /// Ligne JSON, telle que renvoyée par `list_templates` et `swarm-control templates --json`.
    pub fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "description": self.description,
            "class": self.class,
            "requiredVars": self.required_vars,
            "expectedMcpServers": self.expected_mcp_servers,
            "defaults": defaults_json(&self.defaults),
            "source": self.source.as_str(),
        })
    }
}

/// Défauts d'un modèle, en JSON (le runner non précisé est affiché à sa valeur effective).
fn defaults_json(d: &Defaults) -> Value {
    json!({
        "runner": d.runner.clone().unwrap_or_else(|| "claude".into()),
        "executable": d.executable,
        "unit": d.unit,
        "bridgePort": d.bridge_port,
    })
}

/// Tous les modèles disponibles : intégrés, remplacés ou complétés par
/// `<workspace>/templates/`, triés par nom. Une surcharge illisible n'apparaît pas ici
/// ([`load`] signalera précisément son erreur quand on demandera son nom).
pub fn list(ws: &Path) -> Vec<TemplateInfo> {
    let mut out: Vec<TemplateInfo> = BUILT_INS
        .iter()
        .filter_map(|(_, doc, _)| serde_json::from_str::<Template>(doc).ok())
        .map(|t| info(t, Source::BuiltIn))
        .collect();
    for (name, doc) in workspace_templates(ws) {
        if let Ok(t) = serde_json::from_str::<Template>(&doc) {
            let entry = info(t, Source::Workspace);
            match out.iter_mut().find(|i| i.name == name) {
                Some(slot) => *slot = entry,
                None => out.push(entry),
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn info(t: Template, source: Source) -> TemplateInfo {
    TemplateInfo {
        name: t.name.clone(),
        description: t.description.clone(),
        class: t.class.clone(),
        required_vars: t.required_vars.clone(),
        expected_mcp_servers: t.expected_mcp_servers.clone(),
        defaults: t.defaults,
        source,
    }
}

/// `(nom, contenu)` des `*.json` de `<workspace>/templates/`, triés par nom.
fn workspace_templates(ws: &Path) -> Vec<(String, String)> {
    let dir = ws.join("templates");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if let Ok(doc) = std::fs::read_to_string(&path) {
            out.push((name.to_string(), doc));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Charge un modèle par nom : `<workspace>/templates/<nom>.json` d'abord, modèle intégré
/// sinon. La persona (`promptFile`, défaut `<nom>.md`) vient de la surcharge du workspace
/// quand elle existe, du Markdown embarqué sinon, puis est interpolée.
pub fn load(ws: &Path, name: &str) -> Result<Loaded, String> {
    let override_path = ws.join("templates").join(format!("{name}.json"));
    let (doc, source, origin) = if override_path.is_file() {
        let text = std::fs::read_to_string(&override_path).map_err(|e| {
            format!("modèle '{name}' illisible ({}) : {e}", override_path.display())
        })?;
        (text, Source::Workspace, override_path.display().to_string())
    } else if let Some((_, doc, _)) = BUILT_INS.iter().find(|(n, _, _)| *n == name) {
        (doc.to_string(), Source::BuiltIn, "modèle intégré".to_string())
    } else {
        return Err(format!(
            "modèle inconnu '{name}' (intégrés : {}) — ou dépose {} pour l'ajouter.",
            builtin_names().join(", "),
            override_path.display()
        ));
    };
    let template: Template = serde_json::from_str(&doc)
        .map_err(|e| format!("modèle '{name}' illisible ({origin}) : {e}"))?;
    if template.name != name {
        return Err(format!(
            "modèle '{origin}' : son champ name (« {} ») doit valoir « {name} ».",
            template.name
        ));
    }
    let runner = match template.defaults.runner.as_deref() {
        Some(r) => Some(Runner::parse(r).ok_or_else(|| {
            format!(
                "modèle '{name}' : runner par défaut inconnu '{r}' (claude | hermes | antigravity)."
            )
        })?),
        None => None,
    };
    let prompt = load_prompt(ws, name, template.prompt_file.as_deref())?;
    Ok(Loaded { template, runner, prompt, source })
}

/// Noms des modèles embarqués, dans l'ordre de déclaration.
fn builtin_names() -> Vec<&'static str> {
    BUILT_INS.iter().map(|(n, _, _)| *n).collect()
}

/// Persona d'un modèle : `<workspace>/templates/<promptFile>` d'abord, Markdown embarqué
/// du modèle intégré de même nom sinon ; `$VAR` / `${VAR}` interpolés.
fn load_prompt(ws: &Path, name: &str, prompt_file: Option<&str>) -> Result<String, String> {
    let file = match prompt_file {
        Some(f) => f.to_string(),
        None => format!("{name}.md"),
    };
    let override_path = ws.join("templates").join(&file);
    let raw = if override_path.is_file() {
        std::fs::read_to_string(&override_path).map_err(|e| {
            format!("persona du modèle '{name}' illisible ({}) : {e}", override_path.display())
        })?
    } else if let Some((_, _, md)) = BUILT_INS.iter().find(|(n, _, _)| *n == name) {
        md.to_string()
    } else {
        return Err(format!(
            "persona du modèle '{name}' introuvable : dépose {} (ou pointe promptFile vers un \
             fichier existant).",
            override_path.display()
        ));
    };
    match config::interpolate_json(&Value::String(raw)) {
        Value::String(s) => Ok(s),
        _ => unreachable!("interpolate_json d'une chaîne rend une chaîne"),
    }
}

/// Variables requises absentes de l'environnement (système ou `.env` déjà chargé).
pub fn missing_vars(template: &Template) -> Vec<String> {
    template.required_vars.iter().filter(|v| config::var(v).is_none()).cloned().collect()
}

/// Serveurs MCP attendus qui ne figure pas dans le `.mcp.json` du workspace (noms
/// seulement : ce contrôle ne touche jamais à un secret).
pub fn missing_servers(ws: &Path, template: &Template) -> Vec<String> {
    let declared = mcp_server_names(ws);
    template.expected_mcp_servers.iter().filter(|s| !declared.contains(*s)).cloned().collect()
}

/// Marque le modèle et sa classe dans la définition de l'agent, juste après sa création :
/// `settings_<nom>.json` (Claude, Antigravity) ou `swarm.json` dans le profil (Hermes).
/// Un échec n'annule pas la création : l'appelant le signale en avertissement.
#[allow(dead_code)] // la création passe par stamp_with_port ; resté pour le test de
// residents/status.rs (fichier hors périmètre de cette vague).
pub fn stamp(
    ws: &Path,
    name: &str,
    runner: Runner,
    template: &str,
    class: Option<&str>,
) -> Result<(), String> {
    stamp_with_port(ws, name, runner, template, class, None)
}

/// Comme [`stamp`], en posant aussi le port de pont conseillé par le modèle (champ
/// `bridgePort`, défauts du modèle). `None` n'efface pas un port déjà posé — même
/// sémantique que la classe ; l'application au lancement (défaut de `--bridge`) est
/// une vague séparée, qui lira [`bridge_port_of`].
pub fn stamp_with_port(
    ws: &Path,
    name: &str,
    runner: Runner,
    template: &str,
    class: Option<&str>,
    bridge_port: Option<u16>,
) -> Result<(), String> {
    match runner {
        Runner::Hermes => {
            let dir = hermes_profile_dir(name);
            std::fs::create_dir_all(&dir).map_err(io)?;
            let mut doc = json!({ "template": template });
            if let Some(c) = class {
                doc["class"] = json!(c);
            }
            if let Some(p) = bridge_port {
                doc["bridgePort"] = json!(p);
            }
            write_json_atomic(&dir.join("swarm.json"), &doc).map_err(io)
        }
        Runner::Claude | Runner::Antigravity => {
            let path = match runner {
                Runner::Claude => claude_settings_path(ws, name),
                _ => antigravity_settings_path(ws, name),
            };
            let mut doc = read_json(&path).ok_or_else(|| {
                format!("définition de '{name}' introuvable ({})", path.display())
            })?;
            doc["template"] = json!(template);
            if let Some(c) = class {
                doc["class"] = json!(c);
            }
            if let Some(p) = bridge_port {
                doc["bridgePort"] = json!(p);
            }
            write_json_atomic(&path, &doc).map_err(io)
        }
    }
}

/// Marque lue dans la définition d'un agent : `(nom du modèle, classe, port de pont
/// conseillé)` — tous `None` pour un agent créé sans modèle (port non posé sinon).
fn stamped(ws: &Path, name: &str, runner: Runner) -> (Option<String>, Option<String>, Option<u16>) {
    let path = match runner {
        Runner::Hermes => hermes_profile_dir(name).join("swarm.json"),
        Runner::Claude => claude_settings_path(ws, name),
        Runner::Antigravity => antigravity_settings_path(ws, name),
    };
    match read_json(&path) {
        Some(doc) => (
            doc.get("template").and_then(Value::as_str).map(String::from),
            doc.get("class").and_then(Value::as_str).map(String::from),
            doc.get("bridgePort").and_then(Value::as_u64).and_then(|p| u16::try_from(p).ok()),
        ),
        None => (None, None, None),
    }
}

/// Nom du modèle d'origine d'un agent (`None` s'il a été créé sans modèle).
pub fn template_of(ws: &Path, name: &str, runner: Runner) -> Option<String> {
    stamped(ws, name, runner).0
}

/// Classe affichable d'un agent (logo/archétype), posée par son modèle à la création.
///
/// `Some("discord")` pour un agent créé avec `create_agent {template:"discord"}` ;
/// `None` par défaut pour un agent classique (la classe pourra être dérivée plus tard à
/// partir du nom). API stable, consommée à la vague suivante (annonce LiteMCP, `status`,
/// `list_agents`, `/status` du pont) : tout changement de signature doit être coordonné
/// avec ces consommateurs.
pub fn class_of(ws: &Path, name: &str, runner: Runner) -> Option<String> {
    stamped(ws, name, runner).1
}

/// Port de pont conseillé pour cet agent, posé par son modèle à la création
/// ([`stamp_with_port`]). `None` : rien de posé — le lancement (vague suivante) gardera
/// le comportement actuel (pas de pont sans `--bridge` explicite).
#[allow(dead_code)] // pas encore appelé dans la lib : gardé comme contrat, la vague
// lancement en fera le défaut de `--bridge` (PLAN §4.1).
pub fn bridge_port_of(ws: &Path, name: &str, runner: Runner) -> Option<u16> {
    stamped(ws, name, runner).2
}

#[cfg(test)]
mod tests {
    use super::{
        BUILT_INS, Source, Template, bridge_port_of, class_of, list, load, missing_vars, stamp,
        stamp_with_port, template_of,
    };
    use crate::agents::{Runner, antigravity_settings_path, claude_settings_path};
    use crate::store::{read_json, write_json_atomic};
    use serde_json::json;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU32, Ordering};

    static SEQ: AtomicU32 = AtomicU32::new(0);

    /// Workspace temporaire unique, supprimé à la fin du test.
    struct TempWs(PathBuf);

    impl TempWs {
        fn new(tag: &str) -> Self {
            let n = SEQ.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir()
                .join(format!("swarm-templates-{tag}-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            TempWs(dir)
        }
    }

    impl Drop for TempWs {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Les 11 slugs de classe alignés sur les logos de MCP Control (PLAN §4.1).
    const CLASSES: [&str; 11] = [
        "discord",
        "whatsapp",
        "x",
        "google",
        "custom",
        "coder",
        "browser",
        "explorer",
        "coordinator",
        "monitor",
        "verifier",
    ];

    /// Une attente d'un modèle intégré : serveurs MCP attendus, variables requises,
    /// défaut `executable`, port de pont conseillé.
    type Attendu =
        (&'static str, &'static [&'static str], &'static [&'static str], bool, Option<u16>);

    /// Schéma attendu par modèle intégré (`unit` toujours false, runner toujours claude).
    const ATTENDUS: [Attendu; 11] = [
        ("browser", &["serveur_navigateur"], &[], false, None),
        ("coder", &[], &[], false, None),
        ("coordinator", &[], &[], false, None),
        ("custom", &[], &[], true, Some(3013)),
        ("discord", &["serveur_discord"], &["DISCORD_BOT_TOKEN"], true, Some(3011)),
        ("explorer", &[], &[], false, None),
        ("google", &["serveur_google"], &["GOOGLE_CLIENT_ID", "GOOGLE_CLIENT_SECRET"], true, None),
        ("monitor", &[], &[], false, None),
        ("verifier", &[], &[], false, None),
        ("whatsapp", &["serveur_whatsapp"], &[], true, Some(3012)),
        ("x", &["serveur_x"], &["X_API_TOKEN"], true, None),
    ];

    #[test]
    fn les_modeles_integres_se_chargent_et_valident_leur_schema() {
        let ws = TempWs::new("integres");
        // Les 11 intégrés sont listés, tous en builtin.
        let listed = list(&ws.0);
        let names: Vec<&str> = listed.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names.len(), 11, "{names:?}");
        assert!(listed.iter().all(|t| t.source == Source::BuiltIn), "{names:?}");
        // Chaque modèle intégré se charge et valide son schéma complet.
        for (name, servers, vars, executable, port) in ATTENDUS.iter() {
            let loaded = load(&ws.0, name).expect("chargement");
            assert_eq!(loaded.source, Source::BuiltIn, "{name}");
            assert!(!loaded.prompt.trim().is_empty(), "{name} : persona vide");
            assert!(!loaded.template.description.is_empty(), "{name} : description vide");
            // promptFile déclaré = le Markdown embarqué de même nom.
            assert_eq!(
                loaded.template.prompt_file.as_deref().unwrap_or_default(),
                format!("{name}.md"),
                "{name} : promptFile"
            );
            // Classe = nom du modèle, dans la liste officielle des slugs, avec son logo.
            let class = loaded.template.class.as_deref().expect("classe");
            assert_eq!(class, *name, "{name} : classe");
            assert!(CLASSES.contains(&class), "{name} : classe inconnue « {class} »");
            let logo = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("assets/classes")
                .join(format!("hornet-pixel-{class}.svg"));
            assert!(logo.is_file(), "{name} : logo absent ({})", logo.display());
            // Serveurs MCP attendus, variables requises, défauts executable/unit/port.
            assert_eq!(loaded.template.expected_mcp_servers, *servers, "{name} : serveurs MCP");
            assert_eq!(loaded.template.required_vars, *vars, "{name} : variables requises");
            assert_eq!(loaded.runner, Some(Runner::Claude), "{name} : runner par défaut");
            assert_eq!(loaded.template.defaults.executable, *executable, "{name} : executable");
            assert!(!loaded.template.defaults.unit, "{name} : unit");
            assert_eq!(loaded.template.defaults.bridge_port, *port, "{name} : port de pont");
        }
        // Chaque modèle intégré déclare un promptFile qui pointe vers son propre .md.
        for (name, doc, md) in BUILT_INS {
            let t: Template = serde_json::from_str(doc).expect("JSON intégré valide");
            assert_eq!(t.name, *name);
            assert_eq!(t.prompt_file.as_deref().unwrap_or(name), format!("{name}.md"));
            assert!(!md.is_empty());
        }
        assert_eq!(BUILT_INS.len(), 11);
    }

    #[test]
    fn le_port_de_pont_du_modele_se_marque_dans_la_definition() {
        let ws = TempWs::new("port");
        let path = claude_settings_path(&ws.0, "bob");
        write_json_atomic(&path, &json!({ "env": {} })).unwrap();
        // Avec un port : posé, relu, l'env existant conservé.
        stamp_with_port(&ws.0, "bob", Runner::Claude, "discord", Some("discord"), Some(3011))
            .unwrap();
        assert_eq!(bridge_port_of(&ws.0, "bob", Runner::Claude), Some(3011));
        assert_eq!(class_of(&ws.0, "bob", Runner::Claude).as_deref(), Some("discord"));
        assert!(read_json(&path).unwrap().get("env").is_some(), "env conservé");
        // Sans port : un port déjà posé n'est pas effacé (même sémantique que la classe).
        stamp_with_port(&ws.0, "bob", Runner::Claude, "discord", Some("discord"), None).unwrap();
        assert_eq!(bridge_port_of(&ws.0, "bob", Runner::Claude), Some(3011));
        // Antigravity : même écriture dans settings_<nom>.json du profil .antigravity.
        let agp = antigravity_settings_path(&ws.0, "bob");
        write_json_atomic(&agp, &json!({})).unwrap();
        stamp_with_port(&ws.0, "bob", Runner::Antigravity, "custom", Some("custom"), Some(3013))
            .unwrap();
        assert_eq!(bridge_port_of(&ws.0, "bob", Runner::Antigravity), Some(3013));
    }

    #[test]
    fn le_workspace_surcharge_le_modele_integre() {
        let ws = TempWs::new("surcharge");
        let dir = ws.0.join("templates");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("coder.json"),
            r#"{"name":"coder","description":"surcharge locale","promptFile":"coder.md","requiredVars":["SWARM_TEST_SURELY_ABSENT"]}"#,
        )
        .unwrap();
        std::fs::write(dir.join("coder.md"), "persona locale $SWARM_TEST_SURELY_ABSENT-").unwrap();
        let loaded = load(&ws.0, "coder").unwrap();
        assert_eq!(loaded.source, Source::Workspace);
        assert_eq!(loaded.template.description, "surcharge locale");
        // La persona vient du workspace ; l'absente s'interpole en chaîne vide.
        assert_eq!(loaded.prompt, "persona locale -");
        assert_eq!(missing_vars(&loaded.template), ["SWARM_TEST_SURELY_ABSENT"]);
        // La liste marque la surcharge, sans dupliquer l'intégré.
        let listed = list(&ws.0);
        assert_eq!(listed.iter().filter(|t| t.name == "coder").count(), 1);
        let coder = listed.iter().find(|t| t.name == "coder").unwrap();
        assert_eq!(coder.source, Source::Workspace);
    }

    #[test]
    fn une_surchage_sans_persona_reprend_le_markdown_integre() {
        let ws = TempWs::new("sans_persona");
        let dir = ws.0.join("templates");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("coder.json"),
            r#"{"name":"coder","description":"juste la fiche"}"#,
        )
        .unwrap();
        let loaded = load(&ws.0, "coder").unwrap();
        assert!(loaded.prompt.contains("codeur"), "persona intégrée attendue : {}", loaded.prompt);
    }

    #[test]
    fn un_nom_inconnu_liste_les_integres() {
        let ws = TempWs::new("inconnu");
        let err = match load(&ws.0, "nope") {
            Err(e) => e,
            Ok(_) => panic!("un nom inconnu doit échouer"),
        };
        assert!(err.contains("coder") && err.contains("discord"), "{err}");
    }

    #[test]
    fn un_name_incoherent_ou_un_runner_inconnu_sont_refuses() {
        let ws = TempWs::new("incoherent");
        let dir = ws.0.join("templates");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("x.json"), r#"{"name":"autre","description":"d"}"#).unwrap();
        assert!(load(&ws.0, "x").is_err(), "name doit valoir le nom du fichier");
        std::fs::write(
            dir.join("y.json"),
            r#"{"name":"y","description":"d","defaults":{"runner":"gemini"}}"#,
        )
        .unwrap();
        assert!(load(&ws.0, "y").is_err(), "runner par défaut inconnu refusé");
    }

    #[test]
    fn la_marque_du_modele_se_lit_dans_la_definition_claude() {
        let ws = TempWs::new("marque");
        let path = claude_settings_path(&ws.0, "alice");
        write_json_atomic(&path, &json!({ "env": {}, "runner": "claude" })).unwrap();
        // Sans modèle : aucune classe (défaut), sur ce runner comme sur les autres.
        assert_eq!(class_of(&ws.0, "alice", Runner::Claude), None);
        assert_eq!(template_of(&ws.0, "alice", Runner::Claude), None);
        assert_eq!(class_of(&ws.0, "alice", Runner::Antigravity), None);
        // Marque puis relecture : l'env existant est conservé.
        stamp(&ws.0, "alice", Runner::Claude, "discord", Some("discord")).unwrap();
        assert_eq!(class_of(&ws.0, "alice", Runner::Claude).as_deref(), Some("discord"));
        assert_eq!(template_of(&ws.0, "alice", Runner::Claude).as_deref(), Some("discord"));
        assert!(read_json(&path).unwrap().get("env").is_some(), "env conservé");
        // Marquer sans classe ne touche pas à celle déjà posée.
        stamp(&ws.0, "alice", Runner::Claude, "coder", None).unwrap();
        assert_eq!(template_of(&ws.0, "alice", Runner::Claude).as_deref(), Some("coder"));
        assert_eq!(class_of(&ws.0, "alice", Runner::Claude).as_deref(), Some("discord"));
        // Marquer une définition absente échoue (create écrit toujours les settings avant).
        assert!(stamp(&ws.0, "ghost", Runner::Claude, "coder", None).is_err());
    }
}
