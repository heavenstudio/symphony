//! What a dry run says of every worktree it visits, one line each: what it holds on disk, the tasks
//! tied to it and their stage, whether its HEAD is in main, and what the sweep would do with it and
//! why. Its summary then says what the sweep would free.

use super::size;
use std::path::PathBuf;

/// One worktree as the sweep found it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Row {
    /// As git lists it.
    pub path: PathBuf,
    /// What it holds on disk ([`size::bytes`]); none when not measured (the main checkout, or a
    /// pass that is not a dry run).
    pub bytes: Option<u64>,
    /// The tasks tied to it, each `slug (status)`.
    pub tasks: Vec<String>,
    /// Whether its HEAD is in main; none when not read.
    pub merged: Option<String>,
    /// What the sweep does with it, and why.
    pub verdict: String,
}

impl Row {
    /// The row as one line: `worktree PATH: SIZE; TASKS; MERGED; VERDICT`.
    pub fn line(&self) -> String {
        let mut parts = Vec::new();
        if let Some(bytes) = self.bytes {
            parts.push(size::shown(bytes));
        }
        parts.push(match self.tasks.as_slice() {
            [] => "no task".to_string(),
            [one] => format!("task {one}"),
            many => format!("tasks {}", many.join(", ")),
        });
        parts.extend(self.merged.clone());
        parts.push(self.verdict.clone());
        format!("worktree {}: {}", self.path.display(), parts.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_says_its_size_tasks_merge_and_verdict_in_one_line() {
        let row = Row {
            path: PathBuf::from("/wt/a"),
            bytes: Some(12_300_000_000),
            tasks: vec!["tools/x (review)".into()],
            merged: Some("merged into main".into()),
            verdict: "kept: a land runs there (pid 7)".into(),
        };
        assert_eq!(
            row.line(),
            "worktree /wt/a: 12.3 GB; task tools/x (review); merged into main; kept: a land runs there (pid 7)"
        );
        let bare = Row {
            path: PathBuf::from("/r"),
            tasks: vec!["a (done)".into(), "b (cancelled)".into()],
            verdict: "kept: the main checkout".into(),
            ..Row::default()
        };
        assert_eq!(
            bare.line(),
            "worktree /r: tasks a (done), b (cancelled); kept: the main checkout"
        );
        let none = Row {
            verdict: "would remove".into(),
            ..Row::default()
        };
        assert_eq!(none.line(), "worktree : no task; would remove");
    }
}
