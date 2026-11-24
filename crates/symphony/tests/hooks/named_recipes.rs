//! Every `just NAME` the Symphony tells a project to run in what it delivers and prints is a recipe
//! `symphony init` installs (`shipped/symphony.just`), or the consumer's landing, named as the
//! project's own: in the shipped skills, rules and recipes, and the strings its binaries print. The
//! rule is the docs audit's, read by the one reader the audit holds (`src/docs_audit/recipes.rs`,
//! taken here by its path) from this repository's lists (`tools/named-recipes.txt`), so the two
//! cannot disagree; the audit judges the documents, what the Symphony delivers and the guides, and
//! this test what it delivers and the binaries' strings, each keeping current the lists' entries
//! for the files the other does not judge. The allow rules init writes and the Codex rules it
//! delivers allow those recipes but the two server ones.

// The audit's own reader, of which this test needs less than the audit does.
#[path = "../../src/docs_audit/recipes.rs"]
#[allow(dead_code)]
mod rule;

use crate::support::repo as workspace;
use regex::Regex;
use rule::{Rule, Standing, audited, named_in, recipes};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

const SHIPPED_RECIPES: &str = include_str!("../../shipped/symphony.just");

/// This repository's lists and its justfile, which the rule borrows from.
static LISTS: LazyLock<String> = LazyLock::new(|| read(rule::LISTS));
static JUSTFILE: LazyLock<String> = LazyLock::new(|| read("justfile"));

/// The recipes `symphony init` installs.
fn installed() -> BTreeSet<String> {
    recipes(SHIPPED_RECIPES)
        .into_iter()
        .map(String::from)
        .collect()
}

fn read(file: &str) -> String {
    std::fs::read_to_string(workspace().join(file)).unwrap()
}

/// The rule of this repository's lists.
fn the_rule() -> Rule<'static> {
    Rule::read(&LISTS, SHIPPED_RECIPES, &JUSTFILE).unwrap()
}

#[test]
fn each_own_recipe_the_text_names_is_one_of_this_repositorys_and_a_filed_one_is_not_yet() {
    let rule = the_rule();
    for entry in &rule.entries {
        let (file, name) = (entry.file, entry.name);
        if entry.task.is_none() {
            assert!(
                rule.justfile.contains(&name),
                "{file} names `just {name}` as this repository's, and its justfile has no such recipe"
            );
        } else {
            assert!(
                !rule.justfile.contains(&name),
                "`just {name}` is a recipe now: move it from filed to own"
            );
        }
    }
    let (filed, own): (Vec<_>, Vec<_>) = rule.entries.iter().partition(|e| e.task.is_some());
    assert!(!own.is_empty() && !filed.is_empty(), "{rule:?}");
    // The reader of recipes sees each kind of head this justfile writes.
    assert!(rule.justfile.contains(&"fmt"), "a recipe of no arguments");
    assert!(
        rule.justfile.contains(&"test-rs"),
        "one of variadic arguments"
    );
    assert!(
        rule.justfile.contains(&"client-check"),
        "one of a defaulted argument"
    );
    assert!(rule.installed.contains(&"tasks") && rule.installed.contains(&"land"));
    let Standing::Entry(at) = rule.standing("docs/workflow/CHECKS.md", "check-fast") else {
        panic!("docs/workflow/CHECKS.md names `just check-fast` as filed: {rule:?}");
    };
    assert_eq!(rule.entries[at].task, Some("tools/check-fast"));
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

/// The texts the Symphony delivers or prints, each as (file from the workspace root, the line it
/// starts on, text): the shipped skills, rules and recipes, and the string literals of the members'
/// sources. The documents and the guides are the docs audit's alone.
fn texts() -> Vec<(String, usize, String)> {
    let root = workspace();
    let relative = |path: &Path| path.strip_prefix(&root).unwrap().display().to_string();
    let mut found = Vec::new();
    for path in files(&root.join("crates/symphony/shipped")) {
        if let Ok(text) = std::fs::read_to_string(&path) {
            found.push((relative(&path), 1, text));
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
                found.push((rel.clone(), first, literal));
            }
        }
    }
    found
}

