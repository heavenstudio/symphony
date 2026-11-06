//! The attach monitor (`just tasks-attach`): a session's ears. It heartbeats and, every few
//! seconds, prints what reached the session: messages and verdicts, roles taken and dropped on its
//! tasks, their status changes, new ready work for an orchestrator, and a child session going stale
//! or lost with work another could take. An orchestrator also hears the PULL digest
//! ([`super::pull`]) only for actionable entries absent from its last delivered digest. The pane
//! watch ([`pane_watch`]) tells an orchestrator or a parent once when a session it oversees waits on
//! an approval in its hc terminal, and once when hc cannot be asked. What an orchestrator hears
//! is its own tasks and the ones nobody orchestrates ([`crate::orchestration`]).

mod events;
mod panes;
mod seen;
mod watch;

pub use events::diff;
pub use panes::{Blocked, Overseen, PaneWatch, overseen, pane_watch};
pub use seen::{Seen, observe};
pub(crate) use seen::{pull_changed, seen};
pub use watch::{Watch, watch};

#[cfg(test)]
mod tests;
