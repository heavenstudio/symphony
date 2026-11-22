//! The machine's server trying a held update again at each round of a project's idle sweep
//! ([`symphony::machine::held`], docs/workflow/RELEASES.md "A held update tried again"): only the one
//! server, only while it is an installed release, in that project's own maintenance worker, so no
//! request waits on a move and one project's move holds up no other's sweep. The retry runs each
//! command it starts as the sweep runs its reap: in a group of its own, ended at its bound or once
//! the server stops ([`crate::sys::output_while`]).

use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::Arc;
use std::time::Duration;

use symphony::machine::held::{self, Host, Retry};
use symphony::machine::updates::TaskBoard;
use symphony::taskcli::CliBoard;
use symphony_board::sys::Command;

use crate::front::{Clock, Shared};

/// What a project's maintenance needs to try its held update again: the machine folder, and the
/// installed release this server is.
#[derive(Clone, Debug)]
pub(crate) struct Retrying {
    pub(crate) machine: PathBuf,
    pub(crate) release: String,
}

impl Retrying {
    /// The one server's, with its machine folder `home`: none for a server that is no installed
    /// release (a checkout's build), which tries nothing again.
    pub(crate) fn of_this_server(home: &Path) -> Option<Retrying> {
        let (_, manifest) = symphony::machine::release::this()?;
        Some(Retrying {
            machine: home.to_path_buf(),
            release: manifest.release.clone(),
        })
    }
}

/// One round's retry of the held update of the project `shared` serves, each command within
/// `bound` on `clock`: what it did, in the log's lines. A server of its own, which serves its one
/// project at `/`, tries nothing.
pub(crate) fn round(
    shared: &Arc<Shared>,
    retrying: &Retrying,
    bound: Duration,
    clock: &impl Clock,
) -> Vec<String> {
    let project = shared.base.trim_start_matches('/');
    if project.is_empty() {
        return Vec::new();
    }
    let host = Server { shared, clock };
    held::retry(
        &Retry {
            machine: &retrying.machine,
            release: &retrying.release,
            project,
            bound,
        },
        &host,
    )
}

/// The server as a retry needs it.
struct Server<'a, C: Clock> {
    shared: &'a Arc<Shared>,
    clock: &'a C,
}

impl<C: Clock> Host for Server<'_, C> {
    fn now(&self) -> f64 {
        let at = self.clock.now();
        at.wall_seconds()
    }

    fn run(&self, command: Command, bound: Duration) -> Result<Output, String> {
        crate::sys::output_while(
            command,
            bound,
            || !self.shared.stopping(),
            || {
                let at = self.clock.now();
                at.both()
            },
        )
    }

    /// The project loaded here, its hub made, and its board opened afresh at `schema`, which this
    /// build writes: never the hub's last word alone, which a read before the move gave.
    fn writes(&self, schema: i64) -> Result<(), String> {
        if self.shared.api.get().is_none() {
            return Err("this server has not loaded the project".into());
        }
        held::writes(&self.shared.root, schema)
    }

    fn ours(&self, main: &Path) -> Box<dyn TaskBoard> {
        Box::new(CliBoard::new(main))
    }

    fn theirs(&self, root: &Path, tasks: &Path) -> Box<dyn TaskBoard> {
        let mut cleared: Vec<&str> = symphony::machine::moving::UNSET.to_vec();
        cleared.extend(symphony_board::sys::GIT_VARS);
        Box::new(
            CliBoard::with_command(vec![tasks.to_string_lossy().into_owned()], &[], root)
                .without(&cleared),
        )
    }
}

#[cfg(test)]
#[path = "held_tests.rs"]
mod tests;
