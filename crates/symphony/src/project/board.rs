//! The project file's `[board]` table: the guide task messages render as part of, what the board
//! starts when a task moves to review and once one lands, the cap on an orchestrator's open tasks,
//! when the pipeline alarm raises, the size past which the sweep asks for a check of the evidence
//! it set aside, how long a build the project no longer runs holds its board's move to a release
//! (`crate::machine::writers`), the folder of the machine's PostgreSQL programs, where a hosted
//! board's PostgreSQL is (`[board.external_postgresql]`, `symphony_board::pg::external`), and the
//! project's own tools the board reads beside its own: a machine-work reader, a folder of
//! test-slot locks and a folder of landing assessments. A path there is project-relative, and one
//! under `.git/` names the repository's common git folder, but PostgreSQL's, which is the
//! machine's and absolute; none is the Symphony's default, which reads nothing there.

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
    /// The board's guide, project-relative: task messages' links and pictures resolve from it as
    /// they do on its own page. `None` is [`Board::GUIDE`].
    pub guide: Option<String>,
    /// The command the board starts, detached from the main checkout, when a task moves to review,
    /// given the task's slug, its branch and the branch's tip; empty starts none.
    pub on_review: Vec<String>,
    /// The command the board starts, detached from the main checkout, once `just land` closed a
    /// task, given the task's slug and the merge that landed it; empty starts none
    /// (`symphony_board::on_land`).
    pub on_land: Vec<String>,
    /// The open tasks (in progress or in review) an orchestrator may hold before the board refuses
    /// it new work, which the board reads itself (`symphony_board::slots`) and judges as it judges
    /// `[limits]`'s (`symphony_board::limits::judged`); `None` caps nothing, as a value it cannot
    /// read does, which [`super::Project::parse`] refuses as the board says it.
    pub orchestrator_slots: Option<u32>,
    /// The pipeline alarm, which the board reads itself (`symphony_board::alarm`): review holding
    /// more than this many times the work in progress raises it; `None` is the board's default.
    pub review_alarm_ratio: Option<u32>,
    /// The hours review must hold more than the ratio for the alarm to raise.
    pub review_alarm_hours: Option<u32>,
    /// The hours review must hold no more than the ratio for the alarm to clear.
    pub review_alarm_clear_hours: Option<u32>,
    pub machine_wip: BTreeMap<String, WipGuidance>,
    /// The project's own machine-work reader, an executable the server's machine-work scan runs
    /// beside the board's recorder when it is there (`SYMPHONY_MACHINE_WORK_READER` names another);
    /// `None` runs none.
    pub machine_work_reader: Option<String>,
    /// The folder of the project's test-slot locks, which the board's machine probe reads itself
    /// (`symphony_board::cli::probes`); `None` probes none.
    pub slot_locks: Option<String>,
    /// The folder of the project's landing assessments, which the board's assessment API reads itself
    /// (`symphony_board::assessment_publication`); `None` reads none, and an assessment there is refused.
    pub assessments: Option<String>,
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
        let inside = |key: &str, path: Option<String>| {
            path.map(|path| super::inside(key, &path)).transpose()
        };
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
        // The hours a build the project no longer runs holds its board's move to a release, which
        // the writer rule reads itself (`crate::machine::writers::Runs`), a day when unwritten;
        // checked here so a project's lint refuses a value the rule would not take.
        if file.stale_writer_hours == Some(0) {
            return Err(format!(
                "[board] {} is 0; it is a whole number of hours over 0",
                crate::machine::writers::UNUSED_KEY
            ));
        }
        // The folder of the machine's PostgreSQL programs, which the board reads itself
        // (`symphony_board::server::postgres::programs`): the machine's own, so an absolute path,
        // never one inside the project; checked here so a project's lint refuses what the board
        // would not take.
        if let Some(folder) = &file.postgresql
            && !std::path::Path::new(folder).is_absolute()
        {
            return Err(format!(
                "[board] postgresql is {folder:?}; it is the absolute folder of the machine's \
                 PostgreSQL programs"
            ));
        }
        // Where a hosted board's PostgreSQL is and the budget of its connections, which the board
        // reads itself (`symphony_board::pg::external::of`); judged here by the board's own rule,
        // so a project's lint refuses what a hosted build would not take.
        if let Some(external) = file.external_postgresql {
            symphony_board::pg::external::judged(external)?;
        }
        Ok(Board {
            guide: inside("[board] guide", file.guide)?,
            on_review: file.on_review,
            on_land: file.on_land,
            orchestrator_slots: file.orchestrator_slots.and_then(|value| {
                let cap = symphony_board::limits::judged(symphony_board::limits::SLOTS, &value);
                cap.ok().and_then(|cap| u32::try_from(cap).ok())
            }),
            review_alarm_ratio: file.review_alarm_ratio,
            review_alarm_hours: file.review_alarm_hours,
            review_alarm_clear_hours: file.review_alarm_clear_hours,
            machine_wip: file.machine_wip,
            machine_work_reader: inside("[board] machine_work_reader", file.machine_work_reader)?,
            slot_locks: inside("[board] slot_locks", file.slot_locks)?,
            assessments: inside("[board] assessments", file.assessments)?,
        })
    }
}

/// The table as written: every key optional, one this build does not know kept by name.
#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct FileBoard {
    guide: Option<String>,
    on_review: Vec<String>,
    on_land: Vec<String>,
    /// Any value the file writes, so one the board cannot read leaves the rest of the project read,
    /// as `[limits]`'s do (`super::file::FileLimits`).
    orchestrator_slots: Option<toml::Value>,
    review_alarm_ratio: Option<u32>,
    review_alarm_hours: Option<u32>,
    review_alarm_clear_hours: Option<u32>,
    machine_wip: BTreeMap<String, WipGuidance>,
    machine_work_reader: Option<String>,
    slot_locks: Option<String>,
    assessments: Option<String>,
    swept_cap_gb: Option<f64>,
    stale_writer_hours: Option<u32>,
    postgresql: Option<String>,
    external_postgresql: Option<toml::Value>,
    #[serde(flatten)]
    pub(super) unknown: super::file::Unknown,
}
