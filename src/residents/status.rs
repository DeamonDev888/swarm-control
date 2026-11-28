//! État d'un agent résident, au format JSON de l'outil `status` (et du pont `/status`).
//!
//! `rebuildNeeded` (PLAN §4.8) : `null` tant que l'agent n'a jamais été compilé, sinon vrai
//! quand son binaire dédié n'est pas à la version de cette instance.

use super::ctx::Ctx;
use crate::agents::{class_of, compile};
use crate::sessions;
use serde_json::{Value, json};
use std::sync::atomic::Ordering;

impl Ctx {
    pub fn status_json(&self) -> Value {
        let current = self.current.lock().unwrap_or_else(|e| e.into_inner());
        let last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        json!({
            "agent": self.agent,
            "runner": self.runner.as_str(),
            "class": class_of(&self.workspace, &self.agent, self.runner),
            "name": self.name(),
            "url": self.url(),
            "instance": self.instance,
            "rebuildNeeded": compile::rebuild_needed_value(&self.workspace, &self.agent),
            "state": if current.is_some() { "busy" } else { "idle" },
            "queueLength": self.inbox.len(),
            "current": current.as_ref().map(|c| json!({
                "from": c.from,
                "runId": c.run_id,
                "elapsedMs": c.started.elapsed().as_millis() as u64,
            })),
            "session": sessions::get(&self.workspace, self.runner.as_str(), &self.agent),
            "turnsDone": self.turns_done.load(Ordering::SeqCst),
            "lastTurn": last.as_ref().map(|l| json!({
                "ok": l.ok,
                "error": l.error,
                "durationMs": l.duration_ms,
                "at": l.at,
            })),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::Ctx;
    use crate::agents::Runner;
    use crate::agents::templates::stamp;
    use crate::store::write_json_atomic;
    use serde_json::{Value, json};
    use std::path::PathBuf;
    use std::sync::atomic::AtomicU32;

    static SEQ: AtomicU32 = AtomicU32::new(0);

    /// Un Ctx sur un workspace temporaire unique (pid + nom du test) : aucun serveur démarré.
    fn ctx(name: &str) -> Ctx {
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let ws =
            std::env::temp_dir().join(format!("swarm-status-{}-{name}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&ws);
        std::fs::create_dir_all(&ws).unwrap();
        Ctx::new(name, Runner::Claude, ws, "swarm-control-1".into())
    }

    /// Écrit la définition Claude de l'agent, comme `create` le fait avant `stamp`.
    fn definition(ctx: &Ctx) -> PathBuf {
        let path = crate::agents::claude_settings_path(&ctx.workspace, &ctx.agent);
        write_json_atomic(&path, &json!({ "env": {}, "runner": "claude" })).unwrap();
        path
    }

    /// La classe vient de la définition (modèle), jamais du nom : `sniperbot_analyst`
    /// de classe `discord` reste `discord`, et sans classe le champ reste absent.
    #[test]
    fn le_status_publie_la_classe_de_la_definition_pas_celle_du_nom() {
        let ctx = ctx("sniperbot_analyst");
        let status = ctx.status_json();
        assert_eq!(status["class"], Value::Null, "sans classe posée : null, jamais déduite du nom");
        definition(&ctx);
        stamp(&ctx.workspace, &ctx.agent, Runner::Claude, "discord", Some("discord")).unwrap();
        assert_eq!(ctx.status_json()["class"], json!("discord"));
        let _ = std::fs::remove_dir_all(&ctx.workspace);
    }

    /// `rebuildNeeded` : `null` tant que rien n'a été compilé pour l'agent, puis un booléen
    /// (`true` pour un faux binaire texte, qui ne répond pas à `--version`).
    #[test]
    fn le_status_publie_rebuild_needed_seulement_pour_un_agent_deja_compile() {
        let ctx = ctx("rebuild");
        assert_eq!(ctx.status_json()["rebuildNeeded"], Value::Null, "jamais compilé : null");
        let bin = crate::agents::executable::path(&ctx.workspace, &ctx.agent);
        std::fs::create_dir_all(bin.parent().unwrap()).unwrap();
        std::fs::write(&bin, "faux binaire").unwrap();
        assert_eq!(ctx.status_json()["rebuildNeeded"], json!(true));
        let _ = std::fs::remove_dir_all(&ctx.workspace);
    }
}
