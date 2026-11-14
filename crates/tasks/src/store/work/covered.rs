//! A task whose work landed with another task's branch, closed as covered by that landing
//! ([`Store::close_covered`]): by its executor or its orchestrator, with no reviewer's verdict, as
//! nothing of its own is left to judge or to land. A task whose fix landed with another's can be
//! closed or cancelled, as one left open would be a fault of the process.
//!
//! The close is no approve. Its line is the task's last verdict, so no approve stands on the task
//! once it is closed: a landing finds none to go by (`crate::land::approval`), a move back to review
//! finds none to reopen ([`super::approved`]), and an approve given before it, on a task sent back
//! to review since, is passed over the same way. The board cannot see the repository: the command
//! line and the API read it first (`crate::covering::close`), for the landing's commit on the
//! default branch and for a branch of the task's own with commits the default branch lacks.

use super::{Change, Error, Message, Say, Store, Task, refused};
use crate::health::FINISHED;

/// What starts the line a covered close leaves on the thread, as its verdict.
pub const COVERED: &str = "covered by ";

/// The roles whose holder closes a task as covered by another's landing.
const CLOSERS: [&str; 2] = ["executor", "orchestrator"];

/// The commit a landing's line on `thread` names: the thread's last verdict, when it starts
/// `landed `, and the first commit its first line names after its first ` at `. The Symphony's land
/// writes `landed SLUG on DEFAULT at SHA`; a consuming project's own, through [`Store::landed`],
/// may say more after the commit (`landed BRANCH on main at SHA; stamps counted: …`).
pub fn landing(thread: &[Message]) -> Option<&str> {
    let last = thread.iter().rev().find(|m| m.kind == "verdict")?;
    let first = last.text.strip_prefix("landed ")?.lines().next()?;
    let (_, at) = first.split_once(" at ")?;
    crate::land::named(at).first().copied()
}

/// The command that closes `slug` as covered by another's landing, as `board`
/// ([`Store::board_command`]) reaches it: what a refusal of another way to close it names.
pub(crate) fn covered_command(board: &str, slug: &str) -> String {
    format!("{board} done {slug} NOTE --covered-by OTHER")
}

/// The line a covered close writes as `slug`'s verdict: the covering task, the commit its landing
/// names (`commit`, shortened as a landing's line shortens it), who closed it and why.
pub fn covered_line(other: &str, commit: &str, handle: &str, note: &str) -> String {
    format!(
        "{COVERED}{other}, whose landing at {} holds its work; closed by {handle}: {}",
        &commit[..commit.len().min(12)],
        note.trim()
    )
}

/// What a covered close's line says ([`covered_line`]), read back by [`covered_by`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoveredLine<'a> {
    /// The task whose landing holds the work.
    pub other: &'a str,
    /// The commit its landing names, as the line shortens it.
    pub commit: &'a str,
    /// Who closed it.
    pub closer: &'a str,
    pub note: &'a str,
}

/// The parts of `text` when it is a covered close's line ([`covered_line`]); none for any other.
pub fn covered_by(text: &str) -> Option<CoveredLine<'_>> {
    let rest = text.strip_prefix(COVERED)?;
    let (other, rest) = rest.split_once(", whose landing at ")?;
    let (commit, rest) = rest.split_once(" holds its work; closed by ")?;
    let (closer, note) = rest.split_once(": ")?;
    Some(CoveredLine {
        other,
        commit,
        closer,
        note,
    })
}

impl Store {
    /// The commit `other`'s landing names, for a task its landing covers ([`Store::close_covered`]):
    /// `other` is done, closed by its landing, whose line names the commit ([`landing`]).
    /// Otherwise the one line saying what `other` is instead.
    pub fn covering(&self, other: &str) -> Result<String, Error> {
        let task = self.get(other)?;
        if task.status != "done" {
            return Err(refused(format!(
                "{other} is {}: a task is closed as covered only by a task done and landed",
                task.status
            )));
        }
        let thread = self.thread(other)?;
        if let Some(commit) = landing(&thread) {
            return Ok(commit.to_string());
        }
        let last = thread.iter().rev().find(|m| m.kind == "verdict");
        Err(refused(match last {
            Some(line) if line.text.starts_with("landed ") => format!(
                "{other}'s landing names no commit (\"{}\"): a task is closed as covered only by \
                 a landing whose merge the default branch holds",
                line.text.lines().next().unwrap_or_default()
            ),
            _ => format!(
                "{other} is done, but no landing closed it: a task is closed as covered only by a \
                 task done and landed"
            ),
        }))
    }

