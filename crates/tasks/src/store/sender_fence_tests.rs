//! The line the store's one sender holds: only `store/transaction.rs` hands the board's
//! PostgreSQL a statement, `store/raw.rs` its rows' own and `store/database.rs` the making of a
//! database; every other module of the store asks through them, so what the database must answer
//! goes through one door.

use std::fs;
use std::path::{Path, PathBuf};

/// The modules that send, each for its reason.
const SENDERS: [(&str, &str); 3] = [
    ("transaction.rs", "the store's one sender of its statements"),
    (
        "raw.rs",
        "a raw row's own reads and writes, for the tests and the repairs",
    ),
    (
        "database.rs",
        "the board's database, made, and the client that sends to it",
    ),
];

/// How a line sends the database something past the store's sender: PostgreSQL's client asked
/// directly (`.simple(`, `.make_database(`), a step of a request built (`Step::bare(`,
/// `Step::with(`, which only the client's `request` takes), or a client's handle taken to send by
/// (`Postgres::Socket(`, `Postgres::Hosted(`).
const SENDS: [&str; 6] = [
    ".simple(",
    ".make_database(",
    "Step::bare(",
    "Step::with(",
    "Postgres::Socket(",
    "Postgres::Hosted(",
];

fn store() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src/store")
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

/// A file that is tests alone: their own statements are the tests' side of the line.
fn tests_alone(relative: &str) -> bool {
    relative.ends_with("tests.rs") || relative.contains("/tests/")
}

/// The lines of `text` that are the program's: no comment line, nothing from an inline test
/// module on (`#[cfg(test)]` over `mod NAME {`, which by this crate's habit ends a file), and not
/// the one line any other `#[cfg(test)]` stands over.
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

/// Whether `line` sends the database something ([`SENDS`]).
fn sends(line: &str) -> bool {
    SENDS.iter().any(|send| line.contains(send))
}

#[test]
fn the_store_sends_through_its_one_sender() {
    let root = store();
    let mut files = Vec::new();
    walk(&root, &mut files);
    assert!(
        files.len() > 50,
        "the store's sources are read: {}",
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
        let sender = SENDERS.iter().any(|(file, why)| {
            assert!(!why.is_empty(), "{file} says why it sends");
            relative == *file
        });
        if sender || tests_alone(&relative) {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap();
        for (at, line) in program(&text) {
            if sends(line) {
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

/// Each sender the fence names still sends, so that none is let through by a name gone stale.
#[test]
fn each_sender_the_fence_lets_through_sends() {
    for (file, _) in SENDERS {
        let text = fs::read_to_string(store().join(file)).unwrap();
        assert!(
            program(&text).iter().any(|(_, line)| sends(line)),
            "{file} sends nothing: take it out of the fence's senders"
        );
    }
}

#[test]
fn the_fence_reads_a_line_as_a_send_by_its_words_and_a_file_as_its_program_alone() {
    for line in [
        "    client.simple(&sql)?;",
        "            Database::PostgreSql(client) => client.make_database(),",
        "            pg::Step::with(sql, &vars)",
        "                Database::PostgreSql(client) => Postgres::Socket(client),",
    ] {
        assert!(sends(line), "{line}");
    }
    for line in [
        "        let read = self.request(lines, json!({})).map(Some);",
        "        let initialized = proxy.request(",
        "        self.read_together(Statement::from(pg.clone()), vars.clone())",
    ] {
        assert!(!sends(line), "{line}");
    }
    let text = "let a = 1; // client.simple(x)\n    // pg::Step::bare(q)\n/// .simple( \n\
                let b = \"Step::with(\";\n#[cfg(test)]\nmod tests;\nlet c = 3;\n\
                #[cfg(test)]\nmod tests {\n    const Q: &str = \".simple(\";\n}\nlet d = 4;\n";
    assert_eq!(
        program(text),
        [
            (1, "let a = 1; // client.simple(x)"),
            (4, "let b = \"Step::with(\";"),
            (7, "let c = 3;")
        ]
    );
    assert_eq!(program("#[cfg(test)]"), []);
    assert!(tests_alone("store/tests.rs") && tests_alone("callbacks/queue_tests.rs"));
    assert!(tests_alone("admission/tests/mod.rs") && !tests_alone("sessions.rs"));
}
