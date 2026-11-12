//! `PreToolUse` hook: an agent's memory folder is off limits (`symphony hook memory-guard`).
//!
//! What must outlast a session goes in the repository, committed, where every agent sees it. So a
//! write to Claude Code's per-project memory folder (`~/.claude/projects/*/memory/`) or Codex's
//! (`~/.codex/memories/`) is refused: a Write, Edit, MultiEdit or NotebookEdit whose path is in one,
//! and a Bash command that names one, or runs in one, and writes. A refusal is [`REASON`] on stderr
//! and exit 2, which Claude Code shows the agent; everything else, reads included, passes silently.
//!
//! A command is read as text, not run. It names the folder when it, or the folder it runs in, holds
//! the folder's path, or a folder above it (`.claude`, `.claude/projects`, `.claude/projects/NAME`,
//! `.codex`) together with a `memory` or `memories` part, which a `cd` or a variable joins; `.` and
//! `NAME/..` parts are folded first ([`folded`]). It writes when it holds a redirection into a file
//! (not a descriptor, not a device such as `/dev/null`) or a program that writes the files it is
//! given or runs anything ([`WRITER`], [`EDITOR`]). So a command that reads the folder and writes
//! elsewhere in the same call is refused too (Read and Write copy out of it), and so is one that
//! only mentions the folder in text beside a write; a write that never names the folder, from a
//! folder outside it, passes.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use super::event;

/// The line a refusal prints. Its first word is what the installed command passes on
/// (`crate::init`); any other failure lets the call through.
pub const REASON: &str = "Blocked: an agent's memory folder is off limits. A write tool's path in it, or a shell command that names it (or runs in it) and writes anything, is refused; a command that only mentions it in text (a commit message, a board post) is refused too, so rephrase that text (\"the memory folder\") or put it in a file with Write. What must outlast this session goes in the repository, committed: guidance in an AGENTS.md, a skill, a doc, or a task on the board (just tasks create).";

/// The tools that write the file their input names.
const WRITE_TOOLS: [&str; 4] = ["Write", "Edit", "MultiEdit", "NotebookEdit"];

/// The folders, as path components, each starting one: Claude Code's `.claude/projects/NAME/memory`
/// and Codex's `.codex/memories`, in any case (macOS's file system ignores it). A component starts at
/// the text's start, a `/` or what ends a shell word, and ends at the text's end, a `/` or the same;
/// so `memory-notes`, `memory.md`, `docs/memory` and `x.claude/projects` are other folders.
static FOLDER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)(?:^|[/\s'"=:;&|()<>`])(?:\.claude/+projects/+[^/\s'"]+/+memory|\.codex/+memories)(?:$|[/\s'"\\;&|()<>`])"#,
    )
    .expect("the memory folders' pattern")
});

/// A folder above a memory folder, the path ending there: `.claude`, `.claude/projects`,
/// `.claude/projects/NAME` or `.codex`. `.claude/worktrees/x` and `.claude/settings.json` are not.
static ABOVE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)(?:^|[/\s'"=:;&|()<>`])(?:\.claude(?:/+projects(?:/+[^/\s'"]+)?)?|\.codex)/*(?:$|[\s'"\\;&|()<>`])"#,
    )
    .expect("the folders above the memory folders' pattern")
});

/// A path part named as a memory folder is: `memory` or `memories`.
static PART: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(?:^|[/\s'"=:;&|()<>`])(?:memory|memories)(?:$|[/\s'"\\;&|()<>`])"#)
        .expect("the memory part's pattern")
});

/// A program that writes the files it is given, unpacks or downloads into a folder, or runs anything:
/// a whole shell word, maybe named by its path (`/bin/rm`) or escaped past an alias (`\rm`).
static WRITER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?:^|[\s;&|()<>`'"{}/\\])(?:tee|rm|rmdir|unlink|shred|mv|cp|ln|install|rsync|ditto|scp|touch|mkdir|truncate|dd|chmod|chown|patch|tar|unzip|zip|gzip|gunzip|curl|wget|python[0-9.]*|perl|ruby|node|osascript|sh|bash|zsh|eval|xargs)(?:$|[\s;&|()<>`'"{}])"#,
    )
    .expect("the writers' pattern")
});

