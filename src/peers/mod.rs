//! Client MCP minimal vers un autre serveur : découverte, pairs automatiques, appel d'outil.

mod call;
mod discover;
mod http;
pub(crate) mod pairs;
mod url;

pub use call::{CallError, call_tool, list_tools};
pub use discover::{Peer, discover, find, instances};
