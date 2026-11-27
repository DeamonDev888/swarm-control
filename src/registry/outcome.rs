//! Résultat final d'un run, gardé pour `agent_control status` et `wait`.

#[derive(Clone, Debug, Default)]
pub struct Outcome {
    pub result: String,
    pub error: Option<String>,
}
