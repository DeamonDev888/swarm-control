//! Client MCP minimal (std) vers un autre serveur LiteMCP : un agent résident, ou une autre
//! instance. Profil strict de LiteMCP : en-têtes `Mcp-Method` / `Mcp-Name`, `_meta` dans les paramètres.

mod call;
mod http;
mod url;

pub use call::{CallError, call_tool};
