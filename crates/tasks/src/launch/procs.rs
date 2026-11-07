//! What runs under a pane, and whether any of it is work the pane must not lose: the command lines
//! under a process, from `ps`, and the patterns that read them (regular expressions matched by hand as
//! Python's `re` reads them: `\s` is what `str.isspace` calls space, `\w` a letter, digit or
//! underscore).

use super::{LaunchError, Processes};
use crate::cli::{Env, probes};
use crate::sys::Command;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long `ps` may take.
const PS_PATIENCE: Duration = Duration::from_secs(30);

/// The command lines of everything running under `pid`, from `ps`. A table `ps` could not give is
/// `LaunchError`: a pane is never called idle on no information.
pub fn descendants(pid: i64) -> Result<Vec<String>, LaunchError> {
    descendants_by(crate::sys::command("ps"), pid)
}

/// The process table a command in `env` reads: [`descendants`], or on a scratch board (a
/// `TASKS_DATABASE` other than the live one, as every test's is) with `TASKS_PS_TABLE` set,
/// [`listed`] from the file it names, so the command line's tests never read this machine's. On
/// the live board the variable is never read: a stale or leaked one cannot stand in for `ps` there.
pub fn table(env: &Env) -> Box<Processes> {
    match table_file(env) {
        None => Box::new(descendants),
        Some(file) => Box::new(move |pid| listed(&file, pid)),
    }
}

/// The file [`table`] reads in place of `ps`: `TASKS_PS_TABLE`'s, on a scratch board only.
pub fn table_file(env: &Env) -> Option<PathBuf> {
    let scratch = env
        .var("TASKS_DATABASE")
        .is_some_and(|db| db != crate::DATABASE);
    env.var("TASKS_PS_TABLE")
        .filter(|_| scratch)
        .map(PathBuf::from)
}

/// The command lines under `pid` in `file`, which holds a table as `ps -axo pid=,ppid=,command=`
/// prints it. A file that cannot be read, or that lists no process `pid`, is `LaunchError`, as
/// `ps` failing is: a table that does not describe the pane's shell says nothing of what runs there.
pub fn listed(file: &Path, pid: i64) -> Result<Vec<String>, LaunchError> {
    let table = std::fs::read(file)
        .map_err(|why| LaunchError(format!("ps table {}: {why}", file.display())))?;
    under_listed(&String::from_utf8_lossy(&table), pid).ok_or_else(|| {
        LaunchError(format!(
            "ps table {} lists no process {pid}",
            file.display()
        ))
    })
}

/// [`descendants`], the table read from `ps` (a test's stand-in for it): `-axo pid=,ppid=,command=`
/// is added to it. A table that lists no process `pid` is `LaunchError`, as one `ps` could not give.
pub fn descendants_by(mut ps: Command, pid: i64) -> Result<Vec<String>, LaunchError> {
    ps.args(["-axo", "pid=,ppid=,command="]);
    let done =
        probes::within(&mut ps, PS_PATIENCE).map_err(|why| LaunchError(format!("ps: {why}")))?;
    let table = String::from_utf8_lossy(&done.stdout);
    if !done.status.success() || table.trim().is_empty() {
        return Err(LaunchError("ps printed nothing".into()));
    }
    under_listed(&table, pid).ok_or_else(|| LaunchError(format!("ps lists no process {pid}")))
}

/// [`under`], or none when no line of `table` is process `pid`'s own.
fn under_listed(table: &str, pid: i64) -> Option<Vec<String>> {
    table
        .lines()
        .any(|line| columns(line).is_some_and(|(at, _, _)| at == pid))
        .then(|| under(table, pid))
}

/// The command lines under `pid` in `table` (`ps`'s `pid ppid command` lines), each child before
/// what runs under it, the last child's line of descent first, as a stack walks them. Each process
/// is walked once, so a table whose parents loop still ends.
pub fn under(table: &str, pid: i64) -> Vec<String> {
    let mut children: HashMap<i64, Vec<(i64, String)>> = HashMap::new();
    for line in table.lines() {
        let Some((child, parent, command)) = columns(line) else {
            continue;
        };
        children
            .entry(parent)
            .or_default()
            .push((child, command.to_string()));
    }
    let (mut found, mut todo, mut seen) = (Vec::new(), vec![pid], HashSet::from([pid]));
    while let Some(at) = todo.pop() {
        for (child, command) in children.get(&at).into_iter().flatten() {
            if seen.insert(*child) {
                found.push(command.clone());
                todo.push(*child);
            }
        }
    }
    found
}

/// A `ps` line's pid, ppid and command, as `line.split(None, 2)` reads it.
fn columns(line: &str) -> Option<(i64, i64, &str)> {
    let rest = line.trim_start_matches(space);
    let (pid, rest) = rest.split_once(space)?;
    let rest = rest.trim_start_matches(space);
    let (ppid, rest) = rest.split_once(space)?;
    let command = rest.trim_start_matches(space);
    if command.is_empty() {
        return None;
    }
    Some((number(pid)?, number(ppid)?, command))
}

