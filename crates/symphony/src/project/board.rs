//! The `[board]` table: where the board's export goes, what a finished worktree may hold, and
//! what the board starts when a task moves to review.

use serde::Deserialize;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Board {
    /// Where the board's export is written, project-relative; `None` writes none.
    pub export: Option<String>,
    /// What a finished worktree may hold and still be removed.
    pub disposable: Vec<String>,
    /// The command the board starts, detached from the main checkout, when a task moves to review,
    /// given the task's slug, its branch and the branch's tip; empty starts none.
    pub on_review: Vec<String>,
}

/// The table as written: every key optional, nothing else allowed.
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub(super) struct FileBoard {
    pub(super) export: Option<String>,
    pub(super) disposable: Vec<String>,
    pub(super) on_review: Vec<String>,
}
