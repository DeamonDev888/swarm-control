//! Sondes des serveurs MCP du registre LiteMCP : qui est annoncé, qui répond.
//!
//! Lecture seule : `status` sur les agents résidents (outil annoté `read_only` côté serveur).
//! Sondes en parallèle borné : au plus [`PROBE_MAX`] fils, les serveurs en surnombre
//! attendent dans une file ; en std seulement (`mpsc`), pas de runtime async. Attente
//! bornée : elles tournent dans le fil des sondes (`mod.rs`), jamais dans le fil
//! d'interface.

use crate::peers::{self, CallError, Peer, call_tool};
use crate::tui::state::PeerRow;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

/// Sondes MCP en même temps, au plus. Les serveurs au-delà attendent qu'un fil se libère.
const PROBE_MAX: usize = 8;

/// Sonde tous les serveurs `swarm-*` annoncés : au plus [`PROBE_MAX`] sondes à la fois,
/// les serveurs en surnombre attendent dans la file. La ligne affichable ([`PeerRow`]) est
/// définie dans `state` : la découverte en fournit les champs d'identité (nom, adresse,
/// vie, soi), la sonde d'ici ajoute l'état et le détail.
pub fn probe_all(self_agent: &str, wait: Duration) -> Vec<PeerRow> {
    probe_pool(peers::discover(), self_agent, wait)
}

/// Sonde `peers` avec au plus [`PROBE_MAX`] fils (voir [`pool_with`]).
fn probe_pool(peers: Vec<Peer>, self_agent: &str, wait: Duration) -> Vec<PeerRow> {
    pool_with(peers, self_agent, move |peer, own| {
        probe_peer(&peer.name, &peer.url, peer.pid, peer.alive, own, wait)
    })
}

/// La file des travaux est partagée, un fil puise dedans jusqu'à l'épuiser puis meurt ;
/// `probe(&pair, own)` tourne une fois par pair et fournit sa ligne par canal. La sonde est
/// injectée pour que les tests observent la concurrence sans réseau.
/// Aucun fil disponible (repli) : toutes les sondes dans ce fil-ci, l'attente reste
/// bornée chacune.
fn pool_with(
    peers: Vec<Peer>,
    self_agent: &str,
    probe: impl Fn(&Peer, bool) -> PeerRow + Send + Sync + 'static,
) -> Vec<PeerRow> {
    let workers = peers.len().min(PROBE_MAX);
    let (jobs_tx, jobs_rx) = mpsc::channel::<Peer>();
    for peer in peers {
        let _ = jobs_tx.send(peer);
    }
    drop(jobs_tx);
    let jobs_rx = Arc::new(Mutex::new(jobs_rx));
    let probe = Arc::new(probe);
    let (rows_tx, rows_rx) = mpsc::channel();
    let self_agent = self_agent.to_string();
    let mut started = 0;
    for i in 0..workers {
        let jobs_rx = Arc::clone(&jobs_rx);
        let rows_tx = rows_tx.clone();
        let probe = Arc::clone(&probe);
        let agent = self_agent.clone();
        let spawned =
            std::thread::Builder::new().name(format!("swarm-tui-probe-{i}")).spawn(move || {
                loop {
                    // Retrait sous verrou, sonde hors verrou : le garde est relâché à la fin
                    // du `match`. Un `while let` le garderait pendant toute la sonde (réseau,
                    // attente) et les fils attendraient l'un l'autre — tout redeviendrait
                    // séquentiel.
                    let peer = match jobs_rx.lock().unwrap_or_else(|e| e.into_inner()).recv() {
                        Ok(peer) => peer,
                        Err(_) => break, // file vide : tous les travaux sont distribués
                    };
                    let own = peer.agent == agent;
                    let _ = rows_tx.send(probe(&peer, own));
                }
            });
        if spawned.is_err() {
            break;
        }
        started += 1;
    }
    if started == 0 {
        let jobs = jobs_rx.lock().unwrap_or_else(|e| e.into_inner());
        while let Ok(peer) = jobs.recv() {
            let own = peer.agent == self_agent;
            let _ = rows_tx.send(probe(&peer, own));
        }
    }
    drop(rows_tx);
    let mut out: Vec<PeerRow> = rows_rx.iter().collect();
    sort_peers(&mut out);
    out
}

