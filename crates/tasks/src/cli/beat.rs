//! `just tasks beat`: the hook that turns an agent's tool calls into its heartbeat (Claude Code's
//! PreToolUse on Bash and PostToolUse on every tool, `symphony hook tasks-beat`). It runs around
//! every tool call of every session in the repository, so it prints nothing, never fails, never
//! starts the board's server and waits on it at most `WAIT`: any trouble, and the call goes on.

use super::Env;
use crate::git::common_dir;
use crate::server::FOLDER;
use crate::sys::Boottime;
use crate::{Beat, Error, Store};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Claude Code's default Bash timeout, in seconds.
pub const BASH_TIMEOUT: f64 = 120.0;
/// And the longest a call may ask for.
pub const BASH_TIMEOUT_MAX: f64 = 600.0;
/// How long a beat waits on the board before it gives up.
pub const WAIT: Duration = Duration::from_secs(2);

/// What separates commands on a line, as `shlex` with `punctuation_chars` reads them.
const PUNCTUATION: &str = "();<>|&";

/// `command` split as Python's `shlex.shlex(command, posix=False, punctuation_chars=True)` splits
/// it with `whitespace_split`: a quoted string at a token's start is one token, quotes and all, and
/// ends there; a run of `();<>|&` is a token; `#` skips the rest of its line. None: a quote that does
/// not close, which no command can be read from.
pub fn shell_tokens(command: &str) -> Option<Vec<String>> {
    const WHITESPACE: &str = " \t\r\n";
    const WORDCHARS: &str =
        "abcdfeghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_~-./*?=";
    let mut rest = command.chars();
    // shlex's `_pushback_chars`: a character read too far, read again first.
    let mut pushback: Option<char> = None;
    let mut tokens = Vec::new();
    loop {
        let mut token = String::new();
        let mut state = ' ';
        loop {
            let next = pushback.take().or_else(|| rest.next());
            let Some(c) = next else {
                if state == '"' || state == '\'' {
                    return None; // "No closing quotation"
                }
                break;
            };
            match state {
                ' ' => {
                    if WHITESPACE.contains(c) {
                        continue;
                    } else if c == '#' {
                        rest.by_ref().find(|&c| c == '\n');
                    } else if WORDCHARS.contains(c) {
                        token.push(c);
                        state = 'a';
                    } else if PUNCTUATION.contains(c) {
                        token.push(c);
                        state = 'c';
                    } else if c == '"' || c == '\'' {
                        token.push(c);
                        state = c;
                    } else {
                        token.push(c);
                        state = 'a';
                    }
                }
                '"' | '\'' => {
                    token.push(c);
                    if c == state {
                        break; // without POSIX, the quote ends the token
                    }
                }
                _ => {
                    if WHITESPACE.contains(c) {
                        break;
                    } else if c == '#' {
                        // Without POSIX, the rest of the line goes and the token goes on.
                        rest.by_ref().find(|&c| c == '\n');
                    } else if state == 'c' {
                        if PUNCTUATION.contains(c) {
                            token.push(c);
                        } else {
                            pushback = Some(c);
                            break;
                        }
                    } else if PUNCTUATION.contains(c) {
                        pushback = Some(c);
                        break;
                    } else {
                        token.push(c);
                    }
                }
            }
        }
        if token.is_empty() {
            return Some(tokens);
        }
        tokens.push(token);
    }
}

/// `[a-z]+-[a-z]+(-\d+)?`, the whole of `text`.
fn is_handle(text: &str) -> bool {
    let lower = |part: &str| !part.is_empty() && part.chars().all(|c| c.is_ascii_lowercase());
    match text.split('-').collect::<Vec<_>>().as_slice() {
        [a, b] => lower(a) && lower(b),
        [a, b, n] => lower(a) && lower(b) && !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()),
        _ => false,
    }
}

