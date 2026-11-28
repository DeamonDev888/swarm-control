//! Transmission d'un run à son hôte, par `ask`.

use super::eligible;
use crate::owners::Holder;
use crate::peers::{CallError, call_tool};
use crate::runners::{RunRequest, RunResult};
use serde_json::{Value, json};
use std::time::Duration;

/// Au-delà de l'attente par défaut de `ask` (1 h).
const WAIT: Duration = Duration::from_secs(3700);

pub struct Forwarded {
    pub result: RunResult,
    pub host: Holder,
}

/// Confie le run à son hôte. `None` = pas routable, ou hôte injoignable : lancer à la demande.
pub fn forward(req: &RunRequest) -> Option<Forwarded> {
    let host = eligible::host_of(req)?;
    let mut args = json!({ "prompt": req.prompt, "from": host_name() });
    if let Some(model) = &req.model {
        args["model"] = json!(model);
    }
    let result = match call_tool(host.url.as_deref()?, "ask", args, WAIT) {
        Ok(v) => reply(&v),
        Err(CallError::Unreachable(why)) => {
            eprintln!("hôte {} injoignable ({why}) : run à la demande", host.instance);
            return None;
        }
        Err(CallError::Tool(m) | CallError::Protocol(m)) => RunResult::err(m),
    };
    Some(Forwarded { result, host })
}

fn host_name() -> String {
    crate::owners::instance_name()
}