#[test]
fn every_just_command_the_symphony_names_is_an_installed_recipe_or_the_consumers() {
    let texts = texts();
    // Each own or filed entry for a file the docs audit does not judge is this test's to keep
    // current: still named, and a filed one beside its task.
    let errors = the_rule().errors(&texts, |file| !audited(file));
    assert!(errors.is_empty(), "{errors:#?}");
    // The walk reads each kind of text: a skill, the recipes, and a message a binary prints.
    for (file, name) in [
        ("crates/symphony/shipped/skills/work/SKILL.md", "tasks"),
        ("crates/symphony/shipped/symphony.just", "tasks"),
        ("crates/symphony/src/save.rs", "symphony-ask"),
        ("crates/tasks/src/store/mod.rs", "tasks"),
    ] {
        let says = |(at, first, text): &(String, usize, String)| {
            at == file && named_in(text, *first).any(|(_, said)| said == name)
        };
        assert!(texts.iter().any(says), "{file} says `just {name}`");
    }
}

#[test]
fn the_allow_rules_and_the_codex_rules_allow_installed_recipes_but_those_that_ask_first() {
    let installed = installed();
    // An agent is asked before `just symphony`, which starts, replaces or stops the machine's
    // shared server, before `just symphony-cli`, which reaches `serve` and `init`, and before
    // `just tasks-people`, which changes a board's people or a profile: they stay installed, and
    // no rule allows them. `just land` is allowed: what it lands its approval check decides.
    let asked: BTreeSet<String> = ["symphony", "symphony-cli", "tasks-people"]
        .map(String::from)
        .into();
    assert!(asked.is_subset(&installed), "{installed:?}");
    let allowed_here: BTreeSet<String> = installed.difference(&asked).cloned().collect();
    assert!(allowed_here.contains("land"), "{allowed_here:?}");
    let tree = super::support::Tree::checkout();
    let done = std::process::Command::new(super::common::BIN)
        .args(["init", "--root"])
        .arg(&tree.root)
        .output()
        .unwrap();
    assert!(done.status.success(), "{done:?}");
    let settings: serde_json::Value =
        serde_json::from_str(&tree.read(".claude/settings.json")).unwrap();
    // A recipe's words, then the rule's one `*`: an env prefix's `*` would match any text,
    // another program included.
    let rule = Regex::new(r"^Bash\(just ([a-z][a-z0-9_-]*):\*\)$").unwrap();
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
    let codex_rules = include_str!("../../shipped/codex.rules");
    let codex: BTreeSet<String> = prefix
        .captures_iter(codex_rules)
        .map(|found| found[1].to_string())
        .collect();
    assert_eq!(codex, allowed_here);

    // Init writes no ask rule, and Codex prompts for nothing: `tasks land` refuses without the
    // marker only the `land` recipe sets, and what an approved land lands its approval check
    // decides, never a wait on the owner. No Codex rule allows a command with an env prefix.
    let marker = symphony_board::land::RECIPE;
    assert!(settings["permissions"].get("ask").is_none(), "{settings:#}");
    assert!(!codex_rules.contains(marker));
    assert!(
        codex_rules
            .lines()
            .filter(|line| line.starts_with("prefix_rule"))
            .all(|line| line.ends_with("decision=\"allow\")")),
        "{codex_rules}"
    );
    assert!(
        codex_rules
            .lines()
            .filter(|line| line.starts_with("prefix_rule"))
            .all(|line| line.contains("pattern=[\"just\", ")),
        "{codex_rules}"
    );
    let recipes = SHIPPED_RECIPES;
    let setting: Vec<&str> = recipes
        .lines()
        .filter(|line| !line.trim_start().starts_with('#') && line.contains(marker))
        .collect();
    assert_eq!(setting.len(), 1, "{setting:?}");
    assert!(
        setting[0].starts_with(&format!("    @{marker}=1 "))
            && setting[0].ends_with(" tasks land \"$@\""),
        "{setting:?}"
    );
    let land = recipes
        .lines()
        .position(|line| line.starts_with("land "))
        .unwrap();
    assert_eq!(recipes.lines().nth(land + 1), Some(setting[0]));
}

#[test]
fn every_installed_recipe_leaves_its_errors_to_the_command_it_runs() {
    // A failing command has said why: `just` adds no recipe-failed line after it.
    let lines: Vec<&str> = SHIPPED_RECIPES.lines().collect();
    let mut seen = BTreeSet::new();
    for (at, line) in lines.iter().enumerate() {
        let Some(name) = recipes(line).pop().map(String::from) else {
            continue;
        };
        let attributes: Vec<&str> = lines[..at]
            .iter()
            .rev()
            .take_while(|above| above.starts_with('['))
            .copied()
            .collect();
        assert!(
            attributes.contains(&"[no-exit-message]"),
            "{name}: {attributes:?}"
        );
        seen.insert(name);
    }
    assert_eq!(seen, installed());
    assert!(seen.contains("land") && seen.contains("tasks"), "{seen:?}");
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