/// `NAME=…` with NAME `[A-Za-z_][A-Za-z0-9_]*`: a shell assignment before a command.
fn is_assignment(token: &str) -> bool {
    let Some((name, _)) = token.split_once('=') else {
        return false;
    };
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The handle a shell command acts as on the board, or None. Only a board command names one:
/// `just tasks …`, or `just` with any of `--justfile PATH`, `-f PATH`, `--working-directory DIR` and
/// `-d DIR` (each also as `--justfile=PATH`, `-fPATH` and so on) before `tasks`. It names one by its
/// own `--as H` flag, else by a leading `TASKS_SESSION=H` on that same command. A
/// handle anywhere else (a grep pattern, another recipe run as someone, text inside a quoted
/// argument) names nothing. Of several board commands on a line, the last that names one.
pub fn named_handle(command: &str) -> Option<String> {
    let mut found = None;
    let mut segment: Vec<String> = Vec::new();
    for token in shell_tokens(command)?.into_iter().chain([";".to_string()]) {
        if !token.is_empty() && token.chars().all(|c| PUNCTUATION.contains(c)) {
            found = segment_handle(&segment).or(found);
            segment.clear();
        } else {
            segment.push(token);
        }
    }
    found
}

fn segment_handle(tokens: &[String]) -> Option<String> {
    let mut prefix = None;
    let mut i = 0;
    while i < tokens.len() && is_assignment(&tokens[i]) {
        if let Some(("TASKS_SESSION", value)) = tokens[i].split_once('=') {
            prefix = is_handle(value).then(|| value.to_string());
        }
        i += 1;
    }
    if tokens.get(i).map(String::as_str) != Some("just") {
        return None;
    }
    i += 1;
    // A worktree may use another checkout's justfile while keeping its own hook identity.
    while let Some(option) = tokens.get(i).map(String::as_str) {
        match option {
            "--justfile" | "-f" | "--working-directory" | "-d" => {
                tokens.get(i + 1)?;
                i += 2;
            }
            _ if option.starts_with("--justfile=")
                || option.starts_with("--working-directory=")
                || (option.starts_with("-f") || option.starts_with("-d")) && option.len() > 2 =>
            {
                i += 1;
            }
            _ => break,
        }
    }
    if tokens.get(i).map(String::as_str) != Some("tasks") {
        return None;
    }
    let args = &tokens[i + 1..];
    let mut flag = None;
    for (j, token) in args.iter().enumerate() {
        // A quoted argument keeps its quotes here, so it never reads as the flag.
        if token == "--as" && args.get(j + 1).is_some_and(|h| is_handle(h)) {
            flag = Some(args[j + 1].clone());
        } else if let Some(value) = token.strip_prefix("--as=").filter(|v| is_handle(v)) {
            flag = Some(value.to_string());
        }
    }
    flag.or(prefix)
}

/// Python's truth of a JSON value.
fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(yes)) => *yes,
        Some(Value::Number(n)) => n.as_f64() != Some(0.0),
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(fields)) => !fields.is_empty(),
    }
}

