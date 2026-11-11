//! The Symphony opens a board only through [`super::connect_in`], to read: no other source of the
//! crates that link the board (this one and the server) opens one, or opens one to write. So no
//! build of theirs, a branch's command line or preview server among them, moves a board's schema
//! forward or makes a board: only the board's own command does.

use std::path::{Path, PathBuf};

/// What opens a board, or opens one to write, as the board crate spells it.
const OPENS: [&str; 10] = [
    "Opening::write",
    "Access::Write",
    "Store::open(",
    "Store::open_as(",
    "Store::opening(",
    "Store::with(",
    "Store::unchecked",
    "cli::connect(",
    "symphony_board::cli::main",
    "symphony_board::cli::process_main",
];

/// The one source that opens a board, and the openings it may make: by the opening it is handed,
/// which is `Opening::read()`.
const READER: &str = "taskview/connect.rs";
const READS: [&str; 2] = ["Store::open_as(", "Store::opening("];

/// Whether `path` is a test's source, which opens boards as it likes: a `tests.rs` or `*_tests.rs`,
/// or anything in a `tests` or `testing` folder or a `testing.rs`.
fn is_test(path: &Path) -> bool {
    let name = path.file_name().and_then(|name| name.to_str()).unwrap_or("");
    name == "tests.rs"
        || name == "testing.rs"
        || name.ends_with("_tests.rs")
        || path
            .components()
            .any(|part| matches!(part.as_os_str().to_str(), Some("tests" | "testing")))
}

/// Every Rust source in `dir`, under `root`, but the tests' (by its path from `root`).
fn sources(root: &Path, dir: &Path, found: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            sources(root, &path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs")
            && !is_test(path.strip_prefix(root).unwrap())
        {
            found.push(path);
        }
    }
}

/// Each opening `text` makes: its line (from 1) and how it is spelled. A name that only ends in one
/// (`AskStore::open(`) is not one.
fn opens(text: &str) -> Vec<(usize, &'static str)> {
    let mut found = Vec::new();
    for (number, line) in text.lines().enumerate() {
        for word in OPENS {
            for (at, _) in line.match_indices(word) {
                let named = line[..at]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_alphanumeric() || c == '_');
                if !named {
                    found.push((number + 1, word));
                }
            }
        }
    }
    found
}

#[test]
fn the_symphony_opens_a_board_only_through_its_reader() {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut found = Vec::new();
    for root in [
        here.join("src"),
        here.join("../../apps/symphony-server/src"),
    ] {
        assert!(root.is_dir(), "{}", root.display());
        sources(&root, &root, &mut found);
    }
    assert!(found.len() > 50, "{found:?}");
    let mut opened = Vec::new();
    for file in &found {
        let text = std::fs::read_to_string(file).unwrap();
        for (line, word) in opens(&text) {
            if !(file.ends_with(READER) && READS.contains(&word)) {
                opened.push(format!("{}:{line}: {word}", file.display()));
            }
        }
    }
    assert!(
        opened.is_empty(),
        "the Symphony opens a board only through taskview::connect_in, to read; a test opens \
         one from a `*_tests.rs`: {opened:#?}"
    );
    let reader = std::fs::read_to_string(here.join("src").join(READER)).unwrap();
    assert_eq!(opens(&reader).len(), 2, "{reader}");
    assert!(reader.contains("Opening::read()"), "{reader}");
}

#[test]
fn every_spelling_of_an_opening_is_found_and_a_name_ending_in_one_is_not() {
    for (text, word) in [
        ("let s = symphony_board::Store::open(client);", "Store::open("),
        ("Store::with(client, clock, probe, host)", "Store::with("),
        ("&symphony_board::Opening::write().of(None)", "Opening::write"),
        ("Opening { access: Access::Write, ..how }", "Access::Write"),
        ("symphony_board::cli::connect(&env)", "cli::connect("),
        ("Store::unchecked_as(c, clock, probe, host, how)", "Store::unchecked"),
        ("symphony_board::cli::main(&args, &env, i, o, e)", "symphony_board::cli::main"),
    ] {
        assert_eq!(opens(text), [(1, word)], "{text}");
    }
    for text in [
        "AskStore::open(&db)",
        "crate::cli::main(args)",
        "symphony::cli::main(&args, input, out, err)",
        "symphony_board::Opening::read().of(project)",
    ] {
        assert_eq!(opens(text), [], "{text}");
    }
    assert_eq!(
        opens("one\nStore::open(a); Store::open(b)"),
        [(2, "Store::open("), (2, "Store::open(")]
    );
    assert!(is_test(Path::new("src/live/boards_tests.rs")));
    assert!(is_test(Path::new("src/reload/tests.rs")));
    assert!(is_test(Path::new("src/testing.rs")));
    assert!(!is_test(Path::new("src/taskview/connect.rs")));
}
