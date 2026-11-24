//! The owner's answers a worktree holds that no commit has ([`answers`]): a person's dated lines
//! under a card that the Symphony saved into a worktree's document and nobody committed, which
//! removing the worktree, or landing its branch's commits, would leave behind. A person's line is
//! any line not an agent's (docs/product/analysis/users-and-identity.md, "How a person's name is
//! written"): `owner ·` and a named person's `ana ·` alike.
//!
//! A card and its lines are read as the Symphony reads them (`symphony_docs::syntax`), and a line
//! counts only where no copy of its card in the commits it is compared with holds it: so a line a
//! regroup only moved or copied (within the file, or from any committed register) is no answer,
//! and an agent's own note is none either. A card's slug is its identity: renaming a card carrying
//! owner lines needs those lines committed under the new slug before a land. Git's text grep
//! selects files holding a dated line ([`DATED`]) before any file or blob is materialized here, so
//! unrelated text and binaries are never loaded into the register parser, even at the root.

use super::command::{blobs, grep, told};
use std::collections::BTreeMap;
use std::path::Path;
use symphony_docs::syntax::{Said, cards, lines, said, said_line};

/// What marks a file that may hold a person's dated line, as git's extended pattern: an indented
/// stamp and a lowercase name after it, `  [2026-10-06 10:00] owner · approved — …` or `ana ·`.
/// An agent's line matches too, and the register's reader tells them apart.
const DATED: &str =
    r"^[[:space:]]+\[[0-9]{4}-[0-9]{2}-[0-9]{2}( [0-9]{2}:[0-9]{2})?\][[:space:]]+[a-z]";

/// An owner's line no commit has: the file holding it, project-relative, and the card it is under.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answer {
    pub file: String,
    pub card: String,
}

/// One person's dated line under a card: the card's slug and the line as the register reads it
/// (the name it carries, its stamp, verdict and words, the lines that continue it among them).
type Line = (String, String, String, Option<String>, String);