fn reply(v: &Value) -> RunResult {
    RunResult {
        result: v["reply"].as_str().unwrap_or_default().to_string(),
        session_id: v["sessionId"].as_str().map(String::from),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::{forward, reply};
    use crate::agents::Runner;
    use crate::owners::{self, Lease};
    use crate::peers::{CallError, call_tool};
    use crate::runners::RunRequest;
    use serde_json::{Value, json};
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    static SEQ: AtomicU32 = AtomicU32::new(0);

    /// Workspace temporaire unique, supprimé à la fin du test.
    struct Ws(PathBuf);

    impl Ws {
        fn nouveau() -> Self {
            let n = SEQ.fetch_add(1, Ordering::Relaxed);
            let dir =
                std::env::temp_dir().join(format!("swarm-transmis-{}-{n}", std::process::id()));
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

    /// Hôte fictif : serveur HTTP minimal sur `127.0.0.1:0` qui répond `reponse` à chaque
    /// requête et garde ce qu'il a reçu — `(en-têtes, corps JSON)`.
    struct Hote {
        addr: SocketAddr,
        url: String,
        requetes: Arc<Mutex<Vec<(String, Value)>>>,
        arret: Arc<AtomicBool>,
    }

    impl Hote {
        fn nouveau(reponse: &'static str) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("écoute éphémère");
            let addr = listener.local_addr().expect("adresse");
            let requetes = Arc::new(Mutex::new(Vec::new()));
            let arret = Arc::new(AtomicBool::new(false));
            let (vues, stop) = (Arc::clone(&requetes), Arc::clone(&arret));
            std::thread::spawn(move || {
                for flux in listener.incoming() {
                    let Ok(mut flux) = flux else { break };
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    if let Some((tete, corps)) = lire(&mut flux) {
                        vues.lock().unwrap_or_else(|e| e.into_inner()).push((tete, corps));
                        let reponse = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reponse}",
                            reponse.len()
                        );
                        let _ = flux.write_all(reponse.as_bytes());
                    }
                }
            });
            Hote { addr, url: format!("http://{addr}/mcp"), requetes, arret }
        }

        fn recues(&self) -> Vec<(String, Value)> {
            self.requetes.lock().unwrap_or_else(|e| e.into_inner()).clone()
        }
    }

    impl Drop for Hote {
        fn drop(&mut self) {
            self.arret.store(true, Ordering::SeqCst);
            // Réveille l'`accept` du fil : la connexion est refermée aussitôt.
            let _ = TcpStream::connect(self.addr);
        }
    }

    /// Lit une requête HTTP entière (en-têtes, puis `Content-Length` octets de corps).
    fn lire(flux: &mut TcpStream) -> Option<(String, Value)> {
        flux.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
        let mut brut = Vec::new();
        let mut tampon = [0u8; 4096];
        loop {
            let n = flux.read(&mut tampon).ok()?;
            if n == 0 {
                return None;
            }
            brut.extend_from_slice(&tampon[..n]);
            let texte = String::from_utf8_lossy(&brut);
            let Some((tete, corps)) = texte.split_once("\r\n\r\n") else { continue };
            let taille: usize = tete.lines().find_map(|l| {
                l.to_ascii_lowercase().strip_prefix("content-length:")?.trim().parse().ok()
            })?;
            if corps.len() >= taille {
                return Some((tete.to_string(), serde_json::from_str(&corps[..taille]).ok()?));
            }
        }
    }

    /// Demande de `claude` pour `alice` (résidente), sans option autre que le modèle.
    fn requete(ws: &Path, model: Option<&str>) -> RunRequest {
        RunRequest {
            runner: Runner::Claude,
            prompt: "bonjour".into(),
            agent: Some("alice".into()),
            session_id: None,
            auto_resume: true,
            model: model.map(String::from),
            provider: None,
            cwd: None,
            workspace: ws.to_path_buf(),
        }
    }

    /// Héberge `alice` dans `ws` avec l'URL `url` ; bail gardé par l'appelant.
    fn heberger(ws: &Path, url: &str) -> Lease {
        let bail = owners::acquire_host(ws, "claude", "alice", "host-1").unwrap();
        bail.edit(|h| h.url = Some(url.to_string()));
        bail
    }

    #[test]
    fn maps_an_ask_reply_to_a_run_result() {
        let r = reply(&json!({"reply": "salut", "sessionId": "s1"}));
        assert_eq!((r.result.as_str(), r.session_id.as_deref()), ("salut", Some("s1")));
        assert!(r.error.is_none());
    }

    #[test]
    fn un_run_routable_est_transmis_par_ask_avec_prompt_origine_et_modele() {
        let ws = Ws::nouveau();
        let hote = Hote::nouveau(
            r#"{"jsonrpc":"2.0","id":1,"result":{"structuredContent":{"reply":"echo:bonjour","sessionId":"s-hote"}}}"#,
        );
        let _bail = heberger(&ws.0, &hote.url);
        let transmis = forward(&requete(&ws.0, Some("opus"))).expect("transmis");
        assert_eq!(transmis.result.result, "echo:bonjour");
        assert_eq!(transmis.result.session_id.as_deref(), Some("s-hote"));
        assert!(transmis.result.error.is_none());
        assert_eq!(transmis.host.run_id, "host-1");
        // Ce que l'hôte a reçu : un `tools/call` de `ask`, avec les en-têtes de routage.
        let recues = hote.recues();
        assert_eq!(recues.len(), 1);
        let (tete, corps) = &recues[0];
        assert!(tete.starts_with("POST /mcp HTTP/1.1\r\n"), "{tete}");
        let minuscules = tete.to_ascii_lowercase();
        assert!(minuscules.contains("\r\nmcp-method: tools/call\r\n"), "{tete}");
        assert!(minuscules.contains("\r\nmcp-name: ask\r\n"), "{tete}");
        assert_eq!(corps["jsonrpc"], "2.0");
        assert_eq!(corps["method"], "tools/call");
        assert_eq!(corps["params"]["name"], "ask");
        assert_eq!(
            corps["params"]["arguments"],
            json!({"prompt": "bonjour", "from": owners::instance_name(), "model": "opus"})
        );
    }

    #[test]
    fn sans_modele_l_argument_n_est_pas_envoye() {
        let ws = Ws::nouveau();
        let hote = Hote::nouveau(
            r#"{"jsonrpc":"2.0","id":1,"result":{"structuredContent":{"reply":"ok"}}}"#,
        );
        let _bail = heberger(&ws.0, &hote.url);
        let transmis = forward(&requete(&ws.0, None)).expect("transmis");
        assert_eq!((transmis.result.result.as_str(), transmis.result.session_id), ("ok", None));
        let args = hote.recues()[0].1["params"]["arguments"].clone();
        assert_eq!(args.as_object().map(|o| o.len()), Some(2), "prompt et from seulement : {args}");
        assert!(args.get("model").is_none());
    }

    #[test]
    fn un_hote_injoignable_ramene_a_lancer_a_la_demande() {
        let ws = Ws::nouveau();
        // Port fermé de la boucle locale : rien n'écoute sur le port 1.
        let _bail = heberger(&ws.0, "http://127.0.0.1:1/mcp");
        assert!(forward(&requete(&ws.0, None)).is_none(), "pas de repli vers un résultat");
        // La raison qui va dans le message « hôte … injoignable » est bien remplie.
        let raison =
            match call_tool("http://127.0.0.1:1/mcp", "ask", json!({}), Duration::from_secs(2)) {
                Err(CallError::Unreachable(why)) => why,
                autre => panic!("injoignable attendu : {autre:?}"),
            };
        assert!(!raison.is_empty(), "la raison de l'échec est nommée");
    }

    #[test]
    fn une_erreur_de_l_hote_est_remontee_comme_erreur_du_run_sans_repli_local() {
        let ws = Ws::nouveau();
        let hote = Hote::nouveau(
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"AGENT_BUSY: alice a déjà un tour en cours"}}"#,
        );
        let _bail = heberger(&ws.0, &hote.url);
        let transmis = forward(&requete(&ws.0, None))
            .expect("une erreur d'outil n'est pas une absence d'hôte");
        assert_eq!(
            transmis.result.error.as_deref(),
            Some("AGENT_BUSY: alice a déjà un tour en cours")
        );
        assert_eq!(transmis.result.result, "");
        assert_eq!(hote.recues().len(), 1, "une seule demande : rien n'est rejoué ailleurs");
    }

    #[test]
    fn une_reponse_illisible_est_une_erreur_du_run() {
        let ws = Ws::nouveau();
        let hote = Hote::nouveau("pas du tout du json");
        let _bail = heberger(&ws.0, &hote.url);
        let transmis = forward(&requete(&ws.0, None)).expect("transmis malgré la réponse");
        let erreur = transmis.result.error.expect("erreur de protocole remontée");
        assert!(erreur.contains("JSON invalide"), "{erreur}");
    }

    #[test]
    fn un_run_non_routable_n_envoie_rien() {
        let ws = Ws::nouveau();
        let hote = Hote::nouveau(
            r#"{"jsonrpc":"2.0","id":1,"result":{"structuredContent":{"reply":"jamais"}}}"#,
        );
        let _bail = heberger(&ws.0, &hote.url);
        let mut r = requete(&ws.0, None);
        r.session_id = Some("s-1".into());
        assert!(forward(&r).is_none());
        assert!(hote.recues().is_empty(), "aucune requête partie vers l'hôte");
    }
}