/// One hook event, as a heartbeat for the session bound to it. The key is the event's `agent_id` (a
/// subagent), else its `session_id` (a full session with no monitor, as one started in an hc terminal);
/// `bind_agent` never binds a handle with a live monitor, which beats on its own. A foreground Bash
/// call marks the session busy for as long as it may run, so a long `verify-full` reads live; its
/// PostToolUse clears that call's mark. Each tool call of a bound subagent also records, on its
/// binding, the checkout the call runs in (the event's `cwd`), and when it came there, by the board's
/// clock and by `boot`, the clock since boot as the hook read it: where the agents view looks for
/// what it left running once it goes quiet, and which of what runs there began while it was there.
/// The key, and whether a full session is bound by it now (None for a subagent's, or an event with
/// no key).
pub fn beat_event(
    store: &Store,
    event: &Map<String, Value>,
    boot: Option<&Boottime>,
) -> Result<Option<(String, bool)>, Error> {
    let by_agent = truthy(event.get("agent_id"));
    let key = if by_agent {
        event.get("agent_id")
    } else {
        event.get("session_id").filter(|v| truthy(Some(v)))
    };
    let Some(agent) = key.and_then(Value::as_str) else {
        return Ok(None);
    };
    let full = |bound: Option<String>| (!by_agent).then(|| (agent.to_string(), bound.is_some()));
    // A subagent's checkout is where its own calls run, which its session (registered where its
    // orchestrator ran `register`) does not say: each of its calls says it again, so one that moved
    // is looked for where it went.
    let checkout = by_agent
        .then(|| event.get("cwd").and_then(Value::as_str))
        .flatten()
        .filter(|cwd| !cwd.is_empty())
        .and_then(|cwd| crate::git::checkout(Path::new(cwd)));
    let place = checkout
        .as_ref()
        .map(|(top, branch)| (top.to_str().unwrap_or_default(), branch.as_str()))
        .filter(|(top, _)| !top.is_empty());
    let boot = boot.filter(|_| by_agent);
    let empty = Map::new();
    let tool = event
        .get("tool_input")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    if event.get("tool_name").and_then(Value::as_str) != Some("Bash") {
        return Ok(full(store.agent_beat_in(
            agent,
            &Beat::default(),
            place,
            boot,
        )?));
    }
    let command = tool.get("command").and_then(Value::as_str).unwrap_or("");
    if let Some(named) = named_handle(command) {
        store.bind_agent(agent, &named, !by_agent)?;
    }
    let call = event
        .get("tool_use_id")
        .and_then(Value::as_str)
        .map(str::to_string);
    let beat = if event.get("hook_event_name").and_then(Value::as_str) != Some("PreToolUse") {
        Beat {
            busy_for: None,
            call,
            ended: true,
        }
    } else if truthy(tool.get("run_in_background")) {
        Beat::default()
    } else {
        let seconds = tool
            .get("timeout")
            .and_then(Value::as_f64)
            .filter(|ms| *ms > 0.0)
            .map_or(BASH_TIMEOUT, |ms| ms / 1000.0);
        Beat {
            busy_for: Some(seconds.min(BASH_TIMEOUT_MAX)),
            call,
            ended: false,
        }
    };
    Ok(full(store.agent_beat_in(agent, &beat, place, boot)?))
}

/// The bound keys' folder (the heartbeat hook's `keys_folder`): `TASKS_KEYS`; for a board
/// `TASKS_SERVER` names without it, none; else `sf-tasks/keys/` in the git common dir of `cwd`.
pub(crate) fn keys_folder(env: &Env, cwd: &Path) -> Option<PathBuf> {
    if let Some(keys) = env.var("TASKS_KEYS").filter(|keys| !keys.is_empty()) {
        return Some(keys.into());
    }
    if env.var("TASKS_SERVER").is_some() {
        return None;
    }
    let common = common_dir(cwd).ok()?;
    Some(common.join(FOLDER).join("keys"))
}

