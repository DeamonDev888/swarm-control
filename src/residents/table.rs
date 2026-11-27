//! Table (en mémoire) des agents hébergés par ce process.

use super::host::AgentHost;
use crate::agents::Runner;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

type Map = HashMap<String, Arc<AgentHost>>;

static HOSTS: Mutex<Option<Map>> = Mutex::new(None);

fn hosts() -> MutexGuard<'static, Option<Map>> {
    HOSTS.lock().unwrap_or_else(|e| e.into_inner())
}

fn key(runner: Runner, agent: &str) -> String {
    format!("{}:{agent}", runner.as_str())
}

pub fn insert(host: Arc<AgentHost>) {
    let k = key(host.ctx().runner, &host.ctx().agent);
    hosts().get_or_insert_with(HashMap::new).insert(k, host);
}

pub fn remove(runner: Runner, agent: &str) -> Option<Arc<AgentHost>> {
    hosts().as_mut()?.remove(&key(runner, agent))
}

/// Les agents hébergés ici sous ce nom (un par runner au plus), éventuellement filtrés par runner.
pub fn find(agent: &str, runner: Option<Runner>) -> Vec<Arc<AgentHost>> {
    hosts()
        .as_ref()
        .map(|m| {
            m.values()
                .filter(|h| h.ctx().agent == agent && runner.is_none_or(|r| h.ctx().runner == r))
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}
