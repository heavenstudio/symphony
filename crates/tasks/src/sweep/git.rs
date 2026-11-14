//! Repository reads and final deletion guards used by the worktree sweep.

mod command;
mod files;
mod locks;
mod owner;
mod worktrees;

pub use command::{git, rev, succeeds};
pub(crate) use command::{read, stdout, told};
pub use files::{DISPOSABLE, FINISHED_OUTPUT, fnmatch, last_touched, left_behind, modified};
pub(super) use files::{admin, clean_regenerable};
pub(super) use files::{file_status, regenerable};
pub(super) use locks::SAVED_LOCK;
pub(super) use locks::{WorktreeLock, hold_head};
pub use owner::{Answer, answers, under};
pub use worktrees::{Listed, ancestor, parse_worktrees, toplevel, worktrees};