/// Whether `key` can name a file in the bound keys' folder: letters, digits, `_` and `-`, as every
/// session id is.
fn safe(key: &str) -> bool {
    !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Mark `key` bound or not in the bound keys' folder, which the hook looks in before it runs this.
fn mark(folder: &Path, key: &str, bound: bool) {
    if !safe(key) {
        return;
    }
    let file = folder.join(key);
    if bound {
        if !file.exists() && std::fs::create_dir_all(folder).is_ok() {
            let _ = std::fs::write(&file, "");
        }
    } else {
        let _ = std::fs::remove_file(&file);
    }
}

/// Mark each of `keys` bound and no other: the folder as the move to SurrealDB finds the board
/// (`crate::cutover`).
pub(crate) fn mark_exactly(folder: &Path, keys: &[String]) -> std::io::Result<()> {
    std::fs::create_dir_all(folder)?;
    for entry in std::fs::read_dir(folder)? {
        let entry = entry?;
        if !keys.iter().any(|key| entry.file_name() == key.as_str()) {
            std::fs::remove_file(entry.path())?;
        }
    }
    for key in keys.iter().filter(|key| safe(key)) {
        std::fs::write(folder.join(key), "")?;
    }
    Ok(())
}

/// `just tasks beat < EVENT.json`: always 0.
pub fn run(input: &mut dyn std::io::Read, env: &Env) -> i32 {
    let _ = (|| -> Option<()> {
        let mut raw = String::new();
        input.read_to_string(&mut raw).ok()?;
        let event: Value = serde_json::from_str(if raw.is_empty() { "null" } else { &raw }).ok()?;
        let event = event.as_object()?;
        if !truthy(event.get("agent_id")) && !truthy(event.get("session_id")) {
            return None; // no key to be bound by: nothing to open
        }
        let cwd = event
            .get("cwd")
            .and_then(Value::as_str)
            .filter(|cwd| !cwd.is_empty())
            .map_or_else(|| env.cwd.clone(), PathBuf::from);
        let reached = super::reach(env, &cwd, false).ok()?;
        let reached = super::Reached {
            client: reached.client.waiting(WAIT),
            ..reached
        };
        let store = super::open_unchecked(reached, &super::Clock::of(env).ok()?);
        let boot = super::boot_clock(env);
        let (key, bound) = beat_event(&store, event, boot.as_ref()).ok()??;
        mark(&keys_folder(env, &cwd)?, &key, bound);
        Some(())
    })();
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(command: &str) -> Option<Vec<String>> {
        shell_tokens(command)
    }

    fn owned(split: &[&str]) -> Option<Vec<String>> {
        Some(split.iter().map(|t| t.to_string()).collect())
    }

    /// The move writes the folder as it finds the board: what was marked before goes, and a key that
    /// names a path is never written as one.
    #[test]
    fn marking_exactly_leaves_the_keys_given_and_no_other_and_skips_unsafe_ones() {
        let dir = std::env::temp_dir().join(format!("symphony-keys-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let keys = dir.join("keys");
        mark_exactly(&keys, &["old".into()]).unwrap();
        mark_exactly(&keys, &["a-1".into(), "b_2".into(), "../escape".into()]).unwrap();
        let mut names: Vec<String> = std::fs::read_dir(&keys)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["a-1", "b_2"]);
        assert!(!dir.join("escape").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_board_a_test_names_has_no_keys_folder_unless_it_names_one() {
        let named = |vars: &[(&str, &str)]| Env {
            vars: vars
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            cwd: std::env::temp_dir(),
        };
        assert_eq!(
            keys_folder(&named(&[("TASKS_SERVER", "127.0.0.1:1")]), Path::new("/")),
            None
        );
        assert_eq!(
            keys_folder(
                &named(&[("TASKS_SERVER", "127.0.0.1:1"), ("TASKS_KEYS", "/k")]),
                Path::new("/")
            ),
            Some(PathBuf::from("/k"))
        );
    }

    #[test]
    fn a_line_splits_as_pythons_shlex_splits_it() {
        for (command, split) in [
            (
                "cd /r && TASKS_SESSION=amber-heron just tasks inbox",
                &[
                    "cd",
                    "/r",
                    "&&",
                    "TASKS_SESSION=amber-heron",
                    "just",
                    "tasks",
                    "inbox",
                ][..],
            ),
            ("x;y&&z|w", &["x", ";", "y", "&&", "z", "|", "w"]),
            ("(a)", &["(", "a", ")"]),
            ("a>b", &["a", ">", "b"]),
            ("a\tb\r\nc", &["a", "b", "c"]),
            ("ü ñ", &["ü", "ñ"]),
            ("", &[]),
        ] {
            assert_eq!(tokens(command), owned(split), "{command:?}");
        }
    }

    #[test]
    fn a_quote_at_a_tokens_start_keeps_its_quotes_and_ends_the_token() {
        for (command, split) in [
            ("a \"b c\"d e", &["a", "\"b c\"", "d", "e"][..]),
            ("a'b c'", &["a'b", "c'"]),
            ("echo 'it''s'", &["echo", "'it'", "'s'"]),
            ("a=b\"c d\"", &["a=b\"c", "d\""]),
            ("\"q\"x", &["\"q\"", "x"]),
            ("a \"\" b", &["a", "\"\"", "b"]),
            ("a\\ b", &["a\\", "b"]),
        ] {
            assert_eq!(tokens(command), owned(split), "{command:?}");
        }
        assert_eq!(tokens("\"unclosed"), None);
    }

    #[test]
    fn a_hash_skips_the_rest_of_its_line_even_inside_a_word() {
        for (command, split) in [
            (
                "just tasks create x y --link docs/X.md#H --as amber-heron",
                &["just", "tasks", "create", "x", "y", "--link", "docs/X.md"][..],
            ),
            ("a #c\nb", &["a", "b"]),
            ("a#c\nb c", &["ab", "c"]),
            ("#only", &[]),
        ] {
            assert_eq!(tokens(command), owned(split), "{command:?}");
        }
    }

    #[test]
    fn a_dollar_starts_a_word_and_punctuation_ends_it() {
        assert_eq!(
            tokens("$(just tasks --as x y)"),
            owned(&["$", "(", "just", "tasks", "--as", "x", "y", ")"])
        );
        assert_eq!(tokens("a=$(b)"), owned(&["a=$", "(", "b", ")"]));
    }

    #[test]
    fn a_board_command_names_its_handle_by_its_leading_prefix_or_its_own_flag() {
        for command in [
            "TASKS_SESSION=amber-heron just tasks inbox",
            "TASKS_SESSION=amber-heron FOO=1 just tasks claim sim/a",
            "cd /repo && TASKS_SESSION=amber-heron just tasks inbox",
            "just tasks --as amber-heron say sim/a hi",
            "just tasks say sim/a hi --as amber-heron",
            "just tasks --as=amber-heron inbox",
            "TASKS_SESSION=amber-heron just --justfile /main/justfile tasks inbox",
            "just --justfile '/main folder/justfile' tasks inbox --as amber-heron",
            "just -f /main/justfile tasks inbox --as amber-heron",
            "just --justfile=/main/justfile tasks inbox --as amber-heron",
            "just -f/main/justfile tasks inbox --as amber-heron",
            "just -f /main/justfile -d /worktree tasks inbox --as amber-heron",
            "just --working-directory=/worktree --justfile=/main/justfile tasks inbox --as amber-heron",
            "just -d/worktree -f/main/justfile tasks inbox --as amber-heron",
        ] {
            assert_eq!(
                named_handle(command).as_deref(),
                Some("amber-heron"),
                "{command}"
            );
        }
        assert_eq!(
            named_handle("TASKS_SESSION=amber-heron-12 just tasks inbox").as_deref(),
            Some("amber-heron-12")
        );
    }

    #[test]
    fn the_flag_wins_over_the_prefix_as_the_board_reads_it() {
        assert_eq!(
            named_handle("TASKS_SESSION=amber-heron just tasks --as slate-owl inbox").as_deref(),
            Some("slate-owl")
        );
    }

    #[test]
    fn the_last_board_command_on_a_line_names_it() {
        assert_eq!(
            named_handle(
                "TASKS_SESSION=amber-heron just tasks inbox; just tasks --as slate-owl show x"
            )
            .as_deref(),
            Some("slate-owl")
        );
        assert_eq!(
            named_handle("TASKS_SESSION=amber-heron just tasks inbox && ls").as_deref(),
            Some("amber-heron")
        );
    }

    #[test]
    fn a_handle_only_mentioned_names_nothing() {
        for command in [
            "grep 'TASKS_SESSION=amber-heron' artifacts/x.log",
            "grep TASKS_SESSION=amber-heron artifacts/x.log",
            "echo TASKS_SESSION=amber-heron just tasks inbox",
            "TASKS_SESSION=amber-heron just land tools/x",
            "TASKS_SESSION=amber-heron just tasks-attach --resume amber-heron",
            "just tasks say sim/a 'try --as amber-heron'",
            "just tasks say sim/a \"use TASKS_SESSION=amber-heron\"",
            "just check",
            "unbalanced 'quote TASKS_SESSION=amber-heron just tasks inbox",
            "TASKS_SESSION=Amber-Heron just tasks inbox",
            "just tasks --as amber inbox",
            "TASKS_SESSION=amber-heron just --justfile tasks",
            "TASKS_SESSION=amber-heron just --justfile",
            "TASKS_SESSION=amber-heron just --justfile /main/justfile land tools/x",
            "TASKS_SESSION=amber-heron just --command tasks inbox",
            "just --justfile '/main/justfile tasks --as amber-heron' check",
            "",
        ] {
            assert_eq!(named_handle(command), None, "{command}");
        }
    }

    #[test]
    fn a_quoted_flag_inside_a_message_is_text_not_a_flag() {
        assert_eq!(
            named_handle("just tasks say sim/a \"see --as slate-owl above\" --as amber-heron")
                .as_deref(),
            Some("amber-heron")
        );
    }

    #[test]
    fn a_prefix_that_is_not_a_handle_unsets_an_earlier_one() {
        assert_eq!(
            named_handle("TASKS_SESSION=amber-heron TASKS_SESSION=x just tasks inbox"),
            None
        );
    }
}
