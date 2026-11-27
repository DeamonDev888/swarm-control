//! Client MCP minimal vers un autre serveur : découverte, appel d'outil.

mod call;
mod discover;
mod http;
mod url;

pub use call::{CallError, call_tool, list_tools};
pub use discover::{Peer, discover, find, instances};
