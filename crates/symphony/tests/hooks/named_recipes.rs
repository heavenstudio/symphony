//! Every `just NAME` the Symphony tells a project to run is a recipe `symphony init` installs
//! (`shipped/symphony.just`), or the consumer's landing, named as the project's own: in the shipped
//! skills and rules, the docs, and the strings its binaries print. The allow rules init writes and
//! the Codex rules it ships allow those recipes but the two server ones.

use regex::Regex;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The consumer's recipes, which the text names as the project's where it has them.
const CONSUMERS: [&str; 6] = [
    "land",
    "land-approve",
    "land-request",
    "land-stamps",
    "merge-feature",
    "merge-risk",
];

/// Words that follow "just" in prose, never a recipe.
const PROSE: [&str; 20] = [
    "above",
    "after",
    "as",
    "became",
    "before",
    "done",
    "finished",
    "launched",
    "like",
    "now",
    "one",
    "past",
    "published",
    "pushed",
    "ran",
    "started",
    "successful",
    "the",
    "this",
    "written",
];

/// Text of this repository's own development, which names its own justfile's recipes: a source
/// kit's client build, which no project runs, and the checks a task here proves its work with
/// (`check-fast` is the recipe filed as the task `tools/check-fast`), and its releases, which the
/// nightly cuts and no project runs.
const OWN: [(&str, &str); 14] = [
    ("apps/symphony-server/src/api/client.rs", "symphony-ui-dist"),
    ("apps/symphony-server/src/development.rs", "symphony-ui-dev"),
    ("crates/symphony/src/nightly.rs", "nightly"),
    ("crates/symphony/src/nightly.rs", "verify"),
    ("crates/symphony/src/nightly/board.rs", "nightly"),
    ("crates/symphony/src/nightly/schedule.rs", "nightly"),
    ("docs/CHECKS.md", "check-fast"),
    ("docs/CHECKS.md", "client-update"),
    ("docs/CHECKS.md", "lint-rs"),
    ("docs/CHECKS.md", "nightly"),
    ("docs/CHECKS.md", "test-rs"),
    ("docs/CHECKS.md", "verify"),
    ("docs/RELEASES.md", "nightly"),
    ("docs/RELEASES.md", "verify"),
];

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

/// The recipes a justfile's text defines, private ones (`_NAME`) and settings aside.
pub(crate) fn recipes(text: &str) -> BTreeSet<String> {
    let head = Regex::new(r"^([a-z][a-z0-9_-]*)(?:\s[^:]*)?:(?:[^=]|$)").unwrap();
    text.lines()
        .filter_map(|line| head.captures(line))
        .map(|found| found[1].to_string())
        .collect()
}

/// The installed recipes.
fn installed() -> BTreeSet<String> {
    recipes(include_str!("../../shipped/symphony.just"))
}

/// Each `just NAME` in `text`, with its line counted from `first`.
fn named_in(text: &str, first: usize) -> Vec<(usize, String)> {
    let command = Regex::new(r"(?:^|[^A-Za-z0-9_-])just\s+([a-z][a-z0-9_-]*)").unwrap();
    command
        .captures_iter(text)
        .map(|found| {
            let at = found.get(1).unwrap().start();
            (
                first + text[..at].matches('\n').count(),
                found[1].to_string(),
            )
        })
        .collect()
}

