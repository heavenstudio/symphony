//! `tasks beat`: the hook that turns an agent's tool calls into its heartbeat (Claude Code's
//! PreToolUse on Bash and PostToolUse on every tool, `.target/bin/symphony hook tasks-beat`, which
//! runs this binary's `beat` directly, not through `just`, so unmarked: `just tasks beat` binds
//! nothing, as [`beat_event`] says). It runs around
//! every tool call of every session in the repository, so it prints nothing, never fails, never
//! starts the board's server and waits on it at most `WAIT`: any trouble, and the call goes on.
//! Each beat hands the board the tokens the session's own transcript gained since the key's last
//! beat ([`crate::usage::transcript`]), read only for the hook's own event.

use super::Env;
use crate::git::common_dir;
use crate::server::FOLDER;
use crate::sys::Boottime;
use crate::usage::Spent;
use crate::usage::transcript::{self, Ledger};
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
/// Only with `bind` does a board command in the call bind its key: not for a beat run through `just
/// tasks`, whose event an agent wrote, not its own tool call ([`crate::launch::may_bind`]).
/// `spent`, the tokens the session's model calls took since its last beat, is added to the handle
/// bound to the key in the beat's one transaction ([`Store::agent_beat_spending`]).
/// What came of it ([`Beaten`]); none for an event with no key.
pub fn beat_event(
    store: &Store,
    event: &Map<String, Value>,
    boot: Option<&Boottime>,
    bind: bool,
    spent: &[Spent],
) -> Result<Option<Beaten>, Error> {
    let Some((agent, by_agent)) = key_of(event) else {
        return Ok(None);
    };
    let full = |bound: Option<String>| {
        Some(Beaten {
            key: agent.to_string(),
            full: !by_agent,
            handle: bound,
        })
    };
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
        return Ok(full(store.agent_beat_spending(
            agent,
            &Beat::default(),
            place,
            boot,
            spent,
        )?));
    }
    let command = tool.get("command").and_then(Value::as_str).unwrap_or("");
    if let Some(named) = named_handle(command).filter(|_| bind) {
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
    Ok(full(
        store.agent_beat_spending(agent, &beat, place, boot, spent)?,
    ))
}

/// What one beat came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Beaten {
    /// The key it beat by: the agent id of a subagent, else the full session's id.
    pub key: String,
    /// The key is a full session's, which the bound keys' folder lists while it is bound.
    pub full: bool,
    /// The handle bound to the key, which the beat beat for and gave the tokens to; none when
    /// nobody is.
    pub handle: Option<String>,
}

/// The key `event` beats by, and whether it is a subagent's: its `agent_id`, else its `session_id`.
fn key_of(event: &Map<String, Value>) -> Option<(&str, bool)> {
    let by_agent = truthy(event.get("agent_id"));
    let key = if by_agent {
        event.get("agent_id")
    } else {
        event.get("session_id").filter(|v| truthy(Some(v)))
    };
    key.and_then(Value::as_str).map(|key| (key, by_agent))
}

/// The usage folder of the board `env` reaches from `cwd` ([`crate::usage::transcript`]):
/// `TASKS_USAGE`; for a board `TASKS_SERVER` names (a test's) without it, none; else
/// `symphony/tasks/usage/` in the git common dir of `cwd`, beside the bound keys.
pub(crate) fn usage_folder(env: &Env, cwd: &Path) -> Option<PathBuf> {
    if let Some(folder) = env.var("TASKS_USAGE").filter(|folder| !folder.is_empty()) {
        return Some(folder.into());
    }
    if env.var("TASKS_SERVER").is_some() {
        return None;
    }
    let common = common_dir(cwd).ok()?;
    Some(common.join(FOLDER).join(transcript::FOLDER))
}

/// The bound keys' folder (the heartbeat hook's `keys_folder`): `TASKS_KEYS`; for a board
/// `TASKS_SERVER` names without it, none; else `symphony/tasks/keys/` in the git common dir of
/// `cwd`.
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
        if let Some(seen) = considered(folder, key) {
            let _ = std::fs::remove_file(seen);
        }
    }
}

/// The variable Claude Code gives a session's own processes, its hooks among them: the socket its
/// inbox listens on, which `callback` registers with no address.
const SOCKET: &str = "CLAUDE_CODE_MESSAGING_SOCKET";

