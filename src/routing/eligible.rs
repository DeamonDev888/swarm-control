//! Un run est-il routable vers un hôte ?

use crate::owners::{self, Holder};
use crate::runners::RunRequest;

/// L'hôte vivant de l'agent demandé, si le run peut lui être confié tel quel.
///
/// `ask` reprend la session de l'agent, dans le workspace de l'hôte : un run qui impose sa
/// propre session, un répertoire (`path`), un fournisseur ou refuse la reprise reste local.
pub fn host_of(req: &RunRequest) -> Option<Holder> {
    let agent = req.agent.as_deref()?;
    let plain =
        req.session_id.is_none() && req.auto_resume && req.cwd.is_none() && req.provider.is_none();
    if !plain {
        return None;
    }
    owners::host_of(&req.workspace, req.runner.as_str(), agent).filter(|h| h.url.is_some())
}

#[cfg(test)]
mod tests {
    use super::host_of;
    use crate::agents::Runner;
    use crate::owners::{self, Holder, Lease};
    use crate::runners::RunRequest;
    use crate::store::now_ms;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU32, Ordering};

    static SEQ: AtomicU32 = AtomicU32::new(0);

    /// Workspace temporaire unique, supprimé à la fin du test.
    struct Ws(PathBuf);

    impl Ws {
        fn nouveau() -> Self {
            let n = SEQ.fetch_add(1, Ordering::Relaxed);
            let dir =
                std::env::temp_dir().join(format!("swarm-routable-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Ws(dir)
        }
    }

    impl Drop for Ws {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Demande de `claude` pour `agent`, telle que `run_agent` la construit sans option.
    fn requete(ws: &Path, agent: Option<&str>) -> RunRequest {
        RunRequest {
            runner: Runner::Claude,
            prompt: "bonjour".into(),
            agent: agent.map(String::from),
            session_id: None,
            auto_resume: true,
            model: None,
            provider: None,
            cwd: None,
            workspace: ws.to_path_buf(),
        }
    }

    /// Héberge `agent` (`runner`) dans `ws` avec l'URL de son serveur ; bail gardé par l'appelant.
    fn heberger(ws: &Path, runner: &str, agent: &str, url: Option<&str>) -> Lease {
        let bail = owners::acquire_host(ws, runner, agent, "host-1").unwrap();
        if let Some(url) = url {
            bail.edit(|h| h.url = Some(url.to_string()));
        }
        bail
    }

    #[test]
    fn un_agent_heberge_ailleurs_avec_son_url_est_routable_vers_son_hote() {
        let ws = Ws::nouveau();
        let _bail = heberger(&ws.0, "claude", "alice", Some("http://127.0.0.1:23082/mcp"));
        let hote = host_of(&requete(&ws.0, Some("alice"))).expect("routable");
        assert_eq!(hote.url.as_deref(), Some("http://127.0.0.1:23082/mcp"));
        assert_eq!(
            (hote.runner.as_deref(), hote.agent.as_deref()),
            (Some("claude"), Some("alice"))
        );
        assert_eq!(hote.run_id, "host-1", "le jeton d'hébergement identifie l'hôte");
    }

    #[test]
    fn sans_hote_vivant_ou_sans_url_rien_n_est_routable() {
        let ws = Ws::nouveau();
        assert!(host_of(&requete(&ws.0, Some("alice"))).is_none(), "pas de résident");
        // Hébergé, serveur pas encore lié (pas d'URL) : pas routable.
        let _bail = heberger(&ws.0, "claude", "alice", None);
        assert!(host_of(&requete(&ws.0, Some("alice"))).is_none(), "sans URL");
        // L'hôte de `claude/alice` ne sert ni `hermes/alice` ni `claude/bob`.
        let hermes = RunRequest { runner: Runner::Hermes, ..requete(&ws.0, Some("alice")) };
        assert!(host_of(&hermes).is_none(), "autre runner");
        assert!(host_of(&requete(&ws.0, Some("bob"))).is_none(), "autre agent");
        // Sans nom : un run éphémère n'a pas d'hôte, même si un résident existe.
        assert!(host_of(&requete(&ws.0, None)).is_none(), "anonyme");
    }

    #[test]
    fn un_run_qui_impose_sa_session_son_repertoire_ou_son_fournisseur_reste_local() {
        let ws = Ws::nouveau();
        let _bail = heberger(&ws.0, "claude", "alice", Some("http://127.0.0.1:23082/mcp"));
        assert!(host_of(&requete(&ws.0, Some("alice"))).is_some(), "prémisses : routable tel quel");
        let mut r = requete(&ws.0, Some("alice"));
        r.session_id = Some("s-1".into());
        assert!(host_of(&r).is_none(), "sessionId imposé : local");
        let mut r = requete(&ws.0, Some("alice"));
        r.auto_resume = false;
        assert!(host_of(&r).is_none(), "autoResume:false : local");
        let mut r = requete(&ws.0, Some("alice"));
        r.cwd = Some("/ailleurs".into());
        assert!(host_of(&r).is_none(), "path imposé : local");
        let mut r = requete(&ws.0, Some("alice"));
        r.provider = Some("anthropic".into());
        assert!(host_of(&r).is_none(), "provider imposé : local");
        // Le modèle, lui, ne force rien : il est transmis à l'hôte.
        let mut r = requete(&ws.0, Some("alice"));
        r.model = Some("opus".into());
        assert!(host_of(&r).is_some(), "model : routable");
    }

    #[test]
    fn un_hote_mort_n_est_pas_un_hote() {
        let ws = Ws::nouveau();
        // Bail d'hébergement d'un process mort, avec une URL : pas routable.
        std::fs::create_dir_all(ws.0.join("bridge").join("hosts")).unwrap();
        let mort = Holder {
            instance: "mort".into(),
            pid: u32::MAX,
            run_id: "host-mort".into(),
            ts: now_ms(),
            runner: Some("claude".into()),
            agent: Some("alice".into()),
            url: Some("http://127.0.0.1:1/mcp".into()),
        };
        let fichier = ws.0.join("bridge").join("hosts").join("claude-alice.json");
        std::fs::write(fichier, serde_json::to_vec(&mort).unwrap()).unwrap();
        assert!(host_of(&requete(&ws.0, Some("alice"))).is_none(), "hôte mort");
    }
}
