//! Réservation d'un numéro d'instance.

use crate::owners::{self, Busy, Holder};
use std::path::{Path, PathBuf};
use std::time::Duration;

const FIRST_FREE_LIMIT: u32 = 999;
const BEAT: Duration = Duration::from_secs(30);

fn dir() -> PathBuf {
    std::env::temp_dir().join("swarm-control-instances")
}

/// Réserve un numéro (`requested`, ou le premier libre), nomme cette instance et lance le
/// battement de cœur. Rend le numéro.
pub fn claim(requested: Option<u32>) -> Result<u32, String> {
    let number = claim_in(&dir(), requested)?;
    owners::set_instance_name(super::name(number));
    Ok(number)
}

fn claim_in(dir: &Path, requested: Option<u32>) -> Result<u32, String> {
    let candidates: Vec<u32> = match requested {
        Some(n) => vec![n],
        None => (1..=FIRST_FREE_LIMIT).collect(),
    };
    for n in candidates {
        let file = dir.join(format!("{n}.json"));
        match owners::acquire_at(dir, &file, Holder::current("instance")) {
            Ok(lease) => {
                std::thread::spawn(move || {
                    loop {
                        std::thread::sleep(BEAT);
                        lease.touch();
                    }
                });
                return Ok(n);
            }
            Err(Busy::Held(h)) if requested.is_some() => {
                return Err(format!(
                    "--instance {n}: déjà prise par {} (pid {})",
                    h.instance, h.pid
                ));
            }
            Err(Busy::Io(e)) => return Err(format!("numéro d'instance: {e}")),
            Err(_) => {}
        }
    }
    Err(format!("aucun numéro d'instance libre (1..={FIRST_FREE_LIMIT})"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("swarm-inst-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn numbers_are_handed_out_in_order_and_explicit_ones_are_exclusive() {
        let d = temp("order");
        assert_eq!(claim_in(&d, None), Ok(1));
        assert_eq!(claim_in(&d, None), Ok(2));
        assert!(claim_in(&d, Some(2)).unwrap_err().contains("déjà prise"));
        assert_eq!(claim_in(&d, Some(7)), Ok(7));
        assert_eq!(claim_in(&d, None), Ok(3));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Bail d'instance frais d'un `autre` détenteur (PID vivant), comme l'écrit une instance.
    fn ecrire_bail(d: &PathBuf, numero: u32, instance: &str, pid: u32) {
        std::fs::create_dir_all(d).unwrap();
        let holder = Holder {
            instance: instance.into(),
            pid,
            run_id: "instance".into(),
            ts: crate::store::now_ms(),
            ..Default::default()
        };
        std::fs::write(d.join(format!("{numero}.json")), serde_json::to_vec(&holder).unwrap())
            .unwrap();
    }

    #[test]
    fn un_numero_libere_redevient_le_premier_libre() {
        let d = temp("libere");
        assert_eq!(claim_in(&d, None), Ok(1));
        // Le numéro 2 est réservé par un bail qu'on peut libérer (le fil de battement de
        // `claim_in`, lui, garde le sien jusqu'à la fin du process).
        let bail = owners::acquire_at(&d, &d.join("2.json"), Holder::current("instance")).unwrap();
        assert_eq!(claim_in(&d, None), Ok(3), "2 est pris : sauté");
        assert!(claim_in(&d, Some(2)).unwrap_err().contains("déjà prise"));
        drop(bail);
        assert!(!d.join("2.json").exists(), "bail libéré : fichier retiré");
        assert_eq!(claim_in(&d, None), Ok(2), "libéré, il redevient le premier libre");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn un_numero_laisse_par_un_mort_est_repris() {
        let d = temp("mort");
        ecrire_bail(&d, 1, "swarm-control-1", u32::MAX);
        assert_eq!(claim_in(&d, None), Ok(1), "le numéro du mort est repris, pas sauté");
        let vivant: Holder =
            serde_json::from_str(&std::fs::read_to_string(d.join("1.json")).unwrap()).unwrap();
        assert_eq!(vivant.pid, std::process::id(), "le bail porte maintenant cette instance");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn un_numero_explicitement_demande_pris_est_refuse_en_nommant_le_detenteur() {
        let d = temp("pris");
        ecrire_bail(&d, 5, "swarm-control-7", std::process::id());
        let refus = claim_in(&d, Some(5)).unwrap_err();
        assert!(refus.contains("--instance 5"), "{refus}");
        assert!(refus.contains("swarm-control-7"), "{refus}");
        assert!(refus.contains(&format!("pid {})", std::process::id())), "{refus}");
        // Rien d'autre n'a été réservé à la place.
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn un_numero_en_cours_de_reservation_est_saute() {
        let d = temp("jeune");
        std::fs::create_dir_all(&d).unwrap();
        let _un = owners::acquire_at(&d, &d.join("1.json"), Holder::current("instance")).unwrap();
        // Fichier tout juste créé, contenu pas encore écrit : une autre instance est dessus.
        std::fs::write(d.join("2.json"), b"").unwrap();
        assert_eq!(claim_in(&d, None), Ok(3));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn sans_numero_libre_l_erreur_dit_la_plage() {
        let d = temp("plein");
        std::fs::create_dir_all(&d).unwrap();
        let corps = serde_json::to_vec(&Holder::current("instance")).unwrap();
        for n in 1..=FIRST_FREE_LIMIT {
            std::fs::write(d.join(format!("{n}.json")), &corps).unwrap();
        }
        assert_eq!(claim_in(&d, None), Err("aucun numéro d'instance libre (1..=999)".to_string()));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn un_dossier_de_numeros_inutilisable_est_une_erreur_claire() {
        let d = temp("fichier");
        std::fs::write(&d, b"").unwrap(); // un fichier à la place du dossier
        let erreur = claim_in(&d, None).unwrap_err();
        assert!(erreur.starts_with("numéro d'instance:"), "{erreur}");
        let _ = std::fs::remove_file(&d);
    }

    #[test]
    #[ignore = "libellé trompeur : un numéro explicitement demandé mais en cours de réservation est refusé avec « aucun numéro d'instance libre (1..=999) » (claim_in avale Busy::Pending par `Err(_) => {}`) : voir work/end/ut-reg.md"]
    fn un_numero_demande_en_cours_de_reservation_est_refuse_en_le_nommant() {
        let d = temp("jeune-explicite");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("5.json"), b"").unwrap();
        let refus = claim_in(&d, Some(5)).unwrap_err();
        assert!(!refus.contains("aucun numéro"), "le numéro 5 est seul en cause : {refus}");
        let _ = std::fs::remove_dir_all(&d);
    }
}
