//! The line the seam holds: the board's code says nothing in SurrealQL outside this folder and
//! the few files that are the database's own, and sends the database nothing but through the
//! store's one sender.

use std::fs;
use std::path::{Path, PathBuf};

/// Words no line of Rust has by accident: each is how a statement, or a piece of one, reads.
const SURREALQL: [&str; 21] = [
    "type::record(",
    "record::exists(",
    "record::id(",
    "SELECT VALUE ",
    "SELECT * ",
    " FROM ONLY ",
    "UPSERT ",
    "LET $",
    "THROW ",
    "RETURN NONE",
    "RETURN SELECT",
    "RETURN {",
    " RETURN NONE;",
    "BEGIN TRANSACTION",
    "COMMIT TRANSACTION",
    "INFO FOR ",
    "DEFINE ",
    "array::len(",
    "array::distinct(",
    "string::starts_with(",
    "math::max(",
];

/// The files outside this folder that hold SurrealQL, each for its reason.
const OUTSIDE: [(&str, &str); 4] = [
    (
        "schema.rs",
        "the tables, indexes, events and migration steps are the database's own definition, as \
         are the guard a write starts with and the test of a write by a statement's words",
    ),
    (
        "rpc.rs",
        "the client names the namespace and database it defines and enters",
    ),
    (
        "rpc/read.rs",
        "the client tells a read from a write by a statement's words",
    ),
    (
        "server/reclaim.rs",
        "the keeper of the SurrealDB server asks the server what it holds, not the store",
    ),
];

fn sources() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|why| panic!("{}: {why}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            walk(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path);
        }
    }
}

/// A file that is tests alone: their own statements are the tests' side of the seam.
fn tests_alone(relative: &str) -> bool {
    relative.ends_with("tests.rs")
        || relative.contains("/tests/")
        || relative.ends_with("_scratch.rs")
        || relative.ends_with("unanswered_reads.rs")
}

/// The lines of `text` that are the program's: no comment line, nothing from an inline test
/// module on (`#[cfg(test)]` over `mod NAME {`, which by this crate's habit ends a file), and
/// not the one line any other `#[cfg(test)]` stands over.
fn program(text: &str) -> Vec<(usize, &str)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut kept = Vec::new();
    let mut at = 0;
    while at < lines.len() {
        let line = lines[at].trim();
        if line == "#[cfg(test)]" {
            let under = lines.get(at + 1).map_or("", |next| next.trim());
            if under.starts_with("mod ") && under.ends_with('{') {
                break;
            }
            at += 2;
            continue;
        }
        if !line.starts_with("//") {
            kept.push((at + 1, lines[at]));
        }
        at += 1;
    }
    kept
}

fn outside(relative: &str) -> bool {
    relative.starts_with("store/surreal/")
        || OUTSIDE.iter().any(|(file, why)| {
            assert!(!why.is_empty(), "{file} says why it is outside");
            relative == *file
        })
}

#[test]
fn no_surrealql_is_written_outside_the_seam() {
    let root = sources();
    let mut files = Vec::new();
    walk(&root, &mut files);
    assert!(
        files.len() > 100,
        "the crate's sources are read: {}",
        files.len()
    );
    let mut strays = Vec::new();
    for path in files {
        let relative = path
            .strip_prefix(&root)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        if outside(&relative) || tests_alone(&relative) {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap();
        for (at, line) in program(&text) {
            if let Some(word) = SURREALQL.iter().find(|word| line.contains(**word)) {
                strays.push(format!("src/{relative}:{at}: {word:?} in {}", line.trim()));
            }
        }
    }
    assert!(
        strays.is_empty(),
        "SurrealQL outside src/store/surreal/ (a statement belongs there, under a name the \
         store's code asks for):\n{}",
        strays.join("\n")
    );
}

/// Only `store/transaction.rs` hands a client a statement, SurrealDB's or PostgreSQL's
/// (`.simple(`), `store/raw.rs` its rows' own and `store/database.rs` the making of a database:
/// every other module of the store asks through it, so what each database must answer goes
/// through one door.
#[test]
fn the_store_sends_through_its_one_sender() {
    let root = sources().join("store");
    let mut files = Vec::new();
    walk(&root, &mut files);
    let mut strays = Vec::new();
    for path in files {
        let relative = path
            .strip_prefix(&root)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        if ["transaction.rs", "raw.rs", "database.rs"].contains(&relative.as_str())
            || tests_alone(&relative)
        {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap();
        for (at, line) in program(&text) {
            if [".query(", ".define_then(", ".call(", ".simple("]
                .iter()
                .any(|send| line.contains(send))
            {
                strays.push(format!("src/store/{relative}:{at}: {}", line.trim()));
            }
        }
    }
    assert!(
        strays.is_empty(),
        "a statement sent past store/transaction.rs:\n{}",
        strays.join("\n")
    );
}

#[test]
fn the_fence_reads_a_file_as_its_program_alone() {
    let text = "let a = 1; // SELECT VALUE x\n    // LET $x\n/// THROW \nlet b = \"LET $y\";\n\
                #[cfg(test)]\nmod tests;\nlet c = 3;\n\
                #[cfg(test)]\nmod tests {\n    const Q: &str = \"UPSERT \";\n}\nlet d = 4;\n";
    assert_eq!(
        program(text),
        [
            (1, "let a = 1; // SELECT VALUE x"),
            (4, "let b = \"LET $y\";"),
            (7, "let c = 3;")
        ]
    );
    assert_eq!(program("#[cfg(test)]"), []);
    assert!(tests_alone("store/tests.rs") && tests_alone("store/callbacks/queue_tests.rs"));
    assert!(tests_alone("land/tests/mod.rs") && !tests_alone("store/sessions.rs"));
    assert!(outside("store/surreal/sessions.rs") && outside("schema.rs"));
    assert!(outside("rpc/read.rs") && outside("server/reclaim.rs"));
    for inside in [
        "store/sessions.rs",
        "schema_other.rs",
        "cli/mod.rs",
        "server/export.rs",
    ] {
        assert!(!outside(inside), "{inside}");
    }
}
