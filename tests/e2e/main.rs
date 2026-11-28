//! Tests de bout en bout : le vrai binaire `swarm-control`, lancé sur un port
//! éphémère avec un workspace jetable, et un faux CLI (`examples/fake_cli`) à
//! la place de `claude` / `hermes`. Aucun appel réseau, aucun jeton réel.

mod agent_cli;
mod agent_mode;
mod antigravity;
mod archetype;
mod claude_resilience;
mod claude_sessions;
mod compiled;
mod configs;
mod contracts;
mod control;
mod golden;
mod harness;
mod hermes_errors;
mod hermes_lifecycle;
mod hermes_profiles;
mod history;
mod instances;
mod multi_instance;
mod parallel;
mod peers;
mod publish_guard;
mod resident;
mod resident_hosting;
mod resident_queue;
mod rpc;
mod templates;
mod tools_list;
mod tools_merge;
mod validation;
