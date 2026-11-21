//! Symphony names only its own things, never a consuming project's. Two checks over this tree's
//! tracked files:
//!
//! - The old state and tool names an older build carried (`sf-`, `sf_`, `sf.`, any case) are in no
//!   file but the fences below, each with why it stays, pinned by its count of matching lines, so a
//!   new old name anywhere fails here.
//! - The first consuming project's name is in no file's text or path: in any case, its two words
//!   joined by nothing, a space, `-`, `_` or `.`, or parted at a line's end (a comment's or a
//!   quote's marks before the second), and inside any identifier built from it. The one fence is
//!   the owner's own dated lines under a card ([`owner_lines`]), which no agent writes or rewrites.

use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

/// What an old name looks like, as `git grep -i -E` reads it.
const OLD: &str = "sf[-_.]";

/// This file, which names the old names to fence them.
const HERE: &str = "crates/symphony/tests/hooks/own_names.rs";

/// Each file that may still name an old name, how many of its lines do, and why.
const FENCES: [(&str, usize, &str); 1] = [(
    "docs/history/board-went-back-2026-11-12.md",
    1,
    "a past incident on the consuming project's board: the old folder it happened in",
)];

/// The consuming project's name, its two words each spelled backwards so that this file names it
/// nowhere.
const NAME_BACKWARDS: [&str; 2] = ["rats", "yrdnuof"];

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn git(root: &Path, args: &[&str]) -> Vec<u8> {
    let done = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .expect("git runs");
    // git grep answers 1 when no line matches, 2 or more when it fails.
    assert!(
        done.status.code().is_some_and(|code| code <= 1),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&done.stderr)
    );
    done.stdout
}

/// Each tracked text file of `root` with lines matching [`OLD`], and how many do, this file aside.
fn named(root: &Path) -> BTreeMap<String, usize> {
    let exclude = format!(":(exclude){HERE}");
    String::from_utf8(git(
        root,
        &["grep", "-I", "-c", "-i", "-E", OLD, "--", ".", &exclude],
    ))
    .unwrap()
    .lines()
    .map(|line| {
        let (file, count) = line.rsplit_once(':').expect("FILE:COUNT");
        (file.to_string(), count.parse().expect("a count"))
    })
    .collect()
}

/// What differs between the lines `found` and the fences allow: one line per file.
fn strays(found: &BTreeMap<String, usize>, fenced: &BTreeMap<&str, usize>) -> Vec<String> {
    let mut files: Vec<&str> = found.keys().map(String::as_str).collect();
    files.extend(fenced.keys().copied());
    files.sort_unstable();
    files.dedup();
    files
        .into_iter()
        .filter_map(|file| {
            let (lines, allowed) = (found.get(file).copied(), fenced.get(file).copied());
            match (lines, allowed) {
                (Some(lines), Some(allowed)) if lines == allowed => None,
                (Some(lines), Some(allowed)) => Some(format!(
                    "{file}: {lines} lines name an old name, its fence says {allowed}"
                )),
                (Some(lines), None) => Some(format!(
                    "{file}: {lines} lines name an old name, outside every fence"
                )),
                (None, _) => Some(format!("{file}: fenced, but names no old name any more")),
            }
        })
        .collect()
}

/// The consuming project's name, read forwards: its first word, then nothing, one of `-`, `_` or
/// `.`, or any run of spaces, line ends and the marks a comment or a quote starts its next line
/// with, then its second word, in any case and anywhere, a longer identifier's part too.
fn name() -> Regex {
    let [first, second] =
        NAME_BACKWARDS.map(|word| regex::escape(&word.chars().rev().collect::<String>()));
    Regex::new(&format!(r"(?i){first}(?:[-_.]|[\s/!#*>]*){second}")).unwrap()
}

/// Each place `text` names the name, as it is written there: its line (from 1) and the words.
fn in_text<'a>(name: &Regex, text: &'a str) -> Vec<(usize, &'a str)> {
    name.find_iter(text)
        .map(|hit| (1 + text[..hit.start()].matches('\n').count(), hit.as_str()))
        .collect()
}

