//! A worktree a merge (or a rebase, a cherry-pick) stopped in: a document whose file holds conflict
//! markers while git holds it unmerged ([`conflict`]). Its branch's page shows the branch's last
//! committed copy, never the markers ([`committed`]); the scan reads the branch's side of each
//! conflict ([`branch_side`]); and an owner's answer on a card is saved on that side
//! ([`Conflict::with_card`]), for whoever finishes the merge to keep.

use std::borrow::Cow;
use std::path::Path;

use crate::register::parse_decisions;
use crate::text::splitlines;

/// A file's lines with conflict markers, and which of them are the branch's side: every line
/// outside a conflict, and each conflict's first side (git's "ours", the checkout's HEAD).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conflict {
    lines: Vec<String>,
    /// The indices in `lines` of the branch's side, in order.
    ours: Vec<usize>,
}

/// Whether `line` is git's conflict marker of seven `mark`s: alone, or before a space and a label.
fn marker(line: &str, mark: char) -> bool {
    let mut rest = line;
    for _ in 0..7 {
        let Some(after) = rest.strip_prefix(mark) else {
            return false;
        };
        rest = after;
    }
    rest.is_empty() || rest.starts_with(' ')
}

impl Conflict {
    /// The conflicts `text` holds, none unless it holds at least one and each is whole: a `<<<<<<<`
    /// line, the branch's side, an optional `|||||||` base, `=======`, the other side, `>>>>>>>`.
    pub fn parse(text: &str) -> Option<Conflict> {
        #[derive(Clone, Copy, PartialEq)]
        enum At {
            Outside,
            Ours,
            Base,
            Theirs,
        }
        let lines: Vec<String> = splitlines(text).into_iter().map(str::to_string).collect();
        let mut at = At::Outside;
        let mut ours = Vec::new();
        let mut whole = 0;
        for (index, line) in lines.iter().enumerate() {
            at = match at {
                At::Outside if marker(line, '<') => At::Ours,
                At::Ours if marker(line, '|') => At::Base,
                At::Ours | At::Base if line == "=======" => At::Theirs,
                At::Theirs if marker(line, '>') => {
                    whole += 1;
                    At::Outside
                }
                At::Outside | At::Ours => {
                    ours.push(index);
                    at
                }
                other => other,
            };
        }
        (at == At::Outside && whole > 0).then_some(Conflict { lines, ours })
    }

    /// The branch's side of the file: its text with each conflict taken as the branch has it.
    pub fn ours(&self) -> String {
        self.ours
            .iter()
            .map(|&index| format!("{}\n", self.lines[index]))
            .collect()
    }

    /// The file with the card `slug` as `edited` (its branch's side, [`Conflict::ours`], with that
    /// card changed) holds it: the card's lines replaced where they sit, in or out of a conflict.
    /// None when either lacks the card, or its lines are not one run of the file's (it straddles a
    /// marker), so an answer never moves a marker or the other side.
    pub fn with_card(&self, edited: &str, slug: &str) -> Option<String> {
        let ours: Vec<&str> = self.ours.iter().map(|&i| self.lines[i].as_str()).collect();
        let before = parse_decisions(&ours)
            .into_iter()
            .find(|c| c.slug == slug)?;
        let edited = splitlines(edited);
        let after = parse_decisions(&edited)
            .into_iter()
            .find(|c| c.slug == slug)?;
        let from = before.line - 1;
        let at = self.ours.get(from..from + 1 + before.raw.len())?;
        if at.windows(2).any(|pair| pair[1] != pair[0] + 1) {
            return None;
        }
        let block = edited.get(after.line - 1..after.line + after.raw.len())?;
        let (first, last) = (*at.first()?, *at.last()?);
        let text: String = self.lines[..first]
            .iter()
            .map(String::as_str)
            .chain(block.iter().copied())
            .chain(self.lines[last + 1..].iter().map(String::as_str))
            .map(|line| format!("{line}\n"))
            .collect();
        Conflict::parse(&text).map(|_| text)
    }
}

/// Git's answer to `args` in the checkout at `root`.
fn git(root: &Path, args: &[&str]) -> Option<String> {
    crate::branches::git(root, args)
}

/// Whether git holds the project-relative `file` of the checkout at `root` unmerged: a merge,
/// rebase or cherry-pick stopped on it.
pub fn unmerged(root: &Path, file: &str) -> bool {
    git(root, &["ls-files", "--unmerged", "--", file]).is_some_and(|out| !out.trim().is_empty())
}

/// The text of the project-relative `file` in the checkout at `root`'s HEAD commit, none when HEAD
/// has no such file.
pub fn head_text(root: &Path, file: &str) -> Option<String> {
    git(root, &["cat-file", "blob", &format!("HEAD:./{file}")])
}

