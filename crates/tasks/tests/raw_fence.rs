//! The tests' side of the seam: a test reads and sets a board's rows through `Store::raw`, and
//! only the files listed here still hand the client a statement of their own, each for its reason.
//!
//! A target of its own, which reads text and starts nothing, so `just lint-rs` runs it: a branch
//! that touches one board target's tests runs that target and the lint, and the fence judges the
//! files of every target, whichever one changed.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// How a test hands a client a statement: a PostgreSQL client's statement, bare or bound, or its
/// simple query.
const SENDS: [&str; 3] = ["Step::bare(", "Step::with(", ".simple("];

/// The engine is the test's subject: its catalogue, a plan, a definition or trigger made or
/// removed, a transaction refused or tried again, a database not made yet or ended.
const ENGINE: &str = "the engine is its subject";
/// A statement of the library's own run again as the library wrote it.
const LIBRARY: &str = "runs the library's own statement";
/// A row written as no fixture call writes it: a value the board's own writes never hold.
const AS_WRITTEN: &str = "a row written as the fixture cannot";

/// Every test file that still sends a statement, by its path under the package.
const STILL_SENDS: [(&str, &str); 29] = [
    ("tests/cli/attach.rs", ENGINE),
    ("tests/cli/launch_rollback.rs", ENGINE),
    ("tests/cli/pg_lifecycle.rs", ENGINE),
    ("tests/cli/pull_unanswered.rs", AS_WRITTEN),
    ("tests/hosted/analyzer.rs", ENGINE),
    ("tests/hosted/board.rs", ENGINE),
    ("tests/hosted/capacity.rs", ENGINE),
    ("tests/hosted/cluster.rs", ENGINE),
    ("tests/hosted/refusals.rs", ENGINE),
    ("tests/hosted/transactions.rs", ENGINE),
    ("tests/launch/reap_unread.rs", AS_WRITTEN),
    ("tests/perf/board.rs", ENGINE),
    ("tests/perf/proxy.rs", LIBRARY),
    ("tests/perf/words.rs", ENGINE),
    ("tests/server/postgres.rs", ENGINE),
    ("tests/store/callbacks.rs", ENGINE),
    ("tests/store/catalogue.rs", ENGINE),
    ("tests/store/common.rs", ENGINE),
    ("tests/store/compatible_ahead.rs", ENGINE),
    ("tests/store/copy.rs", ENGINE),
    ("tests/store/ownership.rs", ENGINE),
    ("tests/store/pg_client.rs", ENGINE),
    ("tests/store/round_trip.rs", ENGINE),
    ("tests/store/schema.rs", ENGINE),
    ("tests/store/search.rs", ENGINE),
    ("tests/store/session_marker.rs", ENGINE),
    ("tests/store/usage.rs", ENGINE),
    ("tests/store/waits/source_retry.rs", ENGINE),
    ("tests/store/writers.rs", ENGINE),
];

fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|why| panic!("{}: {why}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .collect();
    symphony_board::order::sort_by(&mut entries, &Ord::cmp);
    for path in entries {
        if path.is_dir() {
            walk(&path, found);
        } else if path.extension().is_some_and(|e| e == "rs") {
            found.push(path);
        }
    }
}

/// The test files under `package` that hand the client a statement, by their path under it, this
/// fence aside.
fn senders(package: &Path) -> Vec<String> {
    let mut files = Vec::new();
    walk(&package.join("tests"), &mut files);
    walk(&package.join("examples"), &mut files);
    files
        .into_iter()
        .filter(|path| {
            let text = fs::read_to_string(path).unwrap();
            SENDS.iter().any(|send| text.contains(send))
        })
        .map(|path| {
            let under = path.strip_prefix(package).unwrap();
            under.to_string_lossy().replace('\\', "/")
        })
        .filter(|path| path != "tests/raw_fence.rs")
        .collect()
}

#[test]
fn only_the_listed_tests_send_a_statement_of_their_own() {
    let listed: BTreeMap<&str, &str> = STILL_SENDS.into_iter().collect();
    assert_eq!(listed.len(), STILL_SENDS.len(), "a file is listed twice");
    let senders = senders(Path::new(env!("CARGO_MANIFEST_DIR")));
    let unlisted: Vec<&String> = (senders.iter())
        .filter(|path| !listed.contains_key(path.as_str()))
        .collect();
    assert!(
        unlisted.is_empty(),
        "these tests hand the client a statement: ask `Store::raw` for the row instead, or list \
         the file here with why it cannot: {unlisted:?}"
    );
    let stale: Vec<&&str> = (listed.keys())
        .filter(|path| !senders.iter().any(|sender| sender == **path))
        .collect();
    assert!(
        stale.is_empty(),
        "these no longer send a statement, so leave the list: {stale:?}"
    );
}

/// The fence reads every folder of `tests/` and `examples/`, at any depth: a file of the server's
/// target that sends a statement is found though no store test changed, each way of sending is
/// found, and a file that sends none (a call of the SurrealDB client no board has among them), a
/// file that is not Rust and this fence's own are not. The package is a folder of this process's
/// own, as `just lint-rs` and a board `just test-rs` in one worktree run this target at once.
#[test]
fn a_sender_is_found_in_any_target_whichever_way_it_sends() {
    let package = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("raw_fence_package-{}", std::process::id()));
    if package.exists() {
        fs::remove_dir_all(&package).unwrap();
    }
    let files = [
        ("tests/server/new.rs", "client.simple(\"SELECT 1\");"),
        ("tests/store/quiet.rs", "store.raw().row(\"task\", id);"),
        (
            "tests/cli/deep/nested.rs",
            "client.request(&[Step::bare(\"SELECT 1\")]);",
        ),
        ("tests/launch/define.rs", "db.query(\"INFO FOR DB\");"),
        (
            "tests/perf/bound.rs",
            "client.request(&[Step::with(text, id)]);",
        ),
        ("tests/sweep/step.rs", "plan.push(Step::bare(text));"),
        ("tests/sweep/notes.txt", "client.simple(\"not Rust\");"),
        ("examples/demo.rs", "client.simple(\"SELECT 1\");"),
        ("tests/raw_fence.rs", "client.simple(\"this fence's own\");"),
    ];
    for (path, text) in files {
        let path = package.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    assert_eq!(
        senders(&package),
        [
            "tests/cli/deep/nested.rs",
            "tests/perf/bound.rs",
            "tests/server/new.rs",
            "tests/sweep/step.rs",
            "examples/demo.rs",
        ]
    );
    fs::remove_dir_all(&package).unwrap();
}