/// The file beside a full session's key that names the socket last considered for it
/// ([`connect`]): `KEY.channel`, which no key can be named (a key holds no dot).
fn considered(folder: &Path, key: &str) -> Option<PathBuf> {
    safe(key).then(|| folder.join(format!("{key}.channel")))
}

/// Register the channel of a launched session that has not registered it: `launch` and `restore`
/// leave that to the agent (`just tasks callback --as HANDLE`), and one that skips it sits idle on
/// messages nobody delivers. On the hook's own event before a board command that names the handle
/// bound to the session, where the session's conversation is the one an open launch of that
/// handle records and its environment names its inbox socket, the socket is registered as
/// `callback` with no address would have: once for each socket, so a channel the session turns
/// off stays off, and a restored conversation, a new process on a new socket, is connected again.
/// A registration another session holds the chat seat by is left alone. Whatever happens the beat
/// goes on: a socket that cannot be registered now, or whose session no open launch records yet
/// (a board command run before its launch or restore writes the record), is tried at the
/// session's next board command; so a session the board never launched reads its launches at
/// each.
fn connect(
    store: &Store,
    env: &Env,
    event: &Map<String, Value>,
    beaten: &Beaten,
    folder: Option<&Path>,
) -> Option<()> {
    let handle = beaten.handle.as_deref().filter(|_| beaten.full)?;
    let socket = env.var(SOCKET).filter(|socket| !socket.is_empty())?;
    let text = |name: &str| event.get(name).and_then(Value::as_str);
    if text("hook_event_name") != Some("PreToolUse") || text("tool_name") != Some("Bash") {
        return None;
    }
    let command = event.get("tool_input")?.get("command")?.as_str()?;
    if named_handle(command).as_deref() != Some(handle) {
        return None;
    }
    let seen = folder.and_then(|folder| considered(folder, &beaten.key));
    if let Some(seen) = &seen
        && std::fs::read_to_string(seen).is_ok_and(|last| last == socket)
    {
        return None;
    }
    let launched = store
        .sessions_of(handle)
        .ok()?
        .iter()
        .any(|launch| launch.closed.is_none() && launch.claude_session == beaten.key);
    // Not yet launched, as a command run before its launch writes the record is: tried again.
    if !launched {
        return None;
    }
    let address = format!("uds:{socket}");
    let held = store.callbacks().ok()?.remove(handle);
    if held
        .as_ref()
        .is_none_or(|held| held.address != address && !held.chat)
    {
        store
            .callback_in(
                handle,
                Some(&address),
                false,
                false,
                env.var("HC_TERMINAL_ID"),
            )
            .ok()?;
    }
    if let Some(seen) = seen
        && seen
            .parent()
            .is_some_and(|folder| std::fs::create_dir_all(folder).is_ok())
    {
        let _ = std::fs::write(seen, socket);
    }
    Some(())
}

/// `just tasks beat < EVENT.json`: always 0. A channel it registers ([`connect`]) leaves its
/// delivery to a detached `program dispatch`, as every command line's write does, so the tool call
/// waits on no delivery.
pub fn run(input: &mut dyn std::io::Read, env: &Env, program: Option<&Path>) -> i32 {
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
        let store = super::open_beating(reached, &super::Clock::of(env).ok()?).ok()?;
        let boot = super::boot_clock(env);
        let bind = env.var(crate::confine::MARKER).is_none();
        // Only the hook's own event reads a transcript: one an agent wrote names any file.
        let ledger = key_of(event)
            .filter(|_| bind)
            .zip(transcript::of_event(event))
            .zip(usage_folder(env, &cwd))
            .and_then(|(((key, _), path), folder)| Ledger::open(&folder, key, &path));
        let spent = ledger.as_ref().map_or(&[][..], Ledger::spent);
        let beaten = beat_event(&store, event, boot.as_ref(), bind, spent);
        let recorded = matches!(
            &beaten,
            Ok(Some(Beaten {
                handle: Some(_),
                ..
            }))
        );
        if let Some(ledger) = ledger {
            ledger.close(recorded);
        }
        let beaten = beaten.ok()??;
        let folder = keys_folder(env, &cwd);
        if bind {
            // The dispatcher reaches the board from where the beat did: the event's folder.
            let here = Env {
                vars: env.vars.clone(),
                cwd: cwd.clone(),
            };
            let store = super::detaching(store, &here, program);
            connect(&store, env, event, &beaten, folder.as_deref());
        }
        if beaten.full {
            mark(&folder?, &beaten.key, beaten.handle.is_some());
        }
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
