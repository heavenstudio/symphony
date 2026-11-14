//! The owner's answers a worktree holds that no commit has ([`answers`]): the owner's dated lines
//! under a card that the Symphony saved into a worktree's document and nobody committed, which
//! removing the worktree, or landing its branch's commits, would leave behind.
//!
//! A card and its lines are read as the Symphony reads them (`symphony_docs::syntax`), and a line
//! counts only where no copy of its card in the commits it is compared with holds it: so a line a
//! regroup only moved (within the file, or to a card in another changed file) is no answer, and an
//! agent's own note is none either.

use super::command::{blobs, told};
use std::collections::BTreeMap;
use std::path::Path;
use symphony_docs::syntax::{cards, entries, lines};

/// What marks a file that may hold an owner's dated line: `[2026-10-06 10:00] owner · approved — …`.
const OWNER: &str = "] owner";

/// An owner's line no commit has: the file holding it, project-relative, and the card it is under.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answer {
    pub file: String,
    pub card: String,
}

/// One owner's dated line under a card: the card's slug and the line as the register reads it (its
/// stamp, verdict and words, the lines that continue it among them).
type Line = (String, String, Option<String>, String);

/// The owner's dated lines of `text`, by card, in the file's order.
fn owner_lines(text: &str) -> Vec<Line> {
    if !text.contains(OWNER) {
        return Vec::new();
    }
    cards(&lines(text))
        .into_iter()
        .flat_map(|card| {
            entries(&card.raw)
                .into_iter()
                .filter_map(move |entry| {
                    let ts = entry.ts.filter(|_| entry.who == "owner")?;
                    Some((card.slug.clone(), ts, entry.verdict, entry.text))
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The pathspecs of the project's document folders ([`crate::project::documents`]): "" the root.
fn pathspecs(folders: &[String]) -> Vec<&str> {
    folders
        .iter()
        .map(|folder| if folder.is_empty() { "." } else { folder })
        .collect()
}

/// Whether the project-relative path `name` is in one of the document folders `folders`.
pub fn under(name: &str, folders: &[String]) -> bool {
    folders.iter().any(|folder| {
        folder.is_empty()
            || name
                .strip_prefix(folder.as_str())
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    })
}

/// The names in git's NUL-separated `out`.
fn names(out: &[u8]) -> Vec<String> {
    out.split(|b| *b == 0)
        .filter(|name| !name.is_empty())
        .map(|name| String::from_utf8_lossy(name).into_owned())
        .collect()
}

/// The owner's answers the worktree at `path` holds under the project's document folders
/// `folders` that no commit has: each owner's dated line, under a card, of a document its files
/// changed or added since its HEAD that none of HEAD's copies of those documents holds on that
/// card, nor, given a commit `tip`, the tip's copies (a land's: what it carries). Lines are
/// compared as a count per card, so a line a regroup only moved is none. An answer git cannot read
/// is an error, never none: a caller that cannot tell must keep what it would lose.
pub fn answers(path: &Path, folders: &[String], tip: Option<&str>) -> Result<Vec<Answer>, String> {
    let specs = pathspecs(folders);
    // Both names of a file moved: its old name's copy in HEAD holds the lines it moved.
    let mut diff = vec!["diff", "--name-only", "--no-renames", "-z", "HEAD", "--"];
    diff.extend(&specs);
    let mut untracked = vec!["ls-files", "-z", "--others", "--exclude-standard", "--"];
    untracked.extend(&specs);
    let changed = names(&told(path, &diff)?.stdout);
    let added = names(&told(path, &untracked)?.stdout);
    let mut written: Vec<(String, Line)> = Vec::new();
    for file in changed.iter().chain(&added) {
        let at = path.join(file);
        if at.is_dir() {
            continue; // a submodule or a nested checkout: no document
        }
        let text = match std::fs::read(&at) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(format!("{} could not be read: {error}", at.display())),
        };
        written.extend(owner_lines(&text).into_iter().map(|l| (file.clone(), l)));
    }
    if written.is_empty() {
        return Ok(Vec::new());
    }
    // What the commits compared with hold on each card, wherever in those documents it is.
    let revs: Vec<&str> = std::iter::once("HEAD").chain(tip).collect();
    let mut objects = Vec::new();
    for file in &changed {
        if file.contains('\n') {
            return Err(format!("{file:?} cannot be named to git cat-file"));
        }
        objects.extend(revs.iter().map(|rev| format!("{rev}:{file}")));
    }
    // How many copies of each line one commit holds, the most of either: HEAD's and the tip's are
    // two versions of the same documents, never two lines.
    let mut counts: Vec<BTreeMap<Line, usize>> = vec![BTreeMap::new(); revs.len()];
    for (at, text) in blobs(path, &objects)?.into_iter().enumerate() {
        for line in text.as_deref().map(owner_lines).unwrap_or_default() {
            *counts[at % revs.len()].entry(line).or_default() += 1;
        }
    }
    let mut held: BTreeMap<&Line, usize> = BTreeMap::new();
    for (line, n) in counts.iter().flatten() {
        let most = held.entry(line).or_default();
        *most = (*most).max(*n);
    }
    let mut left: Vec<Answer> = Vec::new();
    for (file, line) in &written {
        if let Some(n) = held.get_mut(line).filter(|n| **n > 0) {
            *n -= 1;
            continue;
        }
        let answer = Answer {
            file: file.clone(),
            card: line.0.clone(),
        };
        if !left.contains(&answer) {
            left.push(answer);
        }
    }
    Ok(left)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_owners_lines_are_their_dated_lines_under_a_card() {
        let text = "# A\n\n[2026-10-06 10:00] owner · approved — outside any card\n\n\
                    - [x] **lamp** — Lamp.\n  [2026-10-06 09:00] agent — Posted.\n  \
                    [2026-10-06 10:00] owner · approved — warm\n      and bright\n  \
                    an owner's undated note\n  ↳ an agent's answer\n\
                    - [ ] **fan** — Fan.\n  [2026-10-07] owner — Why?\n\
                    ```\n- [ ] **code** — C.\n  [2026-10-08 08:00] owner — in a fence\n```\n";
        let line = |slug: &str, ts: &str, verdict: Option<&str>, words: &str| {
            (
                slug.to_string(),
                ts.to_string(),
                verdict.map(str::to_string),
                words.to_string(),
            )
        };
        assert_eq!(
            owner_lines(text),
            [
                line(
                    "lamp",
                    "2026-10-06 10:00",
                    Some("approved"),
                    "warm\nand bright"
                ),
                line("fan", "2026-10-07", None, "Why?"),
            ]
        );
        assert!(owner_lines("- [ ] **x** — X.\n  [2026-10-06 09:00] agent — y\n").is_empty());
    }

    #[test]
    fn a_path_is_under_a_folder_by_its_parts_and_under_the_root_always() {
        let folders = |names: &[&str]| names.iter().map(|n| n.to_string()).collect::<Vec<_>>();
        for (name, at, under_it) in [
            ("docs/A.md", &["docs"][..], true),
            ("docs", &["docs"][..], true),
            ("docs/deep/A.md", &["docs"][..], true),
            ("docsx/A.md", &["docs"][..], false),
            ("crates/x/docs/A.md", &["docs"][..], false),
            ("spec/A.md", &["docs", "spec"][..], true),
            ("anything", &[""][..], true),
            ("design/spec/A.md", &["design/spec"][..], true),
            ("design/A.md", &["design/spec"][..], false),
        ] {
            assert_eq!(under(name, &folders(at)), under_it, "{name} {at:?}");
        }
        assert_eq!(pathspecs(&folders(&["", "docs"])), [".", "docs"]);
    }

    /// `git ARGS` in `root`, which must succeed.
    fn git(root: &Path, args: &[&str]) {
        let done = std::process::Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(args)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(done.status.success(), "git {args:?}: {done:?}");
    }

    /// A scratch repository named `name` under the system's temporary folder, with `files`
    /// committed.
    fn repository(name: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
        let root =
            std::env::temp_dir().join(format!("symphony-owner-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        for (file, text) in files {
            write(&root, file, text);
        }
        git(&root, &["init", "-q"]);
        git(&root, &["add", "."]);
        git(&root, &["commit", "-qm", "init"]);
        root
    }

    fn write(root: &Path, file: &str, text: &str) {
        let at = root.join(file);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, text).unwrap();
    }

    #[test]
    fn a_batch_reads_each_blob_and_none_where_a_name_holds_no_text() {
        let root = repository("blobs", &[("docs/A.md", "a\n"), ("docs/B b.md", "b\n")]);
        let specs = [
            "HEAD:docs/A.md",
            "HEAD:docs/B b.md",
            "HEAD:docs",
            "HEAD:docs/none.md",
        ]
        .map(String::from);
        assert_eq!(
            blobs(&root, &specs).unwrap(),
            [Some("a\n".into()), Some("b\n".into()), None, None]
        );
        assert!(blobs(&root, &[]).unwrap().is_empty());
        let failed = told(&root, &["rev-parse", "--verify", "nowhere"]).unwrap_err();
        assert!(failed.starts_with("git rev-parse failed in "), "{failed}");
        let gone = root.join("gone");
        let unrun = told(&gone, &["status"]).unwrap_err();
        assert!(unrun.starts_with("git status could not run in "), "{unrun}");
        assert!(blobs(&gone, &specs).is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn an_answer_is_a_line_no_copy_holds_counted_per_copy_in_the_folders_read() {
        let register = "# D\n\n- [x] **x** — X.\n  [2026-11-14 01:52] owner · approved — go\n";
        let root = repository("answers", &[("docs/D.md", register)]);
        let docs = vec!["docs".to_string()];
        // The register moved to a file of its own: its old name is gone, its line is no answer.
        std::fs::remove_file(root.join("docs/D.md")).unwrap();
        write(&root, "docs/E.md", register);
        // A nested checkout under the folder holds no document; a note outside it is not read.
        let nested = root.join("docs/sub");
        std::fs::create_dir_all(&nested).unwrap();
        git(&nested, &["init", "-q"]);
        write(&nested, "S.md", register);
        let note = "- [ ] **y** — Y.\n  [2026-11-14 02:00] owner — why?\n";
        write(&root, "notes/N.md", note);
        assert_eq!(answers(&root, &docs, None).unwrap(), []);
        let answer = |file: &str, card: &str| Answer {
            file: file.into(),
            card: card.into(),
        };
        // The root's folder reads every file.
        assert_eq!(
            answers(&root, &[String::new()], None).unwrap(),
            [answer("notes/N.md", "y")]
        );
        // A second copy of the line is one no commit holds.
        write(
            &root,
            "docs/E.md",
            &format!("{register}  [2026-11-14 01:52] owner · approved — go\n"),
        );
        assert_eq!(
            answers(&root, &docs, None).unwrap(),
            [answer("docs/E.md", "x")]
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
}
