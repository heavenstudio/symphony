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

/// A row of an open table ([`crate::schema::OPEN`]) may hold a field of an additive step this build
/// does not know, which its writes keep: no statement replaces one whole (`UPSERT` or `UPDATE` with
/// `CONTENT` or `REPLACE`), which would drop it. A `CREATE … CONTENT` makes a row, and drops
/// nothing; PostgreSQL's twins set named columns, and keep the rest.
#[test]
fn no_statement_replaces_a_row_of_an_open_table_whole() {
    let root = sources().join("store/surreal");
    let mut files = Vec::new();
    walk(&root, &mut files);
    assert!(files.len() > 30, "the statements are read: {}", files.len());
    let mut strays = Vec::new();
    for path in files {
        let relative = path
            .strip_prefix(&root)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        if tests_alone(&relative) {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = program(&text).into_iter().map(|(_, line)| line).collect();
        for (table, statement) in whole_row_writes(&lines.join("\n")) {
            strays.push(format!(
                "src/store/surreal/{relative}: {table}: {statement}"
            ));
        }
    }
    assert!(
        strays.is_empty(),
        "a row of an open table written whole (set its fields by name, as `UPSERT … SET`):\n{}",
        strays.join("\n")
    );
}

#[test]
fn a_whole_row_write_is_found_by_its_words_in_any_quoting_across_lines() {
    let found = |text: &str| -> Vec<&str> {
        whole_row_writes(text)
            .into_iter()
            .map(|(table, _)| table)
            .collect()
    };
    assert_eq!(
        found("FOR $r IN $add { UPSERT type::record('role', [$slug, $r.handle]) CONTENT $r; };"),
        ["role"]
    );
    assert_eq!(
        found("UPDATE task:x\n    REPLACE { slug: 'x' } RETURN NONE;"),
        ["task"]
    );
    assert_eq!(
        found("UPSERT `task_session`:[$s, $h] CONTENT $row;"),
        ["task_session"]
    );
    assert_eq!(
        found("upsert ⟨link⟩:x CONTENT {};"),
        Vec::<&str>::new(),
        "statements are upper case"
    );
    // A row made, a row set by its fields, and a table no build reads past are not.
    assert!(found("CREATE type::record('task', $slug) CONTENT { slug: $slug };").is_empty());
    assert!(found("UPSERT type::record('role', $id) SET since = $now RETURN NONE;").is_empty());
    assert!(found("UPSERT type::record('writer', [$host, $path]) CONTENT { a: 1 };").is_empty());
    assert!(found("UPSERT workflow:['active'] CONTENT { kind: 'active' };").is_empty());
    // Its end is its `;`: a later statement's CONTENT is not its.
    assert!(found("UPDATE task SET n = 1;\nCREATE task:y CONTENT {};").is_empty());
    // A word that only contains a table's name names no table.
    assert!(found("UPSERT tasks:x CONTENT {};\nUPSERT task_links:y CONTENT {};").is_empty());
    assert!(found("LAST_UPDATE role CONTENT;").is_empty());
}

/// Each statement of `text` that writes a row whole on an open table, with the table it names
/// before its `CONTENT` or `REPLACE`, and the statement on one line.
fn whole_row_writes(text: &str) -> Vec<(&'static str, String)> {
    let mut found = Vec::new();
    for head in ["UPSERT", "UPDATE"] {
        for start in whole_words(text, head) {
            let end = text[start..]
                .find(';')
                .map_or(text.len(), |end| start + end);
            let statement = &text[start..end];
            let body = ["CONTENT", "REPLACE"]
                .iter()
                .filter_map(|word| whole_words(statement, word).next())
                .min();
            let Some(body) = body else { continue };
            let named = |table: &&&str| {
                statement[..body]
                    .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .any(|word| word == **table)
            };
            if let Some(table) = crate::schema::OPEN.iter().find(named) {
                found.push((
                    *table,
                    statement.split_whitespace().collect::<Vec<_>>().join(" "),
                ));
            }
        }
    }
    found
}

/// Where `word` stands in `text` as a whole word.
fn whole_words<'a>(text: &'a str, word: &'a str) -> impl Iterator<Item = usize> + 'a {
    let part = |c: char| c.is_ascii_alphanumeric() || c == '_';
    text.match_indices(word)
        .map(|(at, _)| at)
        .filter(move |at| {
            !text[..*at].chars().next_back().is_some_and(part)
                && !text[at + word.len()..].chars().next().is_some_and(part)
        })
}
