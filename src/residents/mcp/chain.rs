//! Chaîne d'appels entre pairs : refus des cycles et des chaînes trop longues.
//!
//! Chaque agent résident n'exécute qu'un tour à la fois : si A demande à B pendant que B
//! demande à A, les deux s'attendent pour toujours. `via` liste les agents déjà traversés.

/// Nombre maximal d'agents dans une chaîne.
const MAX_HOPS: usize = 8;

/// `Err(message)` si `agent` est déjà dans la chaîne `via`, ou si elle est trop longue.
pub fn check(agent: &str, via: &[String]) -> Result<(), String> {
    if via.iter().any(|a| a == agent) {
        return Err(format!(
            "CYCLE: {agent} est déjà dans la chaîne [{}] : un agent ne peut pas s'attendre lui-même.",
            via.join(", ")
        ));
    }
    if via.len() >= MAX_HOPS {
        return Err(format!(
            "HOPS: chaîne de {} agents ou plus refusée (maximum {MAX_HOPS}).",
            via.len()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::check;

    fn chain(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn an_empty_or_unrelated_chain_is_fine() {
        assert!(check("alice", &[]).is_ok());
        assert!(check("alice", &chain(&["bob", "carol"])).is_ok());
    }

    #[test]
    fn an_agent_already_in_the_chain_is_a_cycle() {
        let err = check("alice", &chain(&["bob", "alice"])).unwrap_err();
        assert!(err.starts_with("CYCLE") && err.contains("[bob, alice]"), "{err}");
    }

    #[test]
    fn too_long_a_chain_is_refused() {
        let long: Vec<String> = (0..8).map(|i| format!("a{i}")).collect();
        assert!(check("zed", &long).unwrap_err().starts_with("HOPS"));
        assert!(check("zed", &long[..7]).is_ok());
    }
}