/// Ordre d'affichage : vivants d'abord, puis par nom.
pub fn sort_peers(peers: &mut [PeerRow]) {
    peers.sort_by(|a, b| {
        let by_life = b.alive.cmp(&a.alive);
        by_life.then_with(|| a.name.cmp(&b.name))
    });
}

/// Sonde un serveur annoncé : vivant au sens du PID, puis réponse MCP réelle.
pub fn probe_peer(
    name: &str,
    url: &str,
    pid: u32,
    alive: bool,
    own: bool,
    wait: Duration,
) -> PeerRow {
    let mut peer = PeerRow {
        name: name.to_string(),
        addr: addr_of(url),
        alive,
        own,
        state: String::new(),
        detail: String::new(),
    };
    if !alive {
        peer.state = "inactif".into();
        peer.detail = format!("PID {pid} mort");
        return peer;
    }
    match call_tool(url, "status", json!({}), wait) {
        Ok(status) => {
            peer.state = "ok".into();
            peer.detail = detail_from_status(&status);
        }
        Err(error) => {
            peer.state = describe(&error);
            peer.detail = short_reason(&error, 60);
        }
    }
    peer
}

/// `http://hôte:port/mcp` → `hôte:port` (l'URL brute si elle n'a pas la forme attendue).
pub fn addr_of(url: &str) -> String {
    url.trim_start_matches("http://").trim_end_matches("/mcp").to_string()
}

/// Ce que la réponse à `status` vaut pour le panneau : `busy · file 2`.
pub fn detail_from_status(status: &Value) -> String {
    let state = status["state"].as_str().unwrap_or("?");
    let file = status["queueLength"].as_u64().unwrap_or(0);
    format!("{state} · file {file}")
}

/// En un mot, la couleur du panneau.
fn describe(error: &CallError) -> String {
    match error {
        CallError::Unreachable(_) => "injoignable".into(),
        CallError::Tool(_) => "erreur".into(),
        CallError::Protocol(_) => "protocole".into(),
    }
}