/// The fence of the consuming project's name: the lines of `text` (from 1) that are the owner's own
/// words saved under a card. The owner's verdict or comment is saved as they said it and no agent
/// rewrites it, so the owner may name that project there, and the rule stays a rule for what agents
/// write.
///
/// Such a line is read as the card parser reads it, never by a pattern of this test's: a card
/// (`syntax::cards`) whose line is inside a register region (`register::regions`), under it a dated
/// line the parser says the owner wrote (`syntax::entry_line`) or a deeper-indented line continuing
/// one (`syntax::is_continuation`). An undated line under a card is judged although the parser reads
/// it as the owner's note: a card's own body is such lines, and agents write it. So is every line
/// outside a card, outside a region or in a document-level fence, however it reads.
fn owner_lines(text: &str) -> BTreeSet<usize> {
    use symphony_docs::syntax;
    let lines: Vec<&str> = text.split('\n').collect();
    let regions = symphony::register::regions(&lines);
    let mut found = BTreeSet::new();
    for card in syntax::cards(&lines) {
        // `card.line` is from 1, a region's bounds are indexes: the card's index is inside them.
        let at = card.line - 1;
        if !regions.iter().any(|(start, end)| *start < at && at < *end) {
            continue;
        }
        // Who wrote the dated entry the lines so far belong to, none after an undated line.
        let mut dated: Option<&str> = None;
        for (under, line) in card.raw.iter().enumerate() {
            if line.trim().is_empty() || syntax::unseen_owner(line).is_some() {
                continue;
            }
            if !(dated.is_some() && syntax::is_continuation(line)) {
                dated = syntax::entry_line(line).map(|entry| entry.who);
            }
            if dated == Some("owner") {
                found.insert(card.line + 1 + under);
            }
        }
    }
    found
}

/// Each place `text` names the name outside the owner's lines ([`owner_lines`]): a place is passed
/// only when every line it touches is the owner's, the blank lines inside it aside, so a name parted
/// between the owner's line and another is still found.
fn judged<'a>(name: &Regex, text: &'a str) -> Vec<(usize, &'a str)> {
    let hits = in_text(name, text);
    if hits.is_empty() {
        return hits;
    }
    let owners = owner_lines(text);
    let lines: Vec<&str> = text.split('\n').collect();
    hits.into_iter()
        .filter(|(line, words)| {
            !(*line..=*line + words.matches('\n').count())
                .all(|line| owners.contains(&line) || lines[line - 1].trim().is_empty())
        })
        .collect()
}

/// Each place in `root`'s tracked files that names the name, in the order of the paths:
/// `PATH: named WORDS` for a path, `PATH:LINE: WORDS` for a file's text, its line breaks shown as
/// `\n`. A folder, a file holding a NUL byte or one listed but gone has no text, and the owner's
/// lines under a card are not judged ([`owner_lines`]).
fn in_tree(root: &Path) -> Vec<String> {
    let name = name();
    let listed = git(root, &["ls-files", "-z", "--cached"]);
    let mut paths: Vec<String> = listed
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| String::from_utf8(path.to_vec()).expect("git lists paths in UTF-8"))
        .collect();
    paths.sort_unstable();
    let mut found = Vec::new();
    for path in paths {
        for (_, words) in in_text(&name, &path) {
            found.push(format!("{path}: named {words}"));
        }
        for (line, words) in judged(&name, &text(&root.join(&path))) {
            found.push(format!("{path}:{line}: {}", words.replace('\n', "\\n")));
        }
    }
    found
}

/// The text at `file`: none for a folder (or a link to one), a file holding a NUL byte or one gone
/// from the checkout; other bytes lossily.
fn text(file: &Path) -> String {
    if file.is_dir() {
        return String::new();
    }
    match std::fs::read(file) {
        Ok(bytes) if bytes.contains(&0) => String::new(),
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => panic!("{}: {error}", file.display()),
    }
}

#[test]
fn old_names_stay_inside_their_fences() {
    let root = workspace();
    assert!(root.join(HERE).is_file(), "{HERE} names this file");
    let fenced: BTreeMap<&str, usize> = FENCES
        .iter()
        .map(|(file, lines, _)| (*file, *lines))
        .collect();
    let strays = strays(&named(&root), &fenced);
    assert!(
        strays.is_empty(),
        "name Symphony's own thing instead, or fence the file here with why (and its count):\n{}",
        strays.join("\n")
    );
}

#[test]
fn every_fence_is_one_file_with_lines_and_a_reason() {
    let mut files: Vec<&str> = FENCES.iter().map(|(file, _, _)| *file).collect();
    files.sort_unstable();
    files.dedup();
    assert_eq!(files.len(), FENCES.len(), "one fence per file");
    for (file, lines, why) in FENCES {
        assert!(lines > 0, "{file}: a fence of no lines");
        assert!(!why.trim().is_empty(), "{file}: a fence without its reason");
    }
}

#[test]
fn a_stray_is_a_file_outside_the_fences_a_count_past_one_or_a_fence_left_empty() {
    let found: BTreeMap<String, usize> = [("a.rs", 2), ("b.md", 3), ("c.md", 1)]
        .map(|(file, lines)| (file.to_string(), lines))
        .into();
    let fenced: BTreeMap<&str, usize> = [("b.md", 3), ("c.md", 2), ("d.md", 4)].into();
    assert_eq!(
        strays(&found, &fenced),
        [
            "a.rs: 2 lines name an old name, outside every fence",
            "c.md: 1 lines name an old name, its fence says 2",
            "d.md: fenced, but names no old name any more",
        ]
    );
}

