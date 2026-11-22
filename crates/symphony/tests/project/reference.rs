//! docs/operations/PROJECT_FILE.md, the reference of `symphony.toml`, names every key the Symphony reads and
//! no other. The keys are the fields of the structs the file is read into, as their source writes
//! them: `Project::parse` refuses any other key by name, and the strict tables' (an entrypoint's, a
//! generator's, a machine's advice) refuse it where they are read, so those fields are the whole set.

use std::collections::BTreeSet;

use super::common::repo;

/// The reference, from the workspace's root.
pub(super) const REFERENCE: &str = "docs/operations/PROJECT_FILE.md";

/// The source of the struct the whole file is read into, whose fields are its tables, and of the
/// project's own tables'.
const FILE: &str = "crates/symphony/src/project/file.rs";
const SOURCES: [&str; 3] = [
    FILE,
    "crates/symphony/src/project/board.rs",
    "crates/symphony/src/project/theme.rs",
];

/// Each struct a table is read into: the source holding it, from the workspace's root, its name,
/// and the table as the reference's heading names it.
/// The hosted board's settings, which the board reads and judges itself.
const EXTERNAL: &str = "crates/tasks/src/pg/external.rs";

const TABLES: [(&str, &str, &str); 23] = [
    (
        "crates/symphony-api/src/workflow.rs",
        "Definition",
        "[workflow]",
    ),
    (
        "crates/tasks/src/workflow.rs",
        "Mapping",
        "[[workflow_mappings]]",
    ),
    (
        "crates/tasks/src/workflow/guard.rs",
        "Attester",
        "[[workflow_authorities]]",
    ),
    (FILE, "FileProject", "[project]"),
    (FILE, "FileEntrypoint", "[[entrypoints]]"),
    (FILE, "FilePlugins", "[plugins]"),
    (SOURCES[2], "FileTheme", "[theme]"),
    (FILE, "FileAgents", "[agents]"),
    (FILE, "FileLaunch", "[agents.launch]"),
    (FILE, "FileModels", "[agents.models]"),
    (FILE, "FileVerdicts", "[verdicts]"),
    (FILE, "FileLand", "[land]"),
    (
        "crates/tasks/src/land.rs",
        "Generator",
        "[[land.generators]]",
    ),
    ("crates/tasks/src/land.rs", "Upstream", "[land.upstream]"),
    (SOURCES[1], "FileBoard", "[board]"),
    (EXTERNAL, "Settings", "[board.external_postgresql]"),
    (EXTERNAL, "Budget", "[board.external_postgresql.budget]"),
    (
        EXTERNAL,
        "Class",
        "[board.external_postgresql.budget.classes.CLASS]",
    ),
    (SOURCES[1], "WipGuidance", "[board.machine_wip.MACHINE]"),
    (FILE, "FileReleases", "[releases]"),
    (FILE, "FileLimits", "[limits]"),
    (FILE, "FileStamps", "[stamps]"),
    ("crates/tasks/src/stamps.rs", "Written", "[stamps.NAME]"),
];

pub(super) fn read(file: &str) -> String {
    std::fs::read_to_string(repo().join(file)).unwrap_or_else(|e| panic!("{file}: {e}"))
}

/// The fields of the struct `name` as `source` writes it, one a line, but one a `flatten`
/// attribute marks (what keeps the keys this build does not know); `None` when `source` has no
/// such struct.
fn fields(source: &str, name: &str) -> Option<Vec<String>> {
    let open = format!("struct {name} {{");
    let mut lines = source.lines().skip_while(|line| !line.contains(&open));
    lines.next()?;
    let mut found = Vec::new();
    let mut flattened = false;
    for line in lines.map(str::trim).take_while(|line| *line != "}") {
        if line.starts_with("//") {
            continue;
        }
        if line.starts_with("#[") {
            flattened |= line.contains("flatten");
            continue;
        }
        let field = ["pub(super) ", "pub "]
            .iter()
            .find_map(|visibility| line.strip_prefix(visibility))
            .unwrap_or(line);
        let Some((key, _)) = field.split_once(':') else {
            continue;
        };
        if !std::mem::take(&mut flattened) {
            found.push(key.to_string());
        }
    }
    Some(found)
}

/// The names of the structs `source` declares outside its comments.
fn structs(source: &str) -> Vec<&str> {
    source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .filter_map(|line| line.split_once("struct ")?.1.split_whitespace().next())
        .collect()
}

