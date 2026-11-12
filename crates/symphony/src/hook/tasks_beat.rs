//! `PreToolUse` (Bash) and `PostToolUse` hook: a subagent's tool calls are its heartbeat on the task board.
//!
//! It runs around every tool call of every session in this project, so it is built to cost nothing
//! where it has nothing to do: an event with neither `agent_id` nor `session_id` returns before
//! anything is read, and so does a full session's call (no `agent_id`: an orchestrator's, the
//! owner's) whose key is not bound and that is not a board command, which may bind it: bound is a
//! file in the bound keys' folder ([`bound`]), a look at one path. Any other event goes whole to
//! the `tasks` binary's `beat` (`crates/tasks/src/cli/beat.rs`), which beats only for a handle
//! bound to that key: a subagent by its agent id, a full session with no monitor (one started in an
//! hc terminal) by its session id; a session with a monitor beats through it and is never bound.
//! The beat is one call to the board's server, which it never starts, and waits on it at most two
//! seconds.
//!
//! The binary is `TASKS_BIN`, else the build this tree last ran, `.target/bin/tasks` under the project
//! (`SYMPHONY_HOOK_ROOT`, else `CLAUDE_PROJECT_DIR`).
//! With none, the board's filesystem-only project lookup finds it from the event's cwd.
//! A checkout with no link of its own (a worktree whose sessions run main's justfile,
//! `just --justfile MAIN/justfile tasks …`, or a project below its checkout's top) beats on its main
//! checkout's link, as the recipes find a build ([`crate::builds`]), and makes no link. The hook
//! hashes no sources and builds nothing, since a build would hold up the tool call for minutes: the
//! project's build setup makes the links.
//! The hook prints nothing and always exits 0: a heartbeat must never cost an agent its tool call. It
//! stops a beat that has held the call for [`LIMIT`] ([`Limit`]).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use symphony_board::sys::{Command, Stdio};

use serde_json::Value;

use super::event::{self, py_str, truthy};
use symphony_board::git::checkout_common_dir;

/// The bound keys' folder in the git common dir: one empty file per key a full session is bound by. The
/// binary's beat writes it when it binds one and removes it when it finds nobody bound
/// (`crates/tasks/src/cli/beat.rs`).
const KEYS: [&str; 2] = ["symphony/tasks", "keys"];

fn set<'a>(env: &'a HashMap<String, String>, name: &str) -> Option<&'a str> {
    env.get(name)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
}

