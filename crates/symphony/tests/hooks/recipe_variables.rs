//! A variable a recipe sets for Symphony is one Symphony reads. `just verify` points the browser
//! tests' servers at the client it has just checked by `SYMPHONY_UI_DIST`: under a name the server
//! does not read they would serve the embedded client, and the run would pass on a client it never
//! checked.

use std::path::{Path, PathBuf};

/// The recipe files: this workspace's own and the one `symphony init` installs.
const RECIPES: [&str; 2] = ["justfile", "crates/symphony/shipped/symphony.just"];

/// What the verify recipe's test step names as the client it checked.
const VERIFIED: &str = "build/verified-client/public";

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Each `NAME=VALUE` a line of `text` sets, outside comments: its line number, the upper-case
/// name (one starting the line or after a space, `@`, `;` or `(`) and the value up to the next space.
fn assigned(text: &str) -> Vec<(usize, String, String)> {
    let mut found = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim_start().starts_with('#') {
            continue;
        }
        let bytes = line.as_bytes();
        let mut at = 0;
        while at < bytes.len() {
            let after_boundary =
                at == 0 || matches!(bytes[at - 1], b' ' | b'\t' | b'@' | b';' | b'(');
            if !(after_boundary && bytes[at].is_ascii_uppercase()) {
                at += 1;
                continue;
            }
            let end = at
                + bytes[at..]
                    .iter()
                    .take_while(|byte| {
                        byte.is_ascii_uppercase() || byte.is_ascii_digit() || **byte == b'_'
                    })
                    .count();
            if bytes.get(end) == Some(&b'=') && bytes.get(end + 1) != Some(&b'=') {
                let value: String = line[end + 1..]
                    .chars()
                    .take_while(|c| !c.is_whitespace())
                    .collect();
                found.push((index + 1, line[at..end].to_string(), value));
            }
            at = end.max(at + 1);
        }
    }
    found
}

/// Each variable `file` sets, as `assigned` reads it.
fn set_in(file: &str) -> Vec<(usize, String, String)> {
    let text =
        std::fs::read_to_string(workspace().join(file)).unwrap_or_else(|e| panic!("{file}: {e}"));
    assigned(&text)
}

#[test]
fn the_verify_recipe_names_the_client_it_checked_by_the_variable_the_server_reads() {
    let named: Vec<String> = set_in("justfile")
        .into_iter()
        .filter(|(_, _, value)| value.contains(VERIFIED))
        .map(|(_, name, _)| name)
        .collect();
    assert_eq!(
        named,
        [symphony_env::UI_DIST.name],
        "the justfile names {VERIFIED} once, to the server by the variable it reads"
    );
}

#[test]
fn every_symphony_variable_a_recipe_sets_is_one_symphony_reads() {
    let read: Vec<&str> = symphony_env::ALL
        .iter()
        .map(|var| var.name)
        .chain([symphony_board::land::RECIPE])
        .collect();
    let unread: Vec<String> = RECIPES
        .iter()
        .flat_map(|file| {
            set_in(file)
                .into_iter()
                .map(move |(line, name, _)| (file, line, name))
        })
        .filter(|(_, _, name)| name.contains("SYMPHONY") && !read.contains(&name.as_str()))
        .map(|(file, line, name)| format!("{file}:{line}: {name}"))
        .collect();
    assert!(
        unread.is_empty(),
        "set for Symphony, read by nothing of it (symphony_env::ALL, the land's marker):\n{}",
        unread.join("\n")
    );
}

#[test]
fn an_assignment_is_an_upper_case_name_at_a_word_start_before_one_equals_sign() {
    let text = "\
# OLD=1 in a comment
    @OLD_SYMPHONY_UI_DIST=\"$PWD/x\" run
    A=1 B_2=two cmd
    [ \"$X\" == y ] && lower=3 echo
    (C=4; true)
    run --flag=VALUE
";
    assert_eq!(
        assigned(text),
        [
            (2, "OLD_SYMPHONY_UI_DIST", "\"$PWD/x\""),
            (3, "A", "1"),
            (3, "B_2", "two"),
            (5, "C", "4;"),
        ]
        .map(|(line, name, value)| (line, name.to_string(), value.to_string()))
    );
}
