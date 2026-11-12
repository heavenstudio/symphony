//! Symphony names only its own things, never a consuming project's. Two checks over this tree:
//!
//! - The old state and tool names an older build carried (`sf-`, `sf_`, any case) are only in the
//!   move that renames those folders into `symphony/` and in the refusal a build gives before it,
//!   each file a fence below with why it stays, pinned by its count of matching lines, so a new
//!   old name anywhere fails here.
//! - The words of the first consuming project (its name in every spelling, its game's words, its
//!   marks) are nowhere, in no file's text or path, as that project's own test of this tree
//!   reads them: whole words in any case, a word parted from the next by anything but an ASCII
//!   letter or digit, and the marks in their own case. That test also reads the names in its
//!   design data, which only it holds.

use regex::Regex;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// What an old name looks like, as `git grep -i -E` reads it.
const OLD: &str = "sf[-_]";

/// This file, which names the old names to fence them.
const HERE: &str = "crates/symphony/tests/hooks/own_names.rs";

/// Each file that may still name an old name, how many of its lines do, and why.
const FENCES: [(&str, usize, &str); 3] = [
    (
        "tools/move-state.sh",
        50,
        "the move command: the old folders it renames into symphony/",
    ),
    (
        "crates/tasks/tests/launcher/move_state.rs",
        116,
        "the move command's tests: the old folders it moves, as fixtures and expected output",
    ),
    (
        "crates/tasks/src/unmoved.rs",
        9,
        "the refusal a build gives before the move: the old folders it finds still holding state",
    ),
];

/// The first consuming project's words as its test lists them, each spelled backwards so that this
/// file names none of them: its game's words and its name in four spellings.
const WORDS_BACKWARDS: [&str; 28] = [
    "blg",
    "reweiv-ledom",
    "tlit",
    "tibro-aremac",
    "laogotaremacpmuj",
    "lluh",
    "slluh",
    "noitcaf",
    "snoitcaf",
    "noitcafbus",
    "snoitcafbus",
    "retsor",
    "sretsor",
    "pihs",
    "spihs",
    "tra",
    "stra",
    "yrdnuof rats",
    "yrdnuof-rats",
    "yrdnuof_rats",
    "yrdnuofrats",
    "cilbuper",
    "eripme",
    "noitilaoc",
    "lacigoloib",
    "lanoisnemid",
    "ked nug",
    "skced nug",
];

/// Its marks, as its test reads them, in their own case: a code of three capitals, a class and a
/// number, and a design card's field.
const MARKS: &str = r"\b[A-Z]{3}-(?:F|FG|FS|DD|DN|CR|CV|BB)[0-9]*\b|\*(?:Shows|Model|Golden):\*";

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

/// The consuming project's words and marks, read forwards and matched as its test matches them.
struct Vocabulary {
    /// Any word whole, in any case: after the line's start or a character that is no ASCII letter
    /// or digit, and before the line's end or another such character.
    words: Regex,
    marks: Regex,
}

impl Vocabulary {
    fn new() -> Vocabulary {
        let words: Vec<String> = WORDS_BACKWARDS
            .iter()
            .map(|word| regex::escape(&word.chars().rev().collect::<String>()))
            .collect();
        Vocabulary {
            words: Regex::new(&format!(
                r"(?i)(?:^|[^A-Za-z0-9])({})(?:$|[^A-Za-z0-9])",
                words.join("|")
            ))
            .unwrap(),
            marks: Regex::new(MARKS).unwrap(),
        }
    }

    /// Each word `text` names as it is written there, in order, then each mark.
    fn in_text<'a>(&self, text: &'a str) -> Vec<&'a str> {
        let mut found = Vec::new();
        let mut at = 0;
        // The character that ends one word may begin the next.
        while let Some(hit) = self.words.captures_at(text, at) {
            let word = hit.get(1).expect("the pattern's one group is the word");
            found.push(word.as_str());
            at = word.end();
        }
        found.extend(self.marks.find_iter(text).map(|mark| mark.as_str()));
        found
    }

    /// Each place in `root`'s tracked and untracked unignored files that names any of it, in
    /// the order of the paths: `PATH: named WORD` for a path, `PATH:LINE: WORD` for a line of a
    /// file's text. A folder, a file holding a NUL byte or one listed but gone has no text.
    fn in_tree(&self, root: &Path) -> Vec<String> {
        let listed = git(
            root,
            &[
                "ls-files",
                "-z",
                "--cached",
                "--others",
                "--exclude-standard",
            ],
        );
        let mut paths: Vec<String> = listed
            .split(|byte| *byte == 0)
            .filter(|path| !path.is_empty())
            .map(|path| String::from_utf8(path.to_vec()).expect("git lists paths in UTF-8"))
            .collect();
        paths.sort_unstable();
        let mut found = Vec::new();
        for path in paths {
            for word in self.in_text(&path) {
                found.push(format!("{path}: named {word}"));
            }
            for (index, line) in text(&root.join(&path)).lines().enumerate() {
                for word in self.in_text(line) {
                    found.push(format!("{path}:{}: {word}", index + 1));
                }
            }
        }
        found
    }
}