fn number(digits: &str) -> Option<i64> {
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// Why a pane running these commands must stay, or None when nothing in them is work: the first
/// command a protected pattern matches, else the first that is a shell (a shell started a command:
/// that command is work) or is not the agent or one of its helper servers.
pub fn busy_reason(commands: &[String]) -> Option<String> {
    protected(commands).or_else(|| {
        commands
            .iter()
            .find(|c| shell(c) || !harmless(c))
            .map(|c| format!("runs {}", head(c)))
    })
}

/// [`busy_reason`] for the processes under an agent the idle sweep looks at: a helper Claude Code
/// keeps (caffeinate) and a board command or monitor are not work either.
pub fn reap_busy_reason(commands: &[String]) -> Option<String> {
    protected(commands).or_else(|| {
        commands
            .iter()
            .filter(|c| codex_exec(&c.to_ascii_lowercase()) || (!helper(c) && !board(c)))
            .find(|c| shell(c) || !harmless(c))
            .map(|c| format!("runs {}", head(c)))
    })
}

/// Whether a command line is of a kind.
type Matcher = fn(&str) -> bool;

/// What a pane must never lose: the first matching command names why.
const PROTECTED: [(&str, Matcher); 4] = [
    ("a verify", verify),
    ("a test run", test_run),
    ("a land", land),
    ("the Symphony", symphony_app),
];

fn protected(commands: &[String]) -> Option<String> {
    commands.iter().find_map(|command| {
        PROTECTED
            .iter()
            .find(|(_, matches)| matches(command))
            .map(|(why, _)| format!("runs {why} ({})", head(command)))
    })
}

/// The first 60 characters, which a reason quotes.
fn head(command: &str) -> String {
    command.chars().take(60).collect()
}

/// `(?:^|[\s/])verify(?:-full)?(?:\s|$)`
pub fn verify(command: &str) -> bool {
    word_with(command, "verify", "-full")
}

/// `run_sim_tests|run_tool_suites|test[-_](?:sim|render|tools)|tests/run_|\bgodot\b`
pub fn test_run(command: &str) -> bool {
    const NAMES: [&str; 9] = [
        "run_sim_tests",
        "run_tool_suites",
        "test-sim",
        "test_sim",
        "test-render",
        "test_render",
        "test-tools",
        "test_tools",
        "tests/run_",
    ];
    NAMES.iter().any(|name| command.contains(name))
        || starts(command, "godot").any(|at| {
            before(command, at).is_none_or(|c| !word(c))
                && after(command, at + 5).is_none_or(|c| !word(c))
        })
}

/// `(?:^|[\s/])land(?:\.py)?(?:\s|$)`
pub fn land(command: &str) -> bool {
    word_with(command, "land", ".py")
}

/// The Symphony recipe or the native server command.
pub fn symphony_app(command: &str) -> bool {
    starts(command, "just symphony").any(|at| ends(command, at + "just symphony".len()))
        || first_word_is(command, &["symphony-server"])
}

/// The REPL helper server Codex is configured to keep running (`mcp_servers.node_repl` in
/// `~/.codex/config.toml`), by that exact path: no other `node_repl`, and no `node`, is a helper.
pub const CODEX_REPL: &str = "/Applications/ChatGPT.app/Contents/Resources/cua_node/bin/node_repl";

/// `^(?:\S*/)?claude(?:\s|$)|(?:^|[\s/@_-])mcp(?:[-_.\s]|$)`, ignoring case, or [`CODEX_REPL`]:
/// the agent and its helper servers.
pub fn harmless(command: &str) -> bool {
    let lower = command.to_ascii_lowercase();
    // Codex exec jobs are work even when their parent agent is idle.
    if codex_exec(&lower) {
        return false;
    }
    command.split(space).next() == Some(CODEX_REPL)
        || first_word_is(&lower, &["claude", "codex"])
        || starts(&lower, "mcp").any(|at| {
            before(&lower, at).is_none_or(|c| space(c) || "/@_-".contains(c))
                && after(&lower, at + 3).is_none_or(|c| space(c) || "-_.".contains(c))
        })
}

/// `^(?:\S*/)?(?:zsh|bash|sh|dash|fish)(?:\s|$)`
pub fn shell(command: &str) -> bool {
    first_word_is(command, &["zsh", "bash", "sh", "dash", "fish"])
}

/// `^(?:\S*/)?caffeinate(?:\s|$)`: Claude Code keeps the machine awake with it.
pub fn helper(command: &str) -> bool {
    first_word_is(command, &["caffeinate"])
}

/// Only the first positional word can name exec/e; option values and resume prompts are not jobs.
fn codex_exec(lower: &str) -> bool {
    if !first_word_is(lower, &["codex"]) {
        return false;
    }
    // ps flattens argv, which can contain a literal unmatched quote. In that case still read the
    // leading command rather than losing a real exec because of a quote later in its prompt.
    let words = probes::split(lower).unwrap_or_else(|_| {
        lower
            .split(space)
            .filter(|word| !word.is_empty())
            .map(str::to_owned)
            .collect()
    });
    let mut args = words.iter().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            // The delimiter ends subcommand parsing; all following words are prompt text.
            "--" => return false,
            // Codex's global options with a separate value (codex --help). -C is lowercased too.
            "-c"
            | "--config"
            | "-p"
            | "--profile"
            | "-m"
            | "--model"
            | "--cd"
            | "-s"
            | "--sandbox"
            | "-a"
            | "--ask-for-approval"
            | "-i"
            | "--image"
            | "--enable"
            | "--disable"
            | "--remote"
            | "--remote-auth-token-env"
            | "--local-provider"
            | "--add-dir" => {
                let _ = args.next();
            }
            // Flags and options with attached values (--profile=e, -pe) consume no next word.
            option if option.starts_with('-') && option != "-" => {}
            command => return matches!(command, "exec" | "e"),
        }
    }
    false
}

