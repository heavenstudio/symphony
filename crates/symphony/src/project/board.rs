//! The project file's `[board]` table: the board's export, what a finished worktree may hold, the
//! guide task messages render as part of, and what the board starts when a task moves to review.

use serde::Deserialize;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Board {
    /// Where the board's export is written, project-relative; `None` writes none.
    pub export: Option<String>,
    /// What a finished worktree may hold and still be removed.
    pub disposable: Vec<String>,
    /// The board's guide, project-relative: task messages' links and pictures resolve from it as
    /// they do on its own page. `None` is [`Board::GUIDE`].
    pub guide: Option<String>,
    /// The command the board starts, detached from the main checkout, when a task moves to review,
    /// given the task's slug, its branch and the branch's tip; empty starts none.
    pub on_review: Vec<String>,
}

impl Board {
    /// The guide of a project that names none.
    pub const GUIDE: &str = "TASKS.md";

    /// The document task messages render as part of.
    pub fn guide(&self) -> &str {
        self.guide.as_deref().unwrap_or(Self::GUIDE)
    }

    /// The table as written, each path checked to stay inside the project.
    pub(super) fn from_file(file: FileBoard) -> Result<Board, String> {
        let inside =
            |key: &str, path: Option<String>| path.map(|path| super::inside(key, path)).transpose();
        Ok(Board {
            export: inside("[board] export", file.export)?,
            disposable: file.disposable,
            guide: inside("[board] guide", file.guide)?,
            on_review: file.on_review,
        })
    }
}

/// The table as written: every key optional, nothing else allowed.
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub(super) struct FileBoard {
    export: Option<String>,
    disposable: Vec<String>,
    guide: Option<String>,
    on_review: Vec<String>,
}