/// The text at `file`, read as the consuming project's test reads it: none for a folder (or a
/// link to one), a file holding a NUL byte or one gone from the checkout; other bytes lossily.
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
        (
            "own.md",
            "SYMPHONY_PORT in symphony/tasks, the namespace sf, symphony.board\n",
        ),
    ]
    .map(|(file, text)| (file.to_string(), text.as_bytes().to_vec()));
    let mut files = files.to_vec();
    files.push(("binary.bin".to_string(), b"\0sf-tasks\0".to_vec()));
    let scratch = repository_of("own-names", &files);
    let counted: Vec<(String, usize)> = named(&scratch).into_iter().collect();
    assert_eq!(
        counted,
        [("folder.md", 2), ("variable.rs", 1)].map(|(file, lines)| (file.to_string(), lines))
    );
    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
fn no_file_or_path_names_a_consuming_projects_words() {
    let found = Vocabulary::new().in_tree(&workspace());
    assert!(
        found.is_empty(),
        "a consuming project's words, which its own test of this tree refuses: say it in \
         Symphony's own words (\"the consuming project\", a neutral word):\n{}",
        found.join("\n")
    );
}

#[test]
fn a_word_is_found_whole_in_any_case_and_spelling_in_text_and_in_paths() {
    let forwards = |backwards: &str| backwards.chars().rev().collect::<String>();
    let (name, game, part) = (forwards("rats"), forwards("yrdnuof"), forwards("pihs"));
    let vocabulary = Vocabulary::new();
    let joined = format!("{name}{game}");
    let (code, field) = ("ABC", "Golden");
    let loud = format!("{}-{game}", name.to_uppercase());
    for (text, found) in [
        (
            format!("the {name} {game} board"),
            vec![format!("{name} {game}")],
        ),
        // As it is written, in each of the spellings the list holds.
        (loud.clone(), vec![loud.clone()]),
        (format!("{name}_{game}/x"), vec![format!("{name}_{game}")]),
        (format!("a {joined}."), vec![joined.clone()]),
        (
            format!("each {} here", part.to_uppercase()),
            vec![part.to_uppercase()],
        ),
        // One character parts two words, and it may end one and begin the other.
        (format!("{part},{part}"), vec![part.clone(), part.clone()]),
        // Within a longer word, or another word, it is no word of the vocabulary.
        (format!("{part}ped, {name}s, {game}ry"), vec![]),
        // A spelling the list does not hold is none of its words.
        (format!("{name}  {game}, {name}.{game}"), vec![]),
        // A mark, put together here so that this file holds none, found in its own case only.
        (
            format!("{code}-DD3 and *{field}:*"),
            vec![format!("{code}-DD3"), format!("*{field}:*")],
        ),
        (
            format!(
                "{code}-XX3, {}-dd3 and *{}:*",
                code.to_lowercase(),
                field.to_lowercase()
            ),
            vec![],
        ),
    ] {
        assert_eq!(vocabulary.in_text(&text), found, "{text}");
    }

    // In a scratch tree: a path, a text file's line, and no binary file.
    let scratch = repository_of(
        "own-words",
        &[
            (format!("docs/{part}-list.md"), b"nothing here\n".to_vec()),
            (
                "notes.md".to_string(),
                format!("one\nthe {joined} way\n").into_bytes(),
            ),
            ("blob.bin".to_string(), format!("\0{joined}\0").into_bytes()),
        ],
    );
    std::fs::write(scratch.join("untracked.md"), format!("{part}\n")).unwrap();
    // A file the index lists and the checkout has removed has no text, but its path still counts.
    std::fs::remove_file(scratch.join(format!("docs/{part}-list.md"))).unwrap();
    assert_eq!(
        vocabulary.in_tree(&scratch),
        [
            format!("docs/{part}-list.md: named {part}"),
            format!("notes.md:2: {joined}"),
            format!("untracked.md:1: {part}"),
        ]
    );
    let _ = std::fs::remove_dir_all(&scratch);
}
