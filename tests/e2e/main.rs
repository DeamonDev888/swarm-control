//! Tests de bout en bout : le vrai binaire `swarm-control`, lancé sur un port
//! éphémère avec un workspace jetable, et un faux CLI (`examples/fake_cli`) à
//! la place de `claude` / `hermes`. Aucun appel réseau, aucun jeton réel.

mod agent_mode;
mod antigravity;
mod claude_resilience;
mod claude_sessions;
mod configs;
mod control;
mod harness;
mod hermes_errors;
mod hermes_lifecycle;
mod hermes_profiles;
mod instances;
mod multi_instance;
mod parallel;
mod resident;
mod resident_hosting;
mod resident_queue;
mod rpc;
mod tools_list;
mod validation;