/// La raison détaillée d'une sonde ratée, bornée par caractères.
fn short_reason(error: &CallError, max: usize) -> String {
    let reason = match error {
        CallError::Unreachable(m) | CallError::Tool(m) | CallError::Protocol(m) => m.as_str(),
    };
    let reason = reason.split('\n').next().unwrap_or_default();
    let mut out: String = reason.chars().take(max).collect();
    if reason.chars().count() > max {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::time::Instant;

    #[test]
    fn un_pid_mort_est_inactif_sans_reseau_ni_attente() {
        // Délai volontairement long : un pair mort est jugé sur-le-champ, sans sonde réseau.
        let debut = Instant::now();
        let peer = probe_peer(
            "swarm-alice",
            "http://127.0.0.1:23082/mcp",
            4242,
            false,
            false,
            Duration::from_secs(10),
        );
        assert!(
            debut.elapsed() < Duration::from_secs(5),
            "la sonde a attendu : {:?}",
            debut.elapsed()
        );
        assert_eq!(peer.state, "inactif");
        assert_eq!(peer.detail, "PID 4242 mort");
        assert_eq!(peer.addr, "127.0.0.1:23082");
    }

    #[test]
    fn un_port_ferme_est_injoignable() {
        // Le PID 1 est vivant (soi-même) mais le port 9 n'écoute pas : sonde rapide et sûre.
        let peer = probe_peer(
            "swarm-alice",
            "http://127.0.0.1:9/mcp",
            std::process::id(),
            true,
            false,
            Duration::from_millis(500),
        );
        assert_eq!(peer.state, "injoignable", "état : {} ({})", peer.state, peer.detail);
    }

    #[test]
    fn les_vivants_passent_avant_les_morts_puis_par_nom() {
        let mut peers = vec![
            PeerRow {
                name: "swarm-zoe".into(),
                addr: String::new(),
                alive: false,
                own: false,
                state: "inactif".into(),
                detail: String::new(),
            },
            PeerRow {
                name: "swarm-alice".into(),
                addr: String::new(),
                alive: true,
                own: false,
                state: "ok".into(),
                detail: String::new(),
            },
            PeerRow {
                name: "swarm-bob".into(),
                addr: String::new(),
                alive: true,
                own: false,
                state: "ok".into(),
                detail: String::new(),
            },
        ];
        sort_peers(&mut peers);
        let names: Vec<&str> = peers.iter().map(|p| p.name.as_str()).collect();
        // Vivants d'abord (alice, bob), puis les morts (zoe), par nom dans chaque groupe.
        assert_eq!(names, ["swarm-alice", "swarm-bob", "swarm-zoe"]);
        assert!(peers[0].alive && peers[1].alive && !peers[2].alive);
    }

    #[test]
    fn le_statut_devient_une_ligne_courte() {
        assert_eq!(
            detail_from_status(&json!({"state": "busy", "queueLength": 2})),
            "busy · file 2"
        );
        assert_eq!(detail_from_status(&json!({})), "? · file 0");
    }

    #[test]
    fn ladresse_sort_de_lurl() {
        assert_eq!(addr_of("http://127.0.0.1:23082/mcp"), "127.0.0.1:23082");
        assert_eq!(addr_of("http://127.0.0.1:23082"), "127.0.0.1:23082");
    }

    #[test]
    fn la_file_draine_tous_les_pairs_meme_plus_que_le_plafond() {
        // 20 serveurs (2,5× le plafond de 8 fils) avec des PID morts : sondes sans réseau,
        // la file doit tout drainer et le tri doit rester par nom.
        let peers: Vec<Peer> = (0..20)
            .map(|n| Peer {
                name: format!("swarm-p{n}"),
                agent: format!("p{n}"),
                url: format!("http://127.0.0.1:{}/mcp", 20000 + n),
                pid: 3_000_000 + n * 4,
                alive: false,
            })
            .collect();
        let rows = probe_pool(peers, "moi", Duration::from_secs(1));
        assert_eq!(rows.len(), 20, "tous les pairs doivent revenir de la file");
        assert!(rows.iter().all(|r| r.state == "inactif"), "état : {}", rows[0].state);
        assert!(rows.windows(2).all(|w| w[0].name <= w[1].name), "tri par nom attendu");
    }

    #[test]
    fn au_plus_huit_sondes_a_la_fois_et_reellement_en_parallele() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        // 20 sondes de 40 ms chacune, sans réseau : 8 fils puisent la file, donc au plus
        // 8 à la fois, et jamais l'une après l'autre (20 × 40 ms = 800 ms en séquentiel).
        let peers: Vec<Peer> = (0..20)
            .map(|n| Peer {
                name: format!("swarm-p{n:02}"),
                agent: format!("p{n}"),
                url: String::new(),
                pid: 3_000_000 + n * 4,
                alive: true,
            })
            .collect();
        let en_cours = Arc::new(AtomicUsize::new(0));
        let maximum = Arc::new(AtomicUsize::new(0));
        let (compteur, pic) = (Arc::clone(&en_cours), Arc::clone(&maximum));
        let debut = Instant::now();
        let rows = pool_with(peers, "moi", move |peer, _| {
            pic.fetch_max(compteur.fetch_add(1, Ordering::SeqCst) + 1, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(40));
            compteur.fetch_sub(1, Ordering::SeqCst);
            PeerRow {
                name: peer.name.clone(),
                addr: String::new(),
                alive: true,
                own: false,
                state: "ok".into(),
                detail: String::new(),
            }
        });
        let duree = debut.elapsed();
        let vu = maximum.load(Ordering::SeqCst);
        assert_eq!(rows.len(), 20, "un résultat par serveur");
        assert!(vu <= PROBE_MAX, "{vu} sondes à la fois, plafond {PROBE_MAX}");
        assert!(vu >= 2, "les sondes ont tourné l'une après l'autre (maximum {vu})");
        assert!(
            duree < Duration::from_millis(20 * 40),
            "durée {duree:?} : pas plus rapide que le séquentiel"
        );
        assert!(rows.windows(2).all(|w| w[0].name <= w[1].name), "ordre d'affichage attendu");
    }

    #[test]
    fn un_pair_muet_rend_la_main_au_bout_du_delai() {
        // Connexion acceptée, jamais de réponse : la sonde est bornée par `wait`, jamais
        // bloquante pour le fil des sondes.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (fin_tx, fin_rx) = mpsc::channel::<()>();
        let muet = std::thread::spawn(move || {
            if let Ok((flux, _)) = listener.accept() {
                // Tenu ouvert sans répondre, jusqu'à la fin du test.
                let _ = fin_rx.recv();
                drop(flux);
            }
        });
        let attente = Duration::from_millis(300);
        let debut = Instant::now();
        let peer = probe_peer(
            "swarm-muet",
            &format!("http://127.0.0.1:{port}/mcp"),
            std::process::id(),
            true,
            false,
            attente,
        );
        let duree = debut.elapsed();
        let _ = fin_tx.send(());
        let _ = muet.join();
        assert!(duree >= attente, "rendu avant le délai : {duree:?}");
        assert!(duree < attente + Duration::from_secs(5), "sonde non bornée : {duree:?}");
        assert_eq!(peer.state, "protocole", "réponse coupée : {} ({})", peer.state, peer.detail);
    }

    #[test]
    fn lordre_affiche_ne_depend_pas_de_lordre_dachevement() {
        // Entrée dans un ordre quelconque, achèvement dans un autre (la plus lente finit
        // dernière) : le rendu reste vivants d'abord puis par nom, une ligne par pair.
        let pairs: Vec<Peer> = [("swarm-c", true), ("swarm-a", false), ("swarm-b", true)]
            .into_iter()
            .map(|(nom, vivant)| Peer {
                name: nom.into(),
                agent: nom.trim_start_matches("swarm-").to_string(),
                url: String::new(),
                pid: 3_000_000_105,
                alive: vivant,
            })
            .collect();
        let rows = pool_with(pairs, "moi", |peer, own| {
            let delai_ms = match peer.name.as_str() {
                "swarm-a" => 90,
                "swarm-b" => 45,
                _ => 0,
            };
            std::thread::sleep(Duration::from_millis(delai_ms));
            PeerRow {
                name: peer.name.clone(),
                addr: String::new(),
                alive: peer.alive,
                own,
                state: if peer.alive { "ok".into() } else { "inactif".into() },
                detail: String::new(),
            }
        });
        let noms: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(noms, ["swarm-b", "swarm-c", "swarm-a"], "vivants (b, c) puis le mort (a)");
        assert!(rows.iter().all(|r| !r.own), "aucun de ces pairs n'est « moi »");
    }

    #[test]
    fn sans_pair_aucune_sonde_et_un_seul_pair_une_seule_ligne() {
        let vide = pool_with(Vec::new(), "moi", |_, _| -> PeerRow { panic!("sonde sans pair") });
        assert!(vide.is_empty());
        let seul = vec![Peer {
            name: "swarm-moi".into(),
            agent: "moi".into(),
            url: String::new(),
            pid: 3_000_000_105,
            alive: true,
        }];
        let rows = pool_with(seul, "moi", |peer, own| PeerRow {
            name: peer.name.clone(),
            addr: String::new(),
            alive: peer.alive,
            own,
            state: "ok".into(),
            detail: String::new(),
        });
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "swarm-moi");
        assert!(rows[0].own, "le pair de l'agent courant est marqué");
    }
}
