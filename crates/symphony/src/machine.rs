//! The machine's Symphony: one server per machine, whose record, log and registry of the projects
//! it serves live in the machine's data folder ([`dir`]), outside every project:
//! `~/Library/Application Support/Symphony/` on macOS, `$XDG_DATA_HOME/symphony` elsewhere. A
//! project's own state never moves there: its board's data and keys stay in its git common dir,
//! and the Symphony's private files (asks, thumbnails, the queue order, uploads) under its
//! artifacts folder.
//!
//! The registry, `projects.json` ([`Registry`]), holds each project's main checkout, its id (the
//! URL segment, `/<id>/`), its project file and the plugins it names, with the pin and the board
//! schema of the build that registered it: the host rule reads them ([`Registry::add`]).
//!
//! The releases installed on the machine live there too, `releases/NAME/`, with the `current` link
//! naming the one the machine runs ([`release`]). Which build is the one server, and which projects
//! it serves, is the host rule ([`host`]); when that build is the release's, the switch
//! ([`switch`]).

pub mod command;
mod folder;
pub mod host;
mod identity;
mod registry;
pub mod release;
pub mod switch;
pub mod updates;

pub use folder::dir;
pub use identity::{installation_id, installation_id_in};
pub use registry::{Added, Entry, Registry};

/// Names the machine folder in place of the platform's: a test's own, never the owner's.
pub const DIR_ENV: &str = symphony_board::project::MACHINE_DIR;
/// The registry, in the machine folder.
pub const PROJECTS: &str = "projects.json";
/// The lock every writer of the registry takes, in the machine folder.
const LOCK: &str = "projects.lock";
/// The Symphony version a build holds, its pin: raised with each change to the wire contract or
/// the plugin seams a host and its projects share. A host serves a project registered at its pin
/// or an older one, and refuses a newer one.
pub const PIN: u32 = 1;