/// The string literals of Rust source `text`, each with the line it starts on, out of comments and
/// out of every item `#[cfg(test)]` gates: what the binary can print.
pub(crate) fn literals(text: &str) -> Vec<(usize, String)> {
    #[derive(Debug, PartialEq)]
    enum Token {
        Word(String),
        Text(usize, String),
        Mark(char),
    }
    let chars: Vec<char> = text.chars().collect();
    let mut tokens = Vec::new();
    let (mut at, mut line) = (0, 1);
    let peek = |at: usize| chars.get(at).copied().unwrap_or('\0');
    while at < chars.len() {
        let c = chars[at];
        if c == '\n' {
            line += 1;
            at += 1;
        } else if c.is_whitespace() {
            at += 1;
        } else if c == '/' && peek(at + 1) == '/' {
            while at < chars.len() && chars[at] != '\n' {
                at += 1;
            }
        } else if c == '/' && peek(at + 1) == '*' {
            let mut depth = 0;
            while at < chars.len() {
                if chars[at] == '/' && peek(at + 1) == '*' {
                    depth += 1;
                    at += 2;
                } else if chars[at] == '*' && peek(at + 1) == '/' {
                    depth -= 1;
                    at += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    line += usize::from(chars[at] == '\n');
                    at += 1;
                }
            }
        } else if c == '"' {
            let start = line;
            let mut text = String::new();
            at += 1;
            while at < chars.len() && chars[at] != '"' {
                if chars[at] == '\\' {
                    let escaped = peek(at + 1);
                    at += 2;
                    match escaped {
                        '\n' => {
                            line += 1;
                            while at < chars.len() && chars[at].is_whitespace() {
                                line += usize::from(chars[at] == '\n');
                                at += 1;
                            }
                        }
                        'n' => text.push('\n'),
                        't' => text.push('\t'),
                        other => text.push(other),
                    }
                } else {
                    line += usize::from(chars[at] == '\n');
                    text.push(chars[at]);
                    at += 1;
                }
            }
            at += 1;
            tokens.push(Token::Text(start, text));
        } else if c == '\'' {
            if peek(at + 1) == '\\' {
                at += 3;
                while at < chars.len() && chars[at] != '\'' {
                    at += 1;
                }
                at += 1;
            } else if peek(at + 2) == '\'' {
                at += 3;
            } else {
                at += 1;
            }
        } else if c.is_alphanumeric() || c == '_' {
            let mut word = String::new();
            while at < chars.len() && (chars[at].is_alphanumeric() || chars[at] == '_') {
                word.push(chars[at]);
                at += 1;
            }
            let raw = matches!(word.as_str(), "r" | "br" | "cr");
            let hashes = chars[at..].iter().take_while(|c| **c == '#').count();
            if raw && peek(at + hashes) == '"' {
                let start = line;
                let close: String = std::iter::once('"')
                    .chain(std::iter::repeat_n('#', hashes))
                    .collect();
                let rest: String = chars[at + hashes + 1..].iter().collect();
                let end = rest.find(&close).unwrap_or(rest.len());
                let body = &rest[..end];
                line += body.matches('\n').count();
                at += hashes + 1 + body.chars().count() + close.chars().count();
                tokens.push(Token::Text(start, body.to_string()));
            } else if !matches!(word.as_str(), "b" | "c") || peek(at) != '"' {
                tokens.push(Token::Word(word));
            }
        } else {
            tokens.push(Token::Mark(c));
            at += 1;
        }
    }
    let test_gate = [
        Token::Mark('#'),
        Token::Mark('['),
        Token::Word("cfg".into()),
        Token::Mark('('),
        Token::Word("test".into()),
        Token::Mark(')'),
        Token::Mark(']'),
    ];
    let mut found = Vec::new();
    let mut at = 0;
    while at < tokens.len() {
        if tokens[at..].starts_with(&test_gate) {
            at += test_gate.len();
            let mut depth = 0i32;
            while at < tokens.len() {
                match tokens[at] {
                    Token::Mark('{' | '(' | '[') => depth += 1,
                    Token::Mark('}') if depth == 1 => {
                        at += 1;
                        break;
                    }
                    Token::Mark(';') if depth == 0 => {
                        at += 1;
                        break;
                    }
                    Token::Mark('}' | ')' | ']') => depth -= 1,
                    _ => {}
                }
                at += 1;
            }
            continue;
        }
        if let Token::Text(line, text) = &tokens[at] {
            found.push((*line, text.clone()));
        }
        at += 1;
    }
    found
}

/// Every file under `dir`, recursively, in a stable order.
fn files(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            found.extend(files(&path));
        } else {
            found.push(path);
        }
    }
    found
}

/// Each `just NAME` the Symphony says, as (file from the workspace root, line, NAME).
fn named() -> Vec<(String, usize, String)> {
    let root = workspace();
    let relative = |path: &Path| path.strip_prefix(&root).unwrap().display().to_string();
    let mut found = Vec::new();
    for path in files(&root.join("crates/symphony/shipped")) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (line, name) in named_in(&text, 1) {
            found.push((relative(&path), line, name));
        }
    }
    // The docs say how things are now; the decision register and the analyses are records.
    for path in files(&root.join("docs")) {
        let rel = relative(&path);
        let record = rel == "docs/DECISIONS.md"
            || rel.starts_with("docs/analysis/")
            || rel.starts_with("docs/history/");
        if record || path.extension().is_none_or(|ext| ext != "md") {
            continue;
        }
        for (line, name) in named_in(&std::fs::read_to_string(&path).unwrap(), 1) {
            found.push((rel.clone(), line, name));
        }
    }
    for group in ["crates", "apps"] {
        let mut members: Vec<_> = std::fs::read_dir(root.join(group))
            .unwrap()
            .map(|entry| entry.unwrap().path().join("src"))
            .filter(|src| src.is_dir())
            .collect();
        members.sort();
        for path in members.iter().flat_map(|src| files(src)) {
            let rel = relative(&path);
            let name = path.file_name().unwrap().to_string_lossy();
            let test = name == "tests.rs"
                || name.ends_with("_tests.rs")
                || rel.split('/').any(|part| part == "tests");
            if test || path.extension().is_none_or(|ext| ext != "rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            for (first, literal) in literals(&text) {
                for (line, name) in named_in(&literal, first) {
                    found.push((rel.clone(), line, name));
                }
            }
        }
    }
    found
}