/// Whether `key` can name a file in the keys' folder: letters, digits, `_` and `-`, as every agent and
/// session id is.
fn safe(key: &str) -> bool {
    !key.is_empty()
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// The bound keys' folder of the board `env` reaches from `cwd`: `TASKS_KEYS`; for a board
/// `TASKS_SERVER` names (a test's) without it, none; else `symphony/tasks/keys/` in the git common
/// dir, beside the board's server.
fn keys_folder(env: &HashMap<String, String>, cwd: &str) -> Option<PathBuf> {
    if let Some(keys) = set(env, "TASKS_KEYS") {
        return Some(PathBuf::from(keys));
    }
    if set(env, "TASKS_SERVER").is_some() {
        return None;
    }
    let mut folder = symphony_board::project::locate_quick(Path::new(cwd))?.common;
    folder.extend(KEYS);
    Some(folder)
}

/// Whether `key` is bound: None when this look cannot tell (no folder to look in, a key no file can
/// name), and the binary decides.
fn bound(env: &HashMap<String, String>, cwd: &str, key: &str) -> Option<bool> {
    let folder = keys_folder(env, cwd)?;
    if !safe(key) {
        return None;
    }
    Some(folder.join(key).exists())
}

/// Whether the event goes to the board.
fn wanted(raw: &[u8], env: &HashMap<String, String>) -> bool {
    let has = |needle: &[u8]| raw.windows(needle.len()).any(|window| window == needle);
    if !has(b"\"agent_id\"") && !has(b"\"session_id\"") {
        return false;
    }
    let Some(Value::Object(event)) = event::parse(raw) else {
        return false;
    };
    let key = |name: &str| event.get(name).is_some_and(truthy);
    if !key("agent_id") && !key("session_id") {
        return false;
    }
    if key("agent_id") {
        return true;
    }
    // A full session: only a board command may bind it, and only a bound key beats.
    let command = match event.get("tool_input").filter(|input| truthy(input)) {
        None => String::new(),
        Some(Value::Object(input)) => input.get("command").map_or_else(String::new, py_str),
        Some(_) => return false, // a `tool_input` that is no mapping beats nothing
    };
    if command.contains("tasks") {
        return true;
    }
    let cwd = match event.get("cwd").filter(|cwd| truthy(cwd)) {
        Some(cwd) => py_str(cwd),
        None => match std::env::current_dir() {
            Ok(here) => here.to_string_lossy().into_owned(),
            Err(_) => return true, // no folder to look from: the binary decides
        },
    };
    let session = event.get("session_id").map_or_else(String::new, py_str);
    bound(env, &cwd, &session) != Some(false)
}

/// The board's binary: `TASKS_BIN`, else the project's `.target/bin/tasks` while it names a build, else
/// its main checkout's ([`main_link`]).
fn binary(env: &HashMap<String, String>, raw: &[u8]) -> Option<PathBuf> {
    if let Some(named) = set(env, "TASKS_BIN") {
        return Some(PathBuf::from(named));
    }
    let fallback = || {
        let event = event::parse(raw)?;
        let cwd = event
            .get("cwd")
            .and_then(Value::as_str)
            .filter(|cwd| !cwd.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::current_dir().ok())?;
        symphony_board::project::locate_quick(&cwd)
    };
    let explicit = set(env, symphony_env::HOOK_ROOT.name)
        .or_else(|| set(env, "CLAUDE_PROJECT_DIR"))
        .map(PathBuf::from);
    // An explicit root is a checkout boundary. A located manifest may be below that boundary;
    // retain the common directory the board lookup already resolved for it.
    let (root, common) = match explicit {
        Some(root) => {
            let common = if root.join(symphony_board::project::FILE).is_file() {
                symphony_board::project::locate_quick(&root).map(|place| place.common)
            } else {
                checkout_common_dir(&root)
            };
            (root, common)
        }
        None => {
            let place = fallback()?;
            (place.root, Some(place.common))
        }
    };
    let link = root.join(crate::builds::LINKS).join(TASKS);
    if link.exists() {
        return Some(link);
    }
    main_link(&common?)
}

/// The board's binary.
const TASKS: &str = "tasks";

/// The main checkout's link to its build of the board's binary, when the repository whose common dir
/// is `common` has a main checkout (`common` is its `.git` folder) and that link runs. The caller
/// resolves `common` within the project's checkout boundary.
fn main_link(common: &Path) -> Option<PathBuf> {
    let main = common
        .parent()
        .filter(|_| common.file_name().is_some_and(|name| name == ".git"))?;
    let link = main.join(crate::builds::LINKS).join(TASKS);
    crate::builds::runnable(&link).then_some(link)
}

/// Hands the event to the binary's `beat` and waits for it, until its [`Limit`] has passed; whatever
/// happens, nothing is printed.
pub fn run(raw: &[u8], env: &HashMap<String, String>) {
    if !wanted(raw, env) {
        return;
    }
    let Some(binary) = binary(env, raw) else {
        return;
    };
    let limit = Limit::of(env);
    let Ok(interrupts) = symphony_board::sys::Interrupts::listen() else {
        return;
    };
    beat_within(&binary, raw, &mut |ran| {
        interrupts.received() || limit.passed(ran)
    });
}

/// The longest a beat may hold up the tool call.
const LIMIT: Duration = Duration::from_secs(10);

/// When a beat has run too long.
enum Limit<'a> {
    /// Once it has run for [`LIMIT`] by this machine's clock.
    Machine,
    /// Once the file `SYMPHONY_HOOK_BEAT_CLOCK` names is there: a test's clock, which moves only
    /// when the test writes that file, so a test's beat ends when its binary does, or on the test's
    /// word, never on how fast a loaded machine starts the binary.
    Marked(&'a Path),
}

impl Limit<'_> {
    fn of(env: &HashMap<String, String>) -> Limit<'_> {
        set(env, symphony_env::HOOK_BEAT_CLOCK.name)
            .map_or(Limit::Machine, |mark| Limit::Marked(Path::new(mark)))
    }

    /// Whether a beat that has run for `ran` by this machine's clock is over the limit.
    fn passed(&self, ran: Duration) -> bool {
        match self {
            Limit::Machine => ran >= LIMIT,
            Limit::Marked(mark) => mark.exists(),
        }
    }
}

