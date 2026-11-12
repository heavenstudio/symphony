//! The project file's `[board]` table: the board's export, what a finished worktree may hold, the
//! guide task messages render as part of, what the board starts when a task moves to review, the cap
//! on an orchestrator's open tasks, when the pipeline alarm raises, and the size past which the
//! sweep asks for a check of the evidence it set aside.

use serde::Deserialize;
use std::collections::BTreeMap;

/// Optional per-machine orchestration advice. Zero is advice too; nothing enforces it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WipGuidance {
    pub build: Option<u32>,
    pub test: Option<u32>,
}

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
    /// The open tasks (in progress or in review) an orchestrator may hold before the board refuses
    /// it new work, which the board reads itself (`symphony_board::slots`); `None` caps nothing.
    pub orchestrator_slots: Option<u32>,
    /// The pipeline alarm, which the board reads itself (`symphony_board::alarm`): review holding
    /// more than this many times the work in progress raises it; `None` is the board's default.
    pub review_alarm_ratio: Option<u32>,
    /// The hours review must hold more than the ratio for the alarm to raise.
    pub review_alarm_hours: Option<u32>,
    /// The hours review must hold no more than the ratio for the alarm to clear.
    pub review_alarm_clear_hours: Option<u32>,
    pub machine_wip: BTreeMap<String, WipGuidance>,
}

impl Board {
    /// The guide of a project that names none, as the board's own messages name it.
    pub const GUIDE: &str = symphony_board::project::GUIDE;

    /// The document task messages render as part of.
    pub fn guide(&self) -> &str {
        self.guide.as_deref().unwrap_or(Self::GUIDE)
    }

    /// The table as written, each path checked to stay inside the project.
    pub(super) fn from_file(file: FileBoard) -> Result<Board, String> {
        let inside =
            |key: &str, path: Option<String>| path.map(|path| super::inside(key, path)).transpose();
        if file.orchestrator_slots == Some(0) {
            return Err(
                "[board] orchestrator_slots is 0; an orchestrator needs at least one slot".into(),
            );
        }
        for (key, value) in [
            ("review_alarm_ratio", file.review_alarm_ratio),
            ("review_alarm_hours", file.review_alarm_hours),
            ("review_alarm_clear_hours", file.review_alarm_clear_hours),
        ] {
            if value == Some(0) {
                return Err(format!("[board] {key} is 0; it is at least 1"));
            }
        }
        if file.machine_wip.len() > 64
            || file.machine_wip.keys().any(|id| {
                id.trim().is_empty() || id.len() > 256 || id.chars().any(char::is_control)
            })
        {
            return Err("[board] machine_wip needs at most 64 nonempty, control-free machine ids of at most 256 bytes".into());
        }
        // The size past which the sweep asks for a check by hand of the evidence the main checkout
        // keeps of removed worktrees: the board reads it itself (`symphony_board::sweep::kept`),
        // 5 when unwritten; checked here so a project's lint refuses a value the sweep would not
        // take.
        if let Some(gb) = file.swept_cap_gb
            && !(gb > 0.0 && gb.is_finite())
        {
            return Err(format!(
                "[board] swept_cap_gb is {gb}; it is a number of gigabytes over 0"
            ));
        }
        Ok(Board {
            export: inside("[board] export", file.export)?,
            disposable: file.disposable,
            guide: inside("[board] guide", file.guide)?,
            on_review: file.on_review,
            orchestrator_slots: file.orchestrator_slots,
            review_alarm_ratio: file.review_alarm_ratio,
            review_alarm_hours: file.review_alarm_hours,
            review_alarm_clear_hours: file.review_alarm_clear_hours,
            machine_wip: file.machine_wip,
        })
    }
}

/// The table as written: every key optional, one this build does not know kept by name.
#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct FileBoard {
    export: Option<String>,
    disposable: Vec<String>,
    guide: Option<String>,
    on_review: Vec<String>,
    orchestrator_slots: Option<u32>,
    review_alarm_ratio: Option<u32>,
    review_alarm_hours: Option<u32>,
    review_alarm_clear_hours: Option<u32>,
    machine_wip: BTreeMap<String, WipGuidance>,
    swept_cap_gb: Option<f64>,
    #[serde(flatten)]
    pub(super) unknown: super::file::Unknown,
}