#[test]
fn every_just_command_the_symphony_names_is_an_installed_recipe_or_the_consumers() {
    let installed = installed();
    let named = named();
    let mut unknown = Vec::new();
    let mut own_used = BTreeSet::new();
    for (file, line, name) in &named {
        if installed.contains(name)
            || CONSUMERS.contains(&name.as_str())
            || PROSE.contains(&name.as_str())
        {
            continue;
        }
        if let Some(own) = OWN.iter().find(|(at, own)| file == at && name == own) {
            own_used.insert(*own);
            continue;
        }
        unknown.push(format!("{file}:{line}: just {name}"));
    }
    assert!(
        unknown.is_empty(),
        "these name a recipe a freshly initialised project lacks: ship it in symphony.just, name \
         the command another way, or say it is the project's own (CONSUMERS); a word of prose \
         after \"just\" goes in PROSE: {unknown:#?}"
    );
    let stale: Vec<_> = OWN.iter().filter(|own| !own_used.contains(own)).collect();
    assert!(stale.is_empty(), "no longer said: {stale:?}");
    // The walk reads each kind of text: a skill, a doc, and a message a binary prints.
    for (file, name) in [
        ("crates/symphony/shipped/skills/work/SKILL.md", "tasks"),
        ("docs/TASKS.md", "agent-send"),
        ("crates/symphony/src/save.rs", "symphony-ask"),
        ("crates/tasks/src/store/mod.rs", "tasks"),
    ] {
        assert!(
            named.iter().any(|(at, _, said)| at == file && said == name),
            "{file} says `just {name}`"
        );
    }
}

#[test]
fn the_allow_rules_and_the_codex_rules_allow_installed_recipes_but_the_servers() {
    let installed = installed();
    // An agent is asked before `just symphony`, which starts, replaces or stops the machine's
    // shared server, and before `just symphony-cli`, which reaches `serve`, `init` and
    // `publish-client`: both stay installed, and no rule allows them.
    let asked: BTreeSet<String> = ["symphony", "symphony-cli"].map(String::from).into();
    assert!(asked.is_subset(&installed), "{installed:?}");
    let allowed_here: BTreeSet<String> = installed.difference(&asked).cloned().collect();
    let tree = super::support::Tree::new();
    let done = std::process::Command::new(super::common::BIN)
        .args(["init", "--root"])
        .arg(&tree.root)
        .output()
        .unwrap();
    assert!(done.status.success(), "{done:?}");
    let settings: serde_json::Value =
        serde_json::from_str(&tree.read(".claude/settings.json")).unwrap();
    let rule =
        Regex::new(r"^Bash\((?:TASKS_SESSION=\* )?just ([a-z][a-z0-9_-]*)(?::\*| \*)\)$").unwrap();
    let allowed: BTreeSet<String> = settings["permissions"]["allow"]
        .as_array()
        .unwrap()
        .iter()
        .map(|allow| {
            let allow = allow.as_str().unwrap();
            rule.captures(allow)
                .unwrap_or_else(|| panic!("{allow} allows no recipe"))[1]
                .to_string()
        })
        .collect();
    assert_eq!(allowed, allowed_here);

    let prefix = Regex::new(r#"prefix_rule\(pattern=\["just", "([a-z][a-z0-9_-]*)"\]"#).unwrap();
    let codex = include_str!("../../shipped/codex.rules");
    let codex: BTreeSet<String> = prefix
        .captures_iter(codex)
        .map(|found| found[1].to_string())
        .collect();
    assert_eq!(codex, allowed_here);
}

#[test]
fn a_justfile_s_recipes_are_its_public_headers() {
    assert_eq!(
        recipes(
            "set x := 'y'\n_private := 'p'\n# just tasks: no\n[positional-arguments]\ntasks *ARGS:\n    \
             @echo\nbuild: fmt\nsymphony-cli ARG=\"a:b\":\nkey := \"v\"\n"
        ),
        ["build", "symphony-cli", "tasks"]
            .into_iter()
            .map(String::from)
            .collect()
    );
}

#[test]
fn just_names_are_read_after_a_word_boundary_and_counted_by_line() {
    assert_eq!(
        named_in(
            "run `just tasks`\nthen just\n  agent-send x, adjust it, just_x y",
            3
        ),
        [(3, "tasks".to_string()), (5, "agent-send".to_string())]
    );
}

#[test]
fn literals_leave_out_comments_characters_and_test_items() {
    let source = r####"
// "just commented" out
/* a "block /* nested */ comment" */
const A: &str = "just one";
fn f<'a>(x: &'a str) -> char { let _ = (b'"', '"', '\'', '\u{22}'); 'q' }
#[cfg(test)]
mod tests { fn g() { let _ = "just test"; } }
#[cfg(test)]
const B: &str = "just gated";
const C: &str = r#"raw "just raw""#;
const D: &str = "line \
    continued \"quoted\"\n";
const E: &[u8] = b"bytes";
let r#type = 1;
"####;
    assert_eq!(
        literals(source),
        [
            (4, "just one".to_string()),
            (10, "raw \"just raw\"".to_string()),
            (11, "line continued \"quoted\"\n".to_string()),
            (13, "bytes".to_string()),
        ]
    );
}
