//! Un run est-il routable vers un hôte ?

use crate::owners::{self, Holder};
use crate::runners::RunRequest;

/// L'hôte vivant de l'agent demandé, si le run peut lui être confié tel quel.
///
/// `ask` reprend la session de l'agent, dans le workspace de l'hôte : un run qui impose sa
/// propre session, un répertoire (`path`), un fournisseur ou refuse la reprise reste local.
pub fn host_of(req: &RunRequest) -> Option<Holder> {
    let agent = req.agent.as_deref()?;
    let plain =
        req.session_id.is_none() && req.auto_resume && req.cwd.is_none() && req.provider.is_none();
    if !plain {
        return None;
    }
    owners::host_of(&req.workspace, req.runner.as_str(), agent).filter(|h| h.url.is_some())
}
