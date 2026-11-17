//! A task's changed files and the patch of a file opened from that summary. Commit ids bind
//! both reads to one immutable comparison; reading a patch never follows a moving branch tip.

use serde::{Deserialize, Serialize};

/// The route that answers a [`TaskDiff`]: `GET /api/task-diff?slug=SLUG`.
pub const TASK_DIFF_PATH: &str = "/api/task-diff";
/// The route that answers a [`TaskDiffPatch`]:
/// `GET /api/task-diff-file?slug=SLUG&path=PATH&base=BASE&head=HEAD`.
/// `base` and `head` are the commit ids from the summary whose file the reader opened.
pub const TASK_DIFF_FILE_PATH: &str = "/api/task-diff-file";

/// Where a task's comparison comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "kebab-case")]
pub enum TaskDiffSource {
    /// The default branch's merge base with the task branch, compared with the task branch tip.
    Branch,
    /// The landing merge's first parent, compared with the landing merge itself.
    Landing,
    /// No comparison could be read; [`TaskDiff::reason`] says why.
    Unavailable,
}

/// A task's changed files, as [`TASK_DIFF_PATH`] answers them, without any file's patch text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct TaskDiff {
    pub source: TaskDiffSource,
    /// The full base commit id; null when no comparison is available.
    pub base: Option<String>,
    /// The full head commit id; null when no comparison is available.
    pub head: Option<String>,
    pub files: Vec<TaskDiffFile>,
    /// The answer leaves out some of the comparison because a read bound was reached.
    pub truncated: bool,
    /// Why the comparison is unavailable or incomplete, otherwise null. An unavailable answer
    /// explains a gone branch without a landing, missing history or a failed or timed-out read.
    pub reason: Option<String>,
}

/// One repository-relative path in a [`TaskDiff`]. Both counts are null for a binary file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct TaskDiffFile {
    pub path: String,
    pub added: Option<u64>,
    pub removed: Option<u64>,
}

/// One file's patch in the immutable comparison named by the request to [`TASK_DIFF_FILE_PATH`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct TaskDiffPatch {
    pub base: String,
    pub head: String,
    /// The repository-relative path the request named.
    pub path: String,
    pub patch: String,
    /// The patch text stops at a read bound.
    pub truncated: bool,
}