/// `binary beat` with `raw` on its stdin, stopped at the first look at it at which `passed` (handed the
/// time it has run, by this machine's clock) says it is over its limit. Nonblocking input writes
/// keep the deadline effective even when the binary stops reading a pipe-sized event.
fn beat_within(binary: &Path, raw: &[u8], passed: &mut dyn FnMut(Duration) -> bool) {
    use std::io::{ErrorKind, Write};
    let Ok(mut child) = Command::new(binary)
        .arg("beat")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return;
    };
    let mut input = child.stdin.take();
    if input
        .as_ref()
        .is_some_and(|pipe| symphony_board::sys::set_nonblocking(pipe, true).is_err())
    {
        let _ = child.kill();
        let _ = child.wait();
        return;
    }
    let mut remaining = raw;
    let start = Instant::now();
    loop {
        if let Some(pipe) = input.as_mut() {
            match pipe.write(remaining) {
                Ok(wrote) => remaining = &remaining[wrote..],
                Err(error)
                    if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) => {}
                Err(_) => input = None,
            }
            if remaining.is_empty() {
                input = None;
            }
        }
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if !passed(start.elapsed()) => std::thread::sleep(Duration::from_millis(2)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_tree::Tree as Tmp;
    use std::ffi::CString;
    use std::fs::{File, OpenOptions};
    use std::io::Write;
    use std::os::unix::ffi::OsStrExt;

    /// A shell script `name` in `tmp` running `body`, and its hold: a fifo, `NAME.hold`, this test
    /// keeps open, so the script's `read -r line < "$0.hold"` waits until the test writes a line.
    fn held(tmp: &Tmp, name: &str, body: &str) -> (PathBuf, File) {
        let script = tmp.path(name);
        crate::fixture_script::write(&script, &format!("#!/bin/sh\n{body}"), false);
        let hold = fifo(&tmp.path(&format!("{name}.hold")));
        let hold = OpenOptions::new()
            .read(true)
            .write(true)
            .open(hold)
            .unwrap();
        (script, hold)
    }

    fn fifo(path: &Path) -> PathBuf {
        let name = CString::new(path.as_os_str().as_bytes()).unwrap();
        // SAFETY: name is a valid NUL-terminated path in this test's private directory.
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0, "{path:?}");
        path.to_path_buf()
    }

    #[test]
    fn a_binary_that_hangs_is_stopped_at_the_first_look_past_its_limit_whatever_the_event() {
        // It reads none of the event, so one bigger than a pipe holds fills it before the binary ends.
        for size in [16, 1 << 20] {
            let tmp = Tmp::new();
            let ready = fifo(&tmp.path("stuck.ready"));
            let (stuck, mut hold) = held(
                &tmp,
                "stuck",
                "echo $$ > \"$0.ready\"\nread -r line < \"$0.hold\"\n",
            );
            let (mut pid, mut late) = (None, 0);
            beat_within(&stuck, &vec![b' '; size], &mut |_| {
                if pid.is_none() {
                    // Opening the fifo waits for the binary to run: the limit passes once it has.
                    let said = std::fs::read_to_string(&ready).unwrap();
                    pid = Some(said.trim().parse::<libc::pid_t>().unwrap());
                } else {
                    // A beat still waiting past its limit: let the binary end, so the case says so.
                    late += 1;
                    writeln!(hold, "go").unwrap();
                }
                true
            });
            let pid = pid.expect("the beat never looked at the clock while the binary ran");
            assert_eq!(
                late, 0,
                "a {size}-byte event: the beat looked again past its limit"
            );
            // SAFETY: signal 0 only asks whether the process is there.
            let gone = unsafe { libc::kill(pid, 0) } == -1;
            assert!(gone, "a {size}-byte event: the binary was left running");
        }
    }

    #[test]
    fn a_binary_within_its_limit_is_waited_for_and_handed_the_event_whole() {
        for size in [16, 1 << 20] {
            let tmp = Tmp::new();
            let (recorder, mut hold) = held(
                &tmp,
                "recorder",
                "printf '%s\\n' \"$*\" > \"$0.argv\"\nread -r line < \"$0.hold\"\ncat > \"$0.stdin\"\n",
            );
            let event = format!(
                "{{\"hook_event_name\":\"PostToolUse\",\"agent_id\":\"agent-1\",\"padding\":\"{}\"}}",
                "x".repeat(size)
            );
            let event = event.as_bytes();
            let mut looks = 0;
            beat_within(&recorder, event, &mut |_| {
                looks += 1;
                if looks == 3 {
                    writeln!(hold, "go").unwrap(); // the binary ends of itself, two looks on
                }
                false
            });
            assert!(looks >= 3, "the beat stopped waiting after {looks} looks");
            let read = |log: &str| std::fs::read_to_string(tmp.path(log)).unwrap_or_default();
            assert_eq!(read("recorder.argv"), "beat\n");
            assert_eq!(read("recorder.stdin").as_bytes(), event);
        }
    }

    #[test]
    fn the_limit_passes_by_this_machines_clock_unless_a_test_names_its_own() {
        let tmp = Tmp::new();
        let mark = tmp.path("clock");
        let clock = symphony_env::HOOK_BEAT_CLOCK;
        let env = |pairs: &[(&str, &str)]| -> HashMap<String, String> {
            pairs
                .iter()
                .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
                .collect()
        };
        let mark = mark.to_str().unwrap();
        for machine in [
            env(&[]),
            env(&[(clock.name, "")]),
            env(&[("HOOK_BEAT_CLOCK", mark)]),
        ] {
            let limit = Limit::of(&machine);
            assert!(!limit.passed(LIMIT - Duration::from_millis(1)));
            assert!(limit.passed(LIMIT));
        }
        let _ = std::fs::remove_file(mark);
        let named = Limit::of(&env(&[(clock.name, mark)]));
        assert!(
            !named.passed(LIMIT * 100),
            "the machine's time moved a test's clock"
        );
        File::create(mark).unwrap();
        assert!(named.passed(Duration::ZERO));
    }
}