/// The first code span of `line`.
fn code(line: &str) -> Option<&str> {
    let (_, rest) = line.split_once('`')?;
    rest.split_once('`').map(|(span, _)| span)
}

/// The keys `text` lists, each with the table it is listed under: the first code span of a heading
/// that is a table's name, then the first code span of each row of the tables below it. A fenced
/// block is passed over.
pub(super) fn listed(text: &str) -> BTreeSet<(String, String)> {
    let (mut table, mut fenced, mut keys) = (None, false, BTreeSet::new());
    for line in text.lines() {
        if line.starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        if line.starts_with('#') {
            table = code(line)
                .filter(|span| span.starts_with('['))
                .map(str::to_string);
        } else if let Some(table) = &table
            && line.starts_with("| `")
            && let Some(key) = code(line)
        {
            keys.insert((table.clone(), key.to_string()));
        }
    }
    keys
}

#[test]
fn the_reference_names_every_key_the_symphony_reads_and_no_other() {
    let mut read_keys = BTreeSet::new();
    for (file, name, table) in TABLES {
        let fields = fields(&read(file), name).unwrap_or_else(|| {
            panic!("{file} has no struct {name}: name where {table} is read now")
        });
        assert!(!fields.is_empty(), "{name} in {file} has no field");
        read_keys.extend(fields.into_iter().map(|key| (table.to_string(), key)));
    }
    let listed = listed(&read(REFERENCE));
    let lacks: Vec<_> = read_keys.difference(&listed).collect();
    let extra: Vec<_> = listed.difference(&read_keys).collect();
    assert!(
        lacks.is_empty() && extra.is_empty(),
        "{REFERENCE} lacks {lacks:?}, and lists {extra:?}, which the Symphony does not read"
    );
}

#[test]
fn every_table_the_file_holds_has_its_struct_here() {
    let tables = fields(&read(FILE), "File").expect("the file's struct");
    assert!(tables.contains(&"project".to_string()), "{tables:?}");
    for table in &tables {
        assert!(
            TABLES.iter().any(
                |(_, _, heading)| [format!("[{table}]"), format!("[[{table}]]")]
                    .contains(&heading.to_string())
            ),
            "the table {table} is read, and TABLES does not name its struct"
        );
    }
    for source in SOURCES {
        let text = read(source);
        for name in structs(&text)
            .into_iter()
            .filter(|name| name.starts_with("File"))
        {
            assert!(
                name == "File" || TABLES.iter().any(|(_, named, _)| *named == name),
                "{source} reads a table into {name}, which TABLES does not name"
            );
        }
    }
}

#[test]
fn the_reading_finds_fields_but_flattened_ones_and_the_reference_rows_under_tables() {
    let source = "/// A table, the struct of [x].\n#[derive(Deserialize)]\npub(super) struct FileX {\n    /// a: b\n    \
                  pub(super) one: Option<String>,\n    #[serde(default)]\n    two: Vec<String>,\n    \
                  #[serde(flatten)]\n    pub(super) unknown: Unknown,\n    pub three: bool,\n}\n\
                  struct FileY {\n    four: u32,\n}\n";
    assert_eq!(fields(source, "FileX").unwrap(), ["one", "two", "three"]);
    assert_eq!(fields(source, "FileY").unwrap(), ["four"]);
    assert_eq!(fields(source, "FileZ"), None);
    assert_eq!(structs(source), ["FileX", "FileY"]);
    let text = "# The file\n\n| `out` | no table above |\n\n## `[a]`\n\n| Key | Default |\n| --- | --- \
                |\n| `k` | `1` |\n\n```toml\n# a comment\n| `fenced` |\n```\n\n| `l` | none |\n\n## \
                Notes on `x`\n\n| `m` | under no table |\n";
    let pair = |table: &str, key: &str| (table.to_string(), key.to_string());
    assert_eq!(
        listed(text),
        BTreeSet::from([pair("[a]", "k"), pair("[a]", "l")])
    );
}

#[test]
fn init_names_the_reference_in_the_symphonys_repository() {
    let url = symphony::init::PROJECT_FILE_REFERENCE;
    let path = url
        .strip_prefix("https://github.com/heavenstudio/symphony/blob/main/")
        .unwrap_or_else(|| panic!("{url}"));
    assert_eq!(path, REFERENCE);
    assert!(repo().join(path).is_file(), "{path}");
}