    /// `slug`, open, closed as covered by `other`'s landing: by `handle`, its executor or its
    /// orchestrator (the roles held on it), with `note` saying what came of it. `commit` is the one
    /// `other`'s landing names ([`Store::covering`]), as the repository resolved it on the default
    /// branch (`crate::covering::close`, which runs this). The thread's line, its verdict, names the
    /// covering task, the commit and who closed it ([`covered_line`]), and the task is done. It is
    /// no approve, and no session's verdict: a fresh reviewer's approve is still the only one a
    /// landing goes by.
    pub fn close_covered(
        &self,
        slug: &str,
        handle: &str,
        other: &str,
        commit: &str,
        note: &str,
    ) -> Result<Task, Error> {
        if note.trim().is_empty() {
            return Err(refused(
                "closing a task needs a note saying what came of it".into(),
            ));
        }
        if slug == other {
            return Err(refused(format!(
                "{slug} is not covered by a landing of its own: it lands by a fresh reviewer's \
                 approve"
            )));
        }
        let named = self.covering(other)?;
        if !commit.starts_with(&named) {
            return Err(refused(format!(
                "{other}'s landing names {named}, not {commit}"
            )));
        }
        let board = self.board_command();
        let line = covered_line(other, commit, handle, note);
        self.rule(slug, handle, |view| {
            view.actor(handle)?;
            let task = view.task(slug)?;
            if FINISHED.contains(&task.status.as_str()) {
                return Err(refused(format!(
                    "{slug} is {}: only an open task is closed as covered by another's landing",
                    task.status
                )));
            }
            if !CLOSERS.iter().any(|role| view.holds(handle, role)) {
                return Err(refused(format!(
                    "{handle} is not {slug}'s executor or orchestrator, which alone close it as \
                     covered by another's landing; its orchestrator takes the role first: {board} \
                     role {slug} orchestrator"
                )));
            }
            Ok(Change {
                status: Some("done".into()),
                say: Some(Say {
                    kind: "verdict",
                    text: line.clone(),
                    recipient: None,
                }),
                ..Change::default()
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verdict(text: &str) -> Message {
        Message {
            id: 1,
            slug: "a/b".into(),
            sender: "lander".into(),
            recipient: None,
            kind: "verdict".into(),
            text: text.into(),
            ts: 1.0,
        }
    }

    fn said(kind: &str, text: &str) -> Message {
        Message {
            kind: kind.into(),
            ..verdict(text)
        }
    }

    #[test]
    fn a_landing_names_the_commit_after_its_at() {
        for (text, commit) in [
            (
                "landed a/b on main at 3e0c66192471; pushed to origin",
                "3e0c66192471",
            ),
            ("landed a/b on trunk at 3e0c661", "3e0c661"),
            // A consuming project's own land, through `Store::landed`: its branch, which may look
            // like a commit, and the stamps it counted after the commit.
            (
                "landed review/abc1234 on main at 0e97e33aa123; stamps counted: certificate, \
                 nightly, dependency",
                "0e97e33aa123",
            ),
            (
                "landed a/b on main at 3e0c66192471, finishing a land interrupted after its merge",
                "3e0c66192471",
            ),
            (
                "landed a/b on main at 3e0c66192471\nmore at 1234567",
                "3e0c66192471",
            ),
        ] {
            assert_eq!(landing(&[verdict(text)]), Some(commit), "{text}");
        }
    }

    #[test]
    fn only_a_landing_that_is_the_last_verdict_names_one() {
        for text in [
            "landed a/b on main",
            "landed a/b on main at nothing",
            "approve: 3e0c66192471",
            "done: landed at 3e0c66192471",
            "covered by c/d, whose landing at 3e0c66192471 holds its work; closed by x: y",
        ] {
            assert_eq!(landing(&[verdict(text)]), None, "{text}");
        }
        let landed = verdict("landed a/b on main at 3e0c661");
        // A verdict after the landing is the last; a post or a system line is no verdict.
        assert_eq!(landing(&[landed.clone(), verdict("changes: again")]), None);
        assert_eq!(
            landing(&[
                landed,
                said("msg", "landed x on main at 1234567"),
                said("system", "x moved it done → review"),
            ]),
            Some("3e0c661")
        );
        assert_eq!(landing(&[]), None);
    }

    #[test]
    fn the_line_names_the_covering_task_the_commit_and_who_closed_it() {
        assert_eq!(
            covered_line(
                "tests/timeline",
                "75b4516a0123456789abcdef0123456789abcdef",
                "fleet-otter",
                "  its fix is 97e84883 \n"
            ),
            "covered by tests/timeline, whose landing at 75b4516a0123 holds its work; closed by \
             fleet-otter: its fix is 97e84883"
        );
        assert_eq!(
            covered_line("t/x", "75b4516", "h", "n"),
            "covered by t/x, whose landing at 75b4516 holds its work; closed by h: n"
        );
    }

    #[test]
    fn a_covered_line_reads_back_into_its_parts_and_no_other_line_does() {
        let line = covered_line("tests/timeline", "75b4516a0123", "fleet-otter", "a: b\nc");
        assert_eq!(
            covered_by(&line),
            Some(CoveredLine {
                other: "tests/timeline",
                commit: "75b4516a0123",
                closer: "fleet-otter",
                note: "a: b\nc",
            })
        );
        for text in [
            "approve: good",
            "landed a/b on main at 3e0c66192471",
            "covered by a/b",
            "covered by a/b, whose landing at 3e0c661 holds its work",
        ] {
            assert_eq!(covered_by(text), None, "{text}");
        }
    }
}
