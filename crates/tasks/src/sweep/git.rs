//! Repository reads and final deletion guards used by the worktree sweep.

mod command;
mod files;
mod locks;
mod owner;
mod worktrees;

pub(crate) use command::{Object, blobs, objects, read, stdout, told};
pub use command::{git, rev, succeeds};
pub use files::{DISPOSABLE, FINISHED_OUTPUT, fnmatch, last_touched, left_behind, modified};
pub(super) use files::{admin, clean_regenerable};
pub(super) use files::{file_status, regenerable};
pub(super) use locks::SAVED_LOCK;
pub(super) use locks::{WorktreeLock, hold_head};
pub(crate) use owner::pathspecs;
pub use owner::{Answer, answers, under};
pub use worktrees::{Listed, ancestor, parse_worktrees, toplevel, worktrees};
