//! A variable a recipe sets for Symphony is one Symphony reads. `just verify` (its script,
//! `tools/verify.sh`) points the browser tests' servers at the client it has just checked by
//! `SYMPHONY_UI_DIST`: under a name the server does not read they would serve the embedded client,
//! and the run would pass on a client it never checked.

use std::path::{Path, PathBuf};

/// The recipe files, this workspace's own and the one `symphony init` installs, and the script
/// the verify recipe runs.
const RECIPES: [&str; 3] = ["justfile", "crates/symphony/shipped/symphony.just", VERIFY];

/// The verify recipe's script.
const VERIFY: &str = "tools/verify.sh";

/// Where the verify script has the client check build the client it then checks.
const VERIFIED: &str = "build/verified-client";

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

/// The text of `file`, in the workspace.
fn text_of(file: &str) -> String {
    std::fs::read_to_string(workspace().join(file)).unwrap_or_else(|e| panic!("{file}: {e}"))
}

/// Each variable `file` sets, as `assigned` reads it.
fn set_in(file: &str) -> Vec<(usize, String, String)> {
    assigned(&text_of(file))
}

#[test]
fn the_verify_recipe_names_the_client_it_checked_by_the_variable_the_server_reads() {
    let script = text_of(VERIFY);
    let lines: Vec<&str> = script.lines().map(str::trim).collect();
    // The client check builds into its folder, and the client it built is that folder's public/.
    let folder = format!("client={VERIFIED}");
    assert!(lines.contains(&folder.as_str()), "{VERIFY} sets {folder}");
    assert!(
        lines
            .iter()
            .any(|line| line.contains("client-check \"$client\"")),
        "{VERIFY} runs the client check into \"$client\""
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("fresh=\"$PWD/$client/public\"")),
        "{VERIFY} takes the client it checked from \"$client\"/public"
    );
    let named: Vec<String> = assigned(&script)
        .into_iter()
        .filter(|(_, _, value)| value == "\"$fresh\"")
        .map(|(_, name, _)| name)
        .collect();
    assert_eq!(
        named,
        [symphony_env::UI_DIST.name],
        "{VERIFY} names the client it checked once, to the server by the variable it reads"
    );
}

#[test]
fn every_symphony_variable_a_recipe_sets_is_one_symphony_reads() {
    let read: Vec<&str> = symphony_env::ALL
        .iter()
        .map(|var| var.name)
        .chain([
            symphony_board::land::RECIPE,
            symphony_board::confine::MARKER,
        ])
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
        "set for Symphony, read by nothing of it (symphony_env::ALL, the land's marker, the \
         unasked recipes' marker):\n{}",
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