/// The conflicts the text `text` of the project-relative `file` holds while a merge stopped on it
/// in the checkout at `root`, none otherwise: a document that only shows conflict markers is not
/// one (git is asked only when the text holds them).
pub fn conflict(root: &Path, file: &str, text: &str) -> Option<Conflict> {
    Conflict::parse(text).filter(|_| unmerged(root, file))
}

/// The text a branch's page shows of `file` instead of `text`, its worktree's, while a merge
/// stopped on it ([`conflict`]): its last committed copy, or the branch's side when HEAD has none.
pub fn committed(root: &Path, file: &str, text: &str) -> Option<String> {
    conflict(root, file, text)
        .map(|conflict| head_text(root, file).unwrap_or_else(|| conflict.ours()))
}

/// `text`, the worktree's file `file`, as the branch has it: the branch's side of each conflict
/// while a merge stopped on it ([`conflict`]).
pub fn branch_side<'a>(root: &Path, file: &str, text: &'a str) -> Cow<'a, str> {
    match conflict(root, file, text) {
        Some(conflict) => Cow::Owned(conflict.ours()),
        None => Cow::Borrowed(text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MERGING: &str = "# Decisions\n\n- [ ] **outside** — Out.\n<<<<<<< HEAD\n- [ ] **settled** — Ours.\n  [2026-11-11 10:00] agent — Branch.\n=======\n- [r] **settled** — Theirs.\n  [2026-11-11 11:00] agent · resolved\n>>>>>>> main\n- [ ] **after** — After.\n";

    #[test]
    fn markers_are_seven_marks_alone_or_before_a_label() {
        assert!(marker("<<<<<<< HEAD", '<'));
        assert!(marker(">>>>>>>", '>'));
        assert!(!marker("<<<<<<<< eight", '<'));
        assert!(!marker("<<<<<< six", '<'));
        assert!(!marker("|||||||x", '|'));
    }

    #[test]
    fn the_branchs_side_is_every_line_outside_a_conflict_and_each_conflicts_first_side() {
        let conflict = Conflict::parse(MERGING).unwrap();
        assert_eq!(
            conflict.ours(),
            "# Decisions\n\n- [ ] **outside** — Out.\n- [ ] **settled** — Ours.\n  [2026-11-11 10:00] agent — Branch.\n- [ ] **after** — After.\n"
        );
        let diff3 = MERGING.replace(
            "=======",
            "||||||| base\n- [ ] **settled** — Base.\n=======",
        );
        assert_eq!(Conflict::parse(&diff3).unwrap().ours(), conflict.ours());
    }

    #[test]
    fn only_whole_conflicts_count() {
        assert_eq!(
            Conflict::parse("# Title\n=======\nText.\n"),
            None,
            "a heading's underline"
        );
        assert_eq!(
            Conflict::parse("<<<<<<< HEAD\nours\n=======\ntheirs\n"),
            None
        );
        assert_eq!(Conflict::parse("<<<<<<< HEAD\nours\n>>>>>>> main\n"), None);
        assert_eq!(Conflict::parse("No markers.\n"), None);
    }

    #[test]
    fn an_answer_lands_on_the_cards_lines_in_or_out_of_a_conflict() {
        let conflict = Conflict::parse(MERGING).unwrap();
        let asked = "  [2026-11-11 12:00] owner · question — Why?";
        let edited = conflict.ours().replace(
            "  [2026-11-11 10:00] agent — Branch.\n",
            &format!("  [2026-11-11 10:00] agent — Branch.\n{asked}\n"),
        );
        let edited = edited.replace("- [ ] **settled**", "- [?] **settled**");
        let saved = conflict.with_card(&edited, "settled").unwrap();
        assert_eq!(
            saved,
            MERGING
                .replace("- [ ] **settled**", "- [?] **settled**")
                .replace("Branch.\n=======", &format!("Branch.\n{asked}\n=======")),
            "on the branch's side, the other side and the markers as they were"
        );
        let outside = conflict
            .ours()
            .replace("- [ ] **after**", "- [x] **after**");
        let saved = conflict.with_card(&outside, "after").unwrap();
        assert_eq!(saved, MERGING.replace("- [ ] **after**", "- [x] **after**"));
        assert_eq!(conflict.with_card(&outside, "missing"), None);
    }

    #[test]
    fn a_card_that_straddles_a_marker_is_not_changed() {
        let straddling = "<<<<<<< HEAD\n- [ ] **c** — Ours.\n=======\n- [ ] **d** — Theirs.\n>>>>>>> main\n  [2026-11-11 10:00] agent — Under both.\n";
        let conflict = Conflict::parse(straddling).unwrap();
        let ours = conflict.ours();
        assert_eq!(
            ours,
            "- [ ] **c** — Ours.\n  [2026-11-11 10:00] agent — Under both.\n"
        );
        let edited = ours.replace("- [ ] **c**", "- [?] **c**");
        assert_eq!(conflict.with_card(&edited, "c"), None);
    }
}
