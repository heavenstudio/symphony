//! The Symphony opens a board only through [`super::connect_in`], to read: no other source of the
//! crates that link the board (this one and the server) opens one, or opens one to write. So no
//! build of theirs, a branch's command line or preview server among them, moves a board's schema
//! forward or makes a board: only the board's own command does.

use std::path::{Path, PathBuf};

/// What opens a board, or opens one to write, as the board crate spells it.
const OPENS: [&str; 14] = [
    "Opening::write",
    "Access::Write",
    "Store::open(",
    "Store::open_as(",
    "Store::opening(",
    "Store::opening_on(",
    "Store::read_as(",
    "Store::reading(",
    "Store::reading_on(",
    "Store::with(",
    "Store::unchecked",
    "cli::connect(",
    "symphony_board::cli::main",
    "symphony_board::cli::process_main",
];

/// The one source that opens a board, and the openings it may make: by the opening it is handed,
/// which is `Opening::read()`.
const READER: &str = "taskview/connect.rs";
const READS: [&str; 3] = ["Store::read_as(", "Store::reading(", "Store::reading_on("];

/// Whether `path` is a test's source, which opens boards as it likes: a `tests.rs` or `*_tests.rs`,
/// or anything in a `tests` or `testing` folder or a `testing.rs`.
fn is_test(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
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
        (
            "let s = symphony_board::Store::open(client);",
            "Store::open(",
        ),
        ("Store::with(client, clock, probe, host)", "Store::with("),
        (
            "Store::opening_on(db, clock, probe, host, how)",
            "Store::opening_on(",
        ),
        (
            "Store::reading_on(db, clock, probe, host, how)",
            "Store::reading_on(",
        ),
        (
            "&symphony_board::Opening::write().of(None)",
            "Opening::write",
        ),
        ("Opening { access: Access::Write, ..how }", "Access::Write"),
        ("symphony_board::cli::connect(&env)", "cli::connect("),
        (
            "Store::unchecked_on(db, clock, probe, host, how)",
            "Store::unchecked",
        ),
        (
            "symphony_board::cli::main(&args, &env, i, o, e)",
            "symphony_board::cli::main",
        ),
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

/// The branch scan's read of a board ([`super::connect_running`]) never starts the board's server:
/// with none running it is refused, and nothing is made in the repository's common dir.
#[test]
fn a_running_board_read_starts_no_server() {
    let tree = crate::test_tree::Tree::new();
    let done = symphony_board::sys::output(
        symphony_board::sys::Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(&tree.root),
    )
    .unwrap();
    assert!(done.status.success(), "{done:?}");
    let mut env = crate::children::board_env();
    for name in ["TASKS_SERVER", "TASKS_DATABASE"] {
        env.vars.remove(name);
    }
    let refused = super::open(&tree.root, env, symphony_board::cli::held_running_board)
        .err()
        .expect("no server runs");
    assert!(
        refused.starts_with("no server of the board in "),
        "{refused}"
    );
    assert!(!tree.path(".git/symphony/tasks").exists());
}

/// Every read of a held board, the one that starts a server and the branch scan's, is refused
/// naming the hold, and none starts the board's server.
#[test]
fn a_held_board_refuses_the_symphonys_reads_naming_the_hold() {
    let tree = crate::test_tree::Tree::new();
    let done = symphony_board::sys::output(
        symphony_board::sys::Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(&tree.root),
    )
    .unwrap();
    assert!(done.status.success(), "{done:?}");
    let mut env = crate::children::board_env();
    for name in ["TASKS_SERVER", "TASKS_DATABASE"] {
        env.vars.remove(name);
    }
    let home = symphony_board::server::Home::in_common(&tree.path(".git"));
    // No server runs, so the hold stops none and runs none of PostgreSQL's programs.
    let hold = home
        .postgresql(std::path::Path::new("/no/postgresql/bin"))
        .hold("russet-lynx", "a swap")
        .unwrap()
        .hold;
    let said = symphony_board::server::ServerError::Held(hold).to_string();
    let reaches: [(super::Reach, &str); 2] = [
        (symphony_board::cli::held_board, "board"),
        (symphony_board::cli::held_running_board, "running_board"),
    ];
    for (reach, named) in reaches {
        let refused = super::open(&tree.root, env.clone(), reach)
            .err()
            .expect("the board is held");
        assert_eq!(refused, said, "{named}");
    }
    for made in ["postgresql", "backend.json"] {
        assert!(
            !tree.path(".git/symphony/tasks").join(made).exists(),
            "{made}"
        );
    }
    assert!(home.release("russet-lynx", None).unwrap().is_some());
}

/// A folder this process serves is held to the repository it was served from: once its `.git`
/// goes, after whatever check its caller made, every reach of its board by its path, the one that
/// starts a server and the branch scan's, is refused as a folder that is gone, and none walks up to
/// the board of the checkout above it, whose folders are never made. That holds whether the folder
/// keeps a project file, where the board's lookup stops before it reads the repository, or not.
/// Served again, it is held to what the lookup finds then; a folder no repository holds is held to
/// nothing.
#[test]
fn a_served_folder_left_without_its_git_reaches_no_board_above_it() {
    let tree = crate::test_tree::Tree::checkout();
    let mut env = crate::children::board_env();
    for name in ["TASKS_SERVER", "TASKS_DATABASE"] {
        env.vars.remove(name);
    }
    let reaches: [(super::Reach, &str); 2] = [
        (symphony_board::cli::held_board, "board"),
        (symphony_board::cli::held_running_board, "running_board"),
    ];
    for (case, filed) in [("filed", true), ("unfiled", false)] {
        tree.write(
            &format!("artifacts/{case}/.git/HEAD"),
            "ref: refs/heads/main\n",
        );
        if filed {
            tree.write(
                &format!("artifacts/{case}/symphony.toml"),
                "[project]\nid = \"inner\"\n",
            );
        }
        let inner = tree.path(&format!("artifacts/{case}"));
        let own = inner.canonicalize().unwrap().join(".git");
        assert_eq!(super::serve(&inner), Some(own.clone()), "{case}");
        std::fs::remove_dir_all(inner.join(".git")).unwrap();
        for (reach, named) in reaches {
            let refused = super::open(&inner, env.clone(), reach)
                .err()
                .expect("the folder no longer holds its repository");
            assert_eq!(
                refused,
                format!(
                    "the project's folder {} no longer holds the repository it was served from, \
                     {}: {}",
                    inner.display(),
                    own.display(),
                    symphony_board::project::GONE
                ),
                "{case}, {named}"
            );
        }
        assert!(
            !tree.path(".git/symphony").exists(),
            "{case}: the outer checkout's board was looked for"
        );
        // Served again, the folder is held to the checkout that holds it now.
        assert_eq!(
            super::serve(&inner),
            Some(tree.root.canonicalize().unwrap().join(".git")),
            "{case}"
        );
    }
    let plain = crate::test_tree::Tree::new();
    assert_eq!(super::serve(&plain.root), None);
    assert_eq!(super::held(&plain.root), None);
}

/// `git init` in `folder`, made first.
fn git_init(folder: &Path) {
    std::fs::create_dir_all(folder).unwrap();
    let done = symphony_board::sys::output(
        symphony_board::sys::Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(folder),
    )
    .unwrap();
    assert!(done.status.success(), "{done:?}");
}

/// A folder this process serves keeps the journal of its project's chat and asks as it was found
/// when it began to be served: once its `.git` goes, the journal found from it again is the one of
/// the checkout that holds it, and every journal this process finds from the folder is its own
/// still; `ASKS_DB` names another before either. A folder this process does not serve finds its
/// journal now, as the command line does; one served again keeps the journal found then.
#[test]
fn a_served_folders_journal_is_the_one_found_when_it_began_to_be_served() {
    use crate::asks::{default_path, found_path};
    let tree = crate::test_tree::Tree::new();
    git_init(&tree.root);
    let inner = tree.path("artifacts/inner");
    git_init(&inner);
    let canonical = |path: &Path| path.canonicalize().unwrap();
    let own = canonical(&inner).join("artifacts/asks/asks.db");
    let outer = canonical(&tree.root).join("artifacts/asks/asks.db");
    assert_eq!(found_path(&inner), own);
    assert_eq!(super::served_journal(&inner), None, "not served yet");

    super::serve(&inner);
    std::fs::remove_dir_all(inner.join(".git")).unwrap();
    assert_eq!(
        found_path(&inner),
        outer,
        "found again, it is the outer checkout's"
    );
    assert_eq!(super::served_journal(&inner), Some(own.clone()));
    assert_eq!(default_path(&inner, None), own);
    assert_eq!(
        default_path(&inner, Some("")),
        own,
        "an empty ASKS_DB names none"
    );
    assert_eq!(
        default_path(&inner, Some("/x/y.db")),
        PathBuf::from("/x/y.db")
    );
    let unserved = tree.path("artifacts/other");
    std::fs::create_dir_all(&unserved).unwrap();
    assert_eq!(default_path(&unserved, None), outer);
    // Served again, the folder keeps the journal found then.
    super::serve(&inner);
    assert_eq!(default_path(&inner, None), outer);
}