/// A reader with a flag that makes it write: `sed -i` (`--in-place`), `find` with an action that
/// deletes, runs or prints to a file.
static EDITOR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?:^|[\s;&|()`'"/\\])(?:sed\s(?:[^;&|\n]*\s)?(?:-[a-zA-Z]*i|--in-place)|find\s(?:[^;&|\n]*\s)?-(?:delete|exec|execdir|ok|okdir|fprint0?|fprintf|fls)(?:$|\s))"#,
    )
    .expect("the in-place editors' pattern")
});

/// Where output may go without writing a file.
const DEVICES: [&str; 4] = ["/dev/null", "/dev/stdout", "/dev/stderr", "/dev/tty"];

/// `path` with its `.` parts dropped and each `NAME/..` folded, as the file system reads them
/// (links aside): `X/./memory` and `X/../X/memory` are `X/memory`. A `..` at the root or after
/// another stays.
fn fold(path: &str) -> String {
    if !path.contains('/') {
        return path.to_string();
    }
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "." => {}
            ".." if parts.last().is_some_and(|last| !matches!(*last, "" | "..")) => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    parts.join("/")
}

/// `text` with every path in it folded ([`fold`]): a path runs between blanks, quotes and shell
/// operators.
fn folded(text: &str) -> String {
    let ends = |c: char| c.is_whitespace() || "'\"`;&|()<>=:".contains(c);
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(end) = rest.find(ends) {
        out.push_str(&fold(&rest[..end]));
        let width = rest[end..].chars().next().map_or(1, char::len_utf8);
        out.push_str(&rest[end..end + width]);
        rest = &rest[end + width..];
    }
    out.push_str(&fold(rest));
    out
}

/// Whether `path` is a memory folder, or a path in one.
fn in_folder(path: &str) -> bool {
    FOLDER.is_match(&fold(path))
}

/// Whether a shell command run in `cwd` names a memory folder: its path in the command or the
/// folder it runs in, or a folder above one there with a `memory` part in the command.
fn names_a_folder(command: &str, cwd: &str) -> bool {
    let (command, cwd) = (folded(command), fold(cwd));
    FOLDER.is_match(&command)
        || FOLDER.is_match(&cwd)
        || ((ABOVE.is_match(&command) || ABOVE.is_match(&cwd)) && PART.is_match(&command))
}

/// Whether `command` redirects output into a file: a `>` (`>>`, `>|`, `&>`, `2>`, `>&`) whose target
/// is no descriptor (`2>&1`, `>&-`) and no device. A target that cannot be read (`>(…)`) counts.
fn redirects_into_a_file(command: &str) -> bool {
    let mut rest = command;
    while let Some(at) = rest.find('>') {
        rest = &rest[at + 1..];
        rest = rest.strip_prefix('>').unwrap_or(rest);
        rest = rest.strip_prefix('|').unwrap_or(rest);
        if let Some(after) = rest.strip_prefix('&') {
            if after.starts_with(|c: char| c.is_ascii_digit() || c == '-') {
                continue;
            }
            rest = after;
        }
        let target: String = rest
            .trim_start_matches([' ', '\t'])
            .chars()
            .take_while(|c| !c.is_whitespace() && !";|&<>()".contains(*c))
            .filter(|c| !matches!(c, '\'' | '"'))
            .collect();
        if !DEVICES.contains(&target.as_str()) && !target.starts_with("/dev/fd/") {
            return true;
        }
    }
    false
}

/// Whether a shell command run in `cwd` writes in a memory folder: it names one ([`names_a_folder`])
/// and writes anything.
fn blocked_command(command: &str, cwd: &str) -> bool {
    names_a_folder(command, cwd)
        && (redirects_into_a_file(command) || WRITER.is_match(command) || EDITOR.is_match(command))
}

/// Whether a write tool's input names a path in a memory folder: a string under a key `path` or
/// `paths`, or one ending `_path` or `_paths`, however deep (Write's `file_path`, NotebookEdit's
/// `notebook_path`). Text the tool writes (`content`, `new_string`) is no path.
fn names_a_path_in_a_folder(value: &Value, path: bool) -> bool {
    match value {
        Value::Object(fields) => fields.iter().any(|(key, value)| {
            let path = matches!(key.as_str(), "path" | "paths")
                || key.ends_with("_path")
                || key.ends_with("_paths");
            names_a_path_in_a_folder(value, path)
        }),
        Value::Array(items) => items
            .iter()
            .any(|item| names_a_path_in_a_folder(item, path)),
        Value::String(text) => path && in_folder(text),
        _ => false,
    }
}

/// A string field's text; any other value, or none, is empty.
fn text(value: Option<&Value>) -> &str {
    value.and_then(Value::as_str).unwrap_or_default()
}

/// Whether the tool call `raw` describes is one this guard refuses. An event that is not JSON, or
/// names no tool, refuses nothing.
pub fn refuses(raw: &[u8]) -> bool {
    let Some(Value::Object(event)) = event::parse(raw) else {
        return false;
    };
    let input = event.get("tool_input").unwrap_or(&Value::Null);
    match text(event.get("tool_name")) {
        "Bash" => blocked_command(text(input.get("command")), text(event.get("cwd"))),
        tool if WRITE_TOOLS.contains(&tool) => names_a_path_in_a_folder(input, false),
        _ => false,
    }
}

#[cfg(test)]
#[path = "memory_guard_tests.rs"]
mod tests;