/// A board command or monitor, or the binary they run: `tools/rs-bin.sh`'s build of it, or the link
/// the hook runs.
/// `(?:^|[\s/])just\s+tasks(?:-attach)?(?:\s|$)|(?:^|[\s/])tools/tasks\.py(?:\s|$)`
/// `|(?:^|/)(?:(?:sf-cargo/bin|sf-bin)/tasks-[0-9a-f]{16}|\.target/bin/tasks)(?:\s|$)`
pub fn board(command: &str) -> bool {
    let after_word = |at: usize| before(command, at).is_none_or(|c| space(c) || c == '/');
    let after_slash = |at: usize| before(command, at).is_none_or(|c| c == '/');
    let just_tasks = starts(command, "just").any(|at| {
        let rest = &command[at + 4..];
        let tasks = rest.trim_start_matches(space);
        if !after_word(at) || tasks.len() == rest.len() || !tasks.starts_with("tasks") {
            return false;
        }
        let end = command.len() - tasks.len() + 5;
        ends(command, end) || (command[end..].starts_with("-attach") && ends(command, end + 7))
    });
    let script =
        starts(command, "tools/tasks.py").any(|at| after_word(at) && ends(command, at + 14));
    let built = ["sf-cargo/bin/tasks-", "sf-bin/tasks-"]
        .iter()
        .any(|prefix| {
            starts(command, prefix).any(|at| {
                let hash = &command.as_bytes()[at + prefix.len()..];
                after_slash(at)
                    && hash.len() >= 16
                    && hash[..16]
                        .iter()
                        .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
                    && ends(command, at + prefix.len() + 16)
            })
        });
    let linked =
        starts(command, ".target/bin/tasks").any(|at| after_slash(at) && ends(command, at + 17));
    just_tasks || script || built || linked
}

/// The `-n HANDLE` (or `--name HANDLE`) a command line names first:
/// `(?:^|\s)(?:-n|--name)\s+(\S+)`.
pub fn named(command: &str) -> Option<&str> {
    command.char_indices().find_map(|(at, _)| {
        if !before(command, at).is_none_or(space) {
            return None;
        }
        let rest = &command[at..];
        let flag = ["-n", "--name"].iter().find_map(|flag| {
            let value = rest.strip_prefix(flag)?;
            let trimmed = value.trim_start_matches(space);
            (trimmed.len() < value.len()).then_some(trimmed)
        })?;
        let name = flag.split(space).next().unwrap_or("");
        (!name.is_empty()).then_some(name)
    })
}

/// `\s` as Python's `re` reads it in a str pattern.
pub fn space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

/// `\w` as Python's `re` reads it in a str pattern.
fn word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `^(?:\S*/)?NAME(?:\s|$)` for each of `names`: the first word is the name, or a path ending in it.
fn first_word_is(command: &str, names: &[&str]) -> bool {
    let first = command.split(space).next().unwrap_or("");
    names.iter().any(|name| {
        first == *name
            || first
                .strip_suffix(name)
                .is_some_and(|path| path.ends_with('/'))
    })
}

/// `(?:^|[\s/])NAME(?:SUFFIX)?(?:\s|$)`.
fn word_with(command: &str, name: &str, suffix: &str) -> bool {
    starts(command, name).any(|at| {
        let end = at + name.len();
        before(command, at).is_none_or(|c| space(c) || c == '/')
            && (ends(command, end)
                || (command[end..].starts_with(suffix) && ends(command, end + suffix.len())))
    })
}

/// Each place in `text` where `needle` starts, overlapping or not.
fn starts<'t>(text: &'t str, needle: &'t str) -> impl Iterator<Item = usize> + 't {
    text.char_indices()
        .map(|(at, _)| at)
        .filter(move |&at| text[at..].starts_with(needle))
}

fn before(text: &str, at: usize) -> Option<char> {
    text[..at].chars().next_back()
}

fn after(text: &str, at: usize) -> Option<char> {
    text[at..].chars().next()
}

/// `(?:\s|$)` at `at`.
fn ends(text: &str, at: usize) -> bool {
    after(text, at).is_none_or(space)
}
