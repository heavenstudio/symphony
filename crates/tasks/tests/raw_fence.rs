//! The tests' side of the seam: a test reads and sets a board's rows through `Store::raw`, and
//! only the files listed here still hand the client a statement of their own, each for its reason.
//!
//! A target of its own, which reads text and starts nothing, so `just lint-rs` runs it: a branch
//! that touches one board target's tests runs that target and the lint, and the fence judges the
//! files of every target, whichever one changed.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// How a test hands a client a statement: SurrealDB's client's two calls, the tests' retried
/// write, and a PostgreSQL client's statement or simple query.
const SENDS: [&str; 5] = [
    ".query(",
    ".define_then(",
    "RawWrite",
    "Step::bare(",
    ".simple(",
];

/// The engine is the test's subject: its catalogue, a plan, a definition made or removed, a
/// transaction thrown, a write paused at a gate, a database not made yet.
const ENGINE: &str = "the engine is its subject";
/// A statement of the library's own, or its board marker, run as the library wrote it.
const LIBRARY: &str = "runs the library's own statement";
/// Statements that feed each other or compute in the database, and so are one request.
const ONE_REQUEST: &str = "one request whose statements feed each other";
/// Rows of a table the test made for itself, no board's.
const SCRATCH: &str = "a scratch table of its own";
/// A row written as no fixture call writes it: its content replaced whole, a NULL, a vector.
const AS_WRITTEN: &str = "a row written as the fixture cannot";

/// Every test file that still sends a statement, by its path under the package.
const STILL_SENDS: [(&str, &str); 47] = [
    ("tests/cli/attach.rs", ENGINE),
    ("tests/cli/launch_rollback.rs", ENGINE),
    ("tests/cli/main_only/writers.rs", AS_WRITTEN),
    ("tests/cli/monitor_panes.rs", ONE_REQUEST),
    ("tests/cli/pg_lifecycle.rs", ENGINE),
    ("tests/cli/pull_unanswered.rs", ENGINE),
    ("tests/hosted/board.rs", ENGINE),
    ("tests/hosted/capacity.rs", ENGINE),
    ("tests/hosted/cluster.rs", ENGINE),
    ("tests/hosted/refusals.rs", ENGINE),
    ("tests/hosted/transactions.rs", ENGINE),
    ("tests/launch/reap.rs", AS_WRITTEN),
    ("tests/launch/reap_unread.rs", ENGINE),
    ("tests/perf/board.rs", LIBRARY),
    ("tests/perf/cleanup.rs", ENGINE),
    ("tests/perf/plans.rs", ENGINE),
    ("tests/perf/sessions.rs", LIBRARY),
    ("tests/perf/unanswered.rs", LIBRARY),
    ("tests/perf/words.rs", ENGINE),
    ("tests/server/postgres.rs", ENGINE),
    ("tests/store/asks.rs", ENGINE),
    ("tests/store/bindings.rs", ENGINE),
    ("tests/store/board.rs", ENGINE),
    ("tests/store/callbacks.rs", ENGINE),
    ("tests/store/catalogue.rs", ENGINE),
    ("tests/store/closing.rs", ENGINE),
    ("tests/store/common.rs", ENGINE),
    ("tests/store/compatible_ahead.rs", ENGINE),
    ("tests/store/copy.rs", ENGINE),
    ("tests/store/copy_marker.rs", ENGINE),
    ("tests/store/fields.rs", ENGINE),
    ("tests/store/held_stream.rs", SCRATCH),
    ("tests/store/marker_retirement.rs", ENGINE),
    ("tests/store/messages/inbox.rs", LIBRARY),
    ("tests/store/open_read.rs", ENGINE),
    ("tests/store/pg_client.rs", ENGINE),
    ("tests/store/quiet_pull.rs", ENGINE),
    ("tests/store/round_trip.rs", ENGINE),
    ("tests/store/schema.rs", ENGINE),
    ("tests/store/schema_copy.rs", ENGINE),
    ("tests/store/search.rs", ENGINE),
    ("tests/store/session_marker.rs", ENGINE),
    ("tests/store/slots/cost.rs", ENGINE),
    ("tests/store/slots/migration.rs", ENGINE),
    ("tests/store/start.rs", ENGINE),
    ("tests/store/usage.rs", ENGINE),
    ("tests/store/writers.rs", ENGINE),
];

fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|why| panic!("{}: {why}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .collect();
    entries.sort();
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
/// found, and a file that sends none, a file that is not Rust and this fence's own are not. The
/// package is a folder of this process's own, as `just lint-rs` and a board `just test-rs` in one
/// worktree run this target at once.
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
        ("tests/cli/deep/nested.rs", "db.query(\"INFO FOR DB\");"),
        ("tests/launch/define.rs", "db.define_then(text, next);"),
        ("tests/perf/retried.rs", "let write = RawWrite::new(text);"),
        ("tests/sweep/step.rs", "plan.push(Step::bare(text));"),
        ("tests/sweep/notes.txt", "client.simple(\"not Rust\");"),
        ("examples/demo.rs", "db.query(\"SELECT 1\");"),
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
            "tests/launch/define.rs",
            "tests/perf/retried.rs",
            "tests/server/new.rs",
            "tests/sweep/step.rs",
            "examples/demo.rs",
        ]
    );
    fs::remove_dir_all(&package).unwrap();
}
