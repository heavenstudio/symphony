//! The board scope every host of a project's attempts reads and controls them under.

use super::Scope;
use crate::store::Store;

/// The scope of project `project`'s board in `store`: the project's registered id and the board's
/// database. Every host of the board's attempts (the server, `tasks api`, a launcher) admits,
/// reads and controls them under this one, so none refuses another's as a different board's.
pub fn board_scope(project: &str, store: &Store) -> Scope {
    Scope::new(project.to_string(), store.database().database().to_string())
}
