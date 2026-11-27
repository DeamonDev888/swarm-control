//! Exécution d'un agent : Claude Code ou Hermes, avec reprise de session.
//!
//! Cycle commun (`prepare` puis `execute`) :
//! 1. résoudre l'agent et réserver le run dans le registre ;
//! 2. reprendre la dernière session de l'agent (`autoResume`) ;
//! 3. lancer le CLI (`claude/` ou `hermes/`), enregistrer la session rendue ;
//! 4. session reprise invalide : l'oublier et relancer à neuf, une fois.

mod claude;
mod end;
mod execute;
mod guard;
mod hermes;
mod prepare;
mod request;
mod result;
mod session;
mod text;

pub use execute::execute;
pub use prepare::prepare;
pub use request::RunRequest;
pub use result::RunResult;