/// A scratch repository holding `files`, each added.
fn repository_of(name: &str, files: &[(String, Vec<u8>)]) -> PathBuf {
    let scratch = super::scratch(name);
    git(&scratch, &["init", "-q"]);
    for (file, bytes) in files {
        let path = scratch.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    git(&scratch, &["add", "."]);
    scratch
}

#[test]
fn the_pattern_finds_each_form_of_an_old_name_and_not_symphonys_own() {
    let files = [
        ("variable.rs", "SF_PORT\n"),
        ("folder.md", "the board in .git/sf-tasks\nSf_Tool\n"),
        ("setting.md", "git config sf.codexSteer\n"),
        (
            "own.md",
            "SYMPHONY_PORT in symphony/tasks, the namespace sf, symphony.board, a sf/x path\n",
        ),
    ]
    .map(|(file, text)| (file.to_string(), text.as_bytes().to_vec()));
    let mut files = files.to_vec();
    files.push(("binary.bin".to_string(), b"\0sf-tasks\0".to_vec()));
    let scratch = repository_of("own-names", &files);
    // A file git does not track is none of the tree's.
    std::fs::write(scratch.join("untracked.md"), "SF_PORT\n").unwrap();
    let counted: Vec<(String, usize)> = named(&scratch).into_iter().collect();
    assert_eq!(
        counted,
        [("folder.md", 2), ("setting.md", 1), ("variable.rs", 1)]
            .map(|(file, lines)| (file.to_string(), lines))
    );
    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
fn no_tracked_file_or_path_names_the_consuming_project() {
    let found = in_tree(&workspace());
    assert!(
        found.is_empty(),
        "the consuming project's name: say \"the consuming project\" instead:\n{}",
        found.join("\n")
    );
}

#[test]
fn the_name_is_found_in_any_case_and_spelling_inside_identifiers_and_across_a_line() {
    let [first, second] = NAME_BACKWARDS.map(|word| word.chars().rev().collect::<String>());
    let name = name();
    let title = |word: &str| format!("{}{}", word[..1].to_uppercase(), &word[1..]);
    let (one, two) = (title(&first), title(&second));
    let found = |text: &str| -> Vec<(usize, String)> {
        in_text(&name, text)
            .into_iter()
            .map(|(line, words)| (line, words.to_string()))
            .collect()
    };
    for (text, expected) in [
        // Each joining, in any case.
        (
            format!("the {one} {two} board"),
            vec![(1, format!("{one} {two}"))],
        ),
        (
            format!("{}-{}", first.to_uppercase(), second.to_uppercase()),
            vec![(
                1,
                format!("{}-{}", first.to_uppercase(), second.to_uppercase()),
            )],
        ),
        (
            format!("a {first}_{second}/x"),
            vec![(1, format!("{first}_{second}"))],
        ),
        (
            format!("com.{first}.{second}"),
            vec![(1, format!("{first}.{second}"))],
        ),
        // Inside an identifier built from it.
        (
            format!("{one}{two}Plugin and {first}_{second}_tasks"),
            vec![(1, format!("{one}{two}")), (1, format!("{first}_{second}"))],
        ),
        // Parted at a line's end, a comment's or a quote's marks before its second word.
        (
            format!("one\nthe {one}\n//! {two}'s manifest"),
            vec![(2, format!("{one}\n//! {two}"))],
        ),
        (
            format!("> {one}\n> {two}"),
            vec![(1, format!("{one}\n> {two}"))],
        ),
        // One word alone, or the two parted by another word, is not the name.
        (format!("a {first} and a {second}"), vec![]),
        (format!("{one}, {two}"), vec![]),
    ] {
        assert_eq!(found(&text), expected, "{text}");
    }

    // In a scratch tree: a path, a text file's line, and no binary or untracked file.
    let joined = format!("{first}{second}");
    let scratch = repository_of(
        "own-name",
        &[
            (format!("docs/{joined}-list.md"), b"nothing here\n".to_vec()),
            (
                "notes.md".to_string(),
                format!("one\nthe {one}\n{two} way\n").into_bytes(),
            ),
            ("blob.bin".to_string(), format!("\0{joined}\0").into_bytes()),
        ],
    );
    std::fs::write(scratch.join("untracked.md"), format!("{joined}\n")).unwrap();
    // A file the index lists and the checkout has removed has no text, but its path still counts.
    std::fs::remove_file(scratch.join(format!("docs/{joined}-list.md"))).unwrap();
    assert_eq!(
        in_tree(&scratch),
        [
            format!("docs/{joined}-list.md: named {joined}"),
            format!("notes.md:2: {one}\\n{two}"),
        ]
    );
    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
fn only_the_owners_dated_lines_under_a_card_in_a_register_may_name_the_consuming_project() {
    let [first, second] = NAME_BACKWARDS.map(|word| word.chars().rev().collect::<String>());
    let named = format!("{first} {second}");
    let document = [
        "# An analysis".to_string(), // 1
        String::new(),               // 2
        // An owner-looking line outside a card, and one under a card outside the register.
        format!("  [2026-11-21 09:39] owner · rejected — as in {named}"), // 3
        "- [ ] **outside** — A card that is prose.".to_string(),          // 4
        format!("  [2026-11-21 09:39] owner · rejected — as in {named}"), // 5
        String::new(),                                                    // 6
        "<!-- register:start -->".to_string(),                            // 7
        "### Topic".to_string(),                                          // 8
        String::new(),                                                    // 9
        format!("- [-] **card** — Like {named}?"),                        // 10
        format!("    The card's own words, undated: {named}."),           // 11
        format!("  [2026-11-21 09:00] agent — Posted, as {named} has it."), // 12
        format!("      [2026-11-21 09:01] owner · approved — {named}"),   // 13
        format!("  [2026-11-21 09:39] owner · rejected — Look at {named}."), // 14
        format!("      More of it, in {first}"),                          // 15
        String::new(),                                                    // 16
        format!("      {second}, and {first}_{second} again."),           // 17
        format!("  [2026-11-21] owner — a comment naming {named}, then {first}"), // 18
        format!("  {second}, an undated line an agent adds"),             // 19
        format!("  ↳ an undated answer on {named}"),                      // 20
        format!("  [2026-11-21 11:00] owner · approved — {first}"),       // 21
        String::new(),                                                    // 22
        format!("{second} after the card"),                               // 23
        "```".to_string(),                                                // 24
        "- [ ] **fenced** — An example, not a card.".to_string(),         // 25
        format!("  [2026-11-21 09:39] owner · rejected — {named}"),       // 26
        "```".to_string(),                                                // 27
        "<!-- register:end -->".to_string(),                              // 28
        "- [ ] **after** — Past the register.".to_string(),               // 29
        format!("  [2026-11-21 09:39] owner · rejected — {named}"),       // 30
        String::new(),
    ]
    .join("\n");
    // The owner's lines: each dated owner line under the register's card, with what continues it.
    assert_eq!(
        owner_lines(&document).into_iter().collect::<Vec<_>>(),
        [14, 15, 17, 18, 21]
    );
    let name = name();
    let refused: Vec<usize> = judged(&name, &document)
        .into_iter()
        .map(|(line, _)| line)
        .collect();
    // Passed: 14, 15 to 17 (parted inside the owner's entry), 17 and 18's first. Refused: every
    // line an agent could write, 13 (a dated-looking line that continues the agent's entry), and
    // the names parted from the owner's line into an undated one (18 to 19) or into prose (21 to 23).
    assert_eq!(refused, [3, 5, 10, 11, 12, 13, 18, 20, 21, 26, 30]);
    assert_eq!(in_text(&name, &document).len(), refused.len() + 4);

    // The same in a tree, and no fence for a file that is not a register.
    let scratch = repository_of(
        "own-name-owner-lines",
        &[
            ("docs/analysis.md".to_string(), document.into_bytes()),
            (
                "docs/owner.md".to_string(),
                format!(
                    "- [-] **card** — No register here.\n  [2026-11-21 09:39] owner — {named}\n"
                )
                .into_bytes(),
            ),
        ],
    );
    let found = in_tree(&scratch);
    assert_eq!(found.len(), refused.len() + 1, "{found:?}");
    assert_eq!(found[0], format!("docs/analysis.md:3: {named}"));
    assert_eq!(found[refused.len()], format!("docs/owner.md:2: {named}"));
    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
fn a_register_never_closed_or_a_text_without_one_has_no_owner_lines() {
    let entry = "- [-] **card** — A card.\n  [2026-11-21 09:39] owner · rejected — no\n";
    assert!(owner_lines("").is_empty());
    assert!(owner_lines(entry).is_empty());
    assert!(owner_lines(&format!("<!-- register:start -->\n{entry}")).is_empty());
    assert_eq!(
        owner_lines(&format!(
            "<!-- register:start -->\n{entry}<!-- register:end -->\n"
        ))
        .into_iter()
        .collect::<Vec<_>>(),
        [3]
    );
}