/// The people's dated lines of `text`, by card, in the file's order.
fn owner_lines(text: &str) -> Vec<Line> {
    let persons = |line: &str| said_line(line).is_some_and(|said| said.entry.who == "owner");
    if !text.lines().any(persons) {
        return Vec::new();
    }
    cards(&lines(text))
        .into_iter()
        .flat_map(|card| {
            said(&card.raw)
                .into_iter()
                .filter_map(move |Said { entry, name }| {
                    let ts = entry.ts.filter(|_| entry.who == "owner")?;
                    Some((card.slug.clone(), name, ts, entry.verdict, entry.text))
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The pathspecs of the project's document folders ([`crate::project::documents`]): "" the root.
pub(crate) fn pathspecs(folders: &[String]) -> Vec<&str> {
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

/// Text files holding a dated line ([`DATED`]), in the worktree (including untracked files) or `rev`.
/// Git prints only names, excluding binary files; exit 1 means no matches, not a failed read.
fn candidates(path: &Path, specs: &[&str], rev: Option<&str>) -> Result<Vec<String>, String> {
    let mut args = vec!["grep", "-I", "-l", "-z", "-E", "-e", DATED];
    if let Some(rev) = rev {
        args.push(rev);
    } else {
        args.push("--untracked");
    }
    args.push("--");
    args.extend(specs);
    let found = names(&grep(path, &args)?.stdout);
    Ok(found
        .into_iter()
        .map(|name| match rev {
            Some(rev) => name
                .strip_prefix(&format!("{rev}:"))
                .unwrap_or(&name)
                .to_string(),
            None => name,
        })
        .collect())
}

/// The owner's answers the worktree at `path` holds under the project's document folders
/// `folders` that no commit has: each owner's dated line, under a card, of a document its files
/// changed or added since its HEAD that none of HEAD's copies of those documents holds on that
/// card, nor, given a commit `tip`, the tip's copies (a land's: what it carries). Every committed
/// register in the folders counts, including unchanged ones copied into a changed or new file.
/// Lines are compared as a count per card within each written file, so several files copying the
/// same committed register are none. An answer git cannot read is an error, never none: a caller
/// that cannot tell must keep what it would lose.
pub fn answers(path: &Path, folders: &[String], tip: Option<&str>) -> Result<Vec<Answer>, String> {
    let specs = pathspecs(folders);
    // Both names of a file moved: its old name's copy in HEAD holds the lines it moved.
    let mut diff = vec!["diff", "--name-only", "--no-renames", "-z", "HEAD", "--"];
    diff.extend(&specs);
    let mut untracked = vec!["ls-files", "-z", "--others", "--exclude-standard", "--"];
    untracked.extend(&specs);
    let changed = names(&told(path, &diff)?.stdout);
    let added = names(&told(path, &untracked)?.stdout);
    if changed.is_empty() && added.is_empty() {
        return Ok(Vec::new());
    }
    let changed_specs: Vec<String> = changed
        .iter()
        .chain(&added)
        .map(|file| format!(":(literal){file}"))
        .collect();
    let changed_specs: Vec<&str> = changed_specs.iter().map(String::as_str).collect();
    let candidates = candidates(path, &changed_specs, None)?;
    let mut written: Vec<(String, Line)> = Vec::new();
    for file in changed
        .iter()
        .chain(&added)
        .filter(|file| candidates.contains(file))
    {
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
    for (at, rev) in revs.iter().enumerate() {
        for file in self::candidates(path, &specs, Some(rev))? {
            if file.contains('\n') {
                return Err(format!("{file:?} cannot be named to git cat-file"));
            }
            objects.push((at, format!("{rev}:{file}")));
        }
    }
    // How many copies of each line one commit holds, the most of either: HEAD's and the tip's are
    // two versions of the same documents, never two lines.
    let mut counts: Vec<BTreeMap<Line, usize>> = vec![BTreeMap::new(); revs.len()];
    let specs: Vec<String> = objects.iter().map(|(_, spec)| spec.clone()).collect();
    for ((at, _), text) in objects.iter().zip(blobs(path, &specs)?) {
        for line in text.as_deref().map(owner_lines).unwrap_or_default() {
            *counts[*at].entry(line).or_default() += 1;
        }
    }
    let mut held: BTreeMap<&Line, usize> = BTreeMap::new();
    for (line, n) in counts.iter().flatten() {
        let most = held.entry(line).or_default();
        *most = (*most).max(*n);
    }
    let mut left: Vec<Answer> = Vec::new();
    // Written lines are grouped by file. Reuse the line-key count map within each group.
    let mut seen_file = None;
    let mut seen = BTreeMap::new();
    for (file, line) in &written {
        if seen_file != Some(file) {
            seen.clear();
            seen_file = Some(file);
        }
        let n = seen.entry(line).or_insert(0usize);
        *n += 1;
        if *n <= held.get(line).copied().unwrap_or(0) {
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
    use crate::children::Child as _;

    #[test]
    fn the_owners_lines_are_their_dated_lines_under_a_card() {
        let text = "# A\n\n[2026-10-06 10:00] owner · approved — outside any card\n\n\
                    - [x] **lamp** — Lamp.\n  [2026-10-06 09:00] agent — Posted.\n  \
                    [2026-10-06 10:00] owner · approved — warm\n      and bright\n  \
                    an owner's undated note\n  ↳ an agent's answer\n\
                    - [ ] **fan** — Fan.\n  [2026-10-07] owner — Why?\n  \
                    [2026-10-07 11:00] ana · question — and when?\n\
                    ```\n- [ ] **code** — C.\n  [2026-10-08 08:00] owner — in a fence\n```\n";
        let line = |slug: &str, name: &str, ts: &str, verdict: Option<&str>, words: &str| {
            (
                slug.to_string(),
                name.to_string(),
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
                    "owner",
                    "2026-10-06 10:00",
                    Some("approved"),
                    "warm\nand bright"
                ),
                line("fan", "owner", "2026-10-07", None, "Why?"),
                line(
                    "fan",
                    "ana",
                    "2026-10-07 11:00",
                    Some("question"),
                    "and when?"
                ),
            ]
        );
        assert!(owner_lines("- [ ] **x** — X.\n  [2026-10-06 09:00] agent — y\n").is_empty());
        // A named person's line alone marks the file, as an owner's does.
        assert_eq!(
            owner_lines("- [ ] **x** — X.\n  [2026-10-06 09:00] ana · approved — y\n"),
            [line("x", "ana", "2026-10-06 09:00", Some("approved"), "y")]
        );
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
        let done = std::process::Command::of("git")
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
        // A named person's line, the only one a file holds, is an answer as an owner's is; the
        // same words of another person's are no copy of it.
        write(&root, "docs/E.md", register);
        write(
            &root,
            "docs/P.md",
            "- [ ] **z** — Z.\n  [2026-11-14 03:00] ana · approved — go\n",
        );
        assert_eq!(
            answers(&root, &docs, None).unwrap(),
            [answer("docs/P.md", "z")]
        );
        write(
            &root,
            "docs/E.md",
            &format!("{register}  [2026-11-14 01:52] ana · approved — go\n"),
        );
        assert_eq!(
            answers(&root, &docs, None).unwrap(),
            [answer("docs/E.md", "x"), answer("docs/P.md", "z")]
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn unchanged_registers_hold_copies_in_backups_and_changed_files_but_not_renamed_cards() {
        let register = "- [x] **x** — X.\n  [2026-11-14 01:52] owner · approved — go\n";
        let root = repository("copies", &[("docs/D.md", register), ("docs/E.md", "# E\n")]);
        let docs = vec!["docs".to_string()];
        write(&root, "docs/D.md.orig", register);
        assert_eq!(answers(&root, &docs, None).unwrap(), []);
        write(&root, "docs/E.md", register);
        // Several copies of a known line still add no owner answer.
        assert_eq!(answers(&root, &docs, None).unwrap(), []);
        write(
            &root,
            "docs/E.md",
            &register.replace("**x**", "**renamed**"),
        );
        assert_eq!(
            answers(&root, &docs, None).unwrap(),
            [Answer {
                file: "docs/E.md".into(),
                card: "renamed".into(),
            }]
        );
        // The land's tip holds the renamed card in an otherwise unchanged register too.
        git(&root, &["add", "."]);
        git(&root, &["commit", "-qm", "rename"]);
        git(&root, &["branch", "tip"]);
        git(&root, &["checkout", "-q", "HEAD^"]);
        write(
            &root,
            "docs/copy.md",
            &register.replace("**x**", "**renamed**"),
        );
        assert_eq!(answers(&root, &docs, Some("tip")).unwrap(), []);
        assert!(answers(&root, &docs, Some("missing-tip")).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn binary_files_are_never_registers_even_when_the_whole_checkout_is_read() {
        let register = "- [x] **binary** — X.\n  [2026-11-14 01:52] owner · approved — go\n";
        let root = repository("binary", &[("docs/D.md", "# D\n")]);
        // Large binary assets can contain a register's bytes without being a document. A real
        // answer elsewhere makes the reader compare committed files as well as worktree files.
        let mut bytes = vec![0; 4 * 1024 * 1024];
        bytes[..register.len()].copy_from_slice(register.as_bytes());
        std::fs::write(root.join("asset.bin"), &bytes).unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-qm", "asset"]);
        std::fs::write(root.join("copy.bin"), bytes).unwrap();
        write(
            &root,
            "docs/D.md",
            &register.replace("**binary**", "**real**"),
        );
        assert_eq!(
            answers(&root, &[String::new()], None).unwrap(),
            [Answer {
                file: "docs/D.md".into(),
                card: "real".into(),
            }]
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_unreadable_changed_file_is_an_error_with_or_without_other_matches() {
        use std::os::unix::fs::PermissionsExt as _;
        let root = repository("unreadable", &[("docs/D.md", "# D\n")]);
        write(&root, "docs/hidden.md", "# Hidden\n");
        std::fs::set_permissions(
            root.join("docs/hidden.md"),
            std::fs::Permissions::from_mode(0o000),
        )
        .unwrap();
        for text in [
            "# Changed\n",
            "- [x] **x** — X.\n  [2026-11-14 01:52] owner · approved — go\n",
        ] {
            write(&root, "docs/D.md", text);
            let error = answers(&root, &["docs".into()], None).unwrap_err();
            assert!(
                error.starts_with("git grep could not read in ")
                    && error.contains("docs/hidden.md"),
                "{error}"
            );
        }
        std::fs::set_permissions(
            root.join("docs/hidden.md"),
            std::fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
