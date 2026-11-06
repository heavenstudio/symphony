//! hc (herdr-core) and the Herdr app, over their command lines: the binaries `HC_BIN` (default `hc`)
//! and `HERDR_APP_BIN` (default `herdr-app`) name, split as a POSIX shell splits them, with the
//! caller's environment and working directory. Every call the launcher, the cleanup and the idle
//! sweep make goes through [`Hc`]; the tests point both at the crate's `fake-hc`.
//!
//! A terminal the board starts carries the labels `sf.board` ([`Hc::board`]: the project and the
//! board's database, so a scratch board's cleanup never reaches the live board's terminals) and
//! `sf.handle`, beside the Herdr app's `herdr.space` and `herdr.tab`, which place it in a tab.

use super::LaunchError;
use crate::cli::{Env, probes};
use crate::sys::Command;
use serde_json::{Map, Value};
use std::time::Duration;

/// A JSON object hc answered.
pub type Object = Map<String, Value>;

/// How long hc or the Herdr app may take to answer one call.
const PATIENCE: Duration = Duration::from_secs(120);

/// The label naming the board a terminal was started for.
pub const BOARD_LABEL: &str = "sf.board";
/// The label naming the board session a terminal was started for.
pub const HANDLE_LABEL: &str = "sf.handle";

/// hc's and the Herdr app's calls. [`Hc::run`], [`Hc::feed`] and [`Hc::app`] reach them; the rest
/// read what they answer.
pub trait Hc {
    /// What `hc ARGS` printed: `LaunchError` when it could not be run, or failed (an hc error's
    /// kind is in its message: [`is_not_found`]).
    fn run(&self, args: &[&str]) -> Result<String, LaunchError>;

    /// What `hc ARGS` printed given `input` on its stdin (`hc connect`'s JSON-RPC lines).
    fn feed(&self, args: &[&str], input: &str) -> Result<String, LaunchError>;

    /// What `herdr-app ARGS` printed: `LaunchError` when the window did not answer.
    fn app(&self, args: &[&str]) -> Result<String, LaunchError>;

    /// The terminal the caller runs in (`HC_TERMINAL_ID`), never closed by the board.
    fn caller(&self) -> Option<String>;

    /// The value of the [`BOARD_LABEL`] of this board's terminals: the project's folder name and
    /// the board's database.
    fn board(&self) -> String;

    /// The JSON `hc ARGS --json` printed. `--json` is hc's own: it goes before a `--`, past which
    /// the words are the program's.
    fn json(&self, args: &[&str]) -> Result<Value, LaunchError> {
        let mut all = args.to_vec();
        let at = args.iter().position(|a| *a == "--").unwrap_or(args.len());
        all.insert(at, "--json");
        let out = self.run(&all)?;
        serde_json::from_str(out.trim())
            .map_err(|_| LaunchError(format!("hc {} printed no JSON", words(args))))
    }

    /// The terminals `hc ls` lists with these labels (and finished ones too with `all`).
    fn listed(&self, labels: &[String], all: bool) -> Result<Vec<Object>, LaunchError> {
        let mut args = vec!["ls"];
        if all {
            args.push("--all");
        }
        for label in labels {
            args.extend(["--label", label]);
        }
        let answer = self.json(&args)?;
        match answer.get("terminals") {
            Some(Value::Array(found)) => found
                .iter()
                .map(|t| {
                    t.as_object().cloned().ok_or_else(|| {
                        LaunchError("hc ls listed a terminal that is no object".into())
                    })
                })
                .collect(),
            _ => Err(LaunchError("hc ls answered no list of terminals".into())),
        }
    }

    /// Every live terminal of the machine, whoever started it: what presence and the agents view
    /// read. Never closed on that account.
    fn live(&self) -> Result<Vec<Object>, LaunchError> {
        self.listed(&[], false)
    }

    /// This board's terminals, live and finished: the only ones the board ever closes.
    fn terminals(&self) -> Result<Vec<Object>, LaunchError> {
        self.listed(&[format!("{BOARD_LABEL}={}", self.board())], true)
    }

    /// How many live terminals the Herdr app's tab `tab` holds.
    fn in_tab(&self, tab: &str) -> Result<usize, LaunchError> {
        Ok(self.listed(&[format!("herdr.tab={tab}")], false)?.len())
    }

    /// `hc get T`: None when hc knows no such terminal.
    fn get(&self, terminal: &str) -> Result<Option<Object>, LaunchError> {
        match self.json(&["get", terminal]) {
            Ok(Value::Object(found)) => Ok(Some(found)),
            Ok(_) => Err(LaunchError(format!("hc get {terminal} answered no object"))),
            Err(e) if is_not_found(&e) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// What the terminal shows: its visible screen with `lines` 0, else that much of its history.
    fn read(&self, terminal: &str, lines: usize) -> Result<String, LaunchError> {
        self.run(&["read", terminal, "--lines", &lines.to_string()])
    }

    /// Press `keys` (`down`, `enter`, ...) in the terminal.
    fn keys(&self, terminal: &str, keys: &[&str]) -> Result<(), LaunchError> {
        let mut args = vec!["keys", terminal];
        args.extend(keys);
        self.json(&args).map(|_| ())
    }

    /// Send `text` to the agent in the terminal as its next message (hc refuses a blocked one):
    /// whether hc saw its turn start.
    fn send(&self, terminal: &str, text: &str) -> Result<bool, LaunchError> {
        let said = self.json(&["send", terminal, text])?;
        Ok(said.get("started") == Some(&Value::Bool(true)))
    }

    /// End the terminal and remove it: killed while live (or when hc could not say), then removed.
    /// One hc no longer knows is already gone.
    fn close(&self, terminal: &str) -> Result<(), LaunchError> {
        let live = match self.get(terminal) {
            Ok(None) => return Ok(()),
            Ok(Some(found)) => text(found.get("state")) == "live",
            Err(_) => true,
        };
        if live {
            match self.json(&["kill", terminal]) {
                Err(e) if is_not_found(&e) => return Ok(()),
                other => other.map(|_| ())?,
            }
        }
        match self.json(&["rm", terminal]) {
            Err(e) if is_not_found(&e) => Ok(()),
            other => other.map(|_| ()),
        }
    }

    /// The Herdr app's spaces, each with its `id`, `name` and `tabs` (`id`, `name`): what the
    /// window says (`herdr-app ls`), else the app's document the core keeps (`herdr-app`), which
    /// answers while the window does not.
    fn layout(&self) -> Result<Vec<Object>, LaunchError> {
        let window = self.app(&["ls"]).and_then(|out| {
            serde_json::from_str::<Value>(out.trim())
                .map_err(|_| LaunchError("herdr-app ls printed no JSON".into()))
        });
        let layout = match window {
            Ok(layout) if layout.get("spaces").is_some_and(Value::is_array) => layout,
            _ => self.document()?,
        };
        match layout.get("spaces") {
            Some(Value::Array(spaces)) => Ok(spaces
                .iter()
                .filter_map(|s| s.as_object().cloned())
                .collect()),
            _ => Err(LaunchError(
                "the Herdr app's layout holds no list of spaces".into(),
            )),
        }
    }

    /// The app's document, as `hc connect` gives it (`doc.get herdr-app`).
    fn document(&self) -> Result<Value, LaunchError> {
        let calls = [
            r#"{"jsonrpc":"2.0","id":1,"method":"hello","params":{}}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"doc.get","params":{"name":"herdr-app"}}"#,
        ];
        let out = self.feed(&["connect"], &format!("{}\n", calls.join("\n")))?;
        out.lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .find(|answer| answer.get("id") == Some(&Value::from(2)))
            .and_then(|answer| answer.get("result")?.get("value").cloned())
            .filter(Value::is_object)
            .ok_or_else(|| LaunchError("the core holds no Herdr app document".into()))
    }

    /// Name a space or a tab (`what`) of the Herdr app; the window must answer.
    fn rename(&self, what: &str, id: &str, name: &str) -> Result<(), LaunchError> {
        let command = format!("{what}.rename");
        let out = self.app(&["do", &command, "--id", id, "--name", name])?;
        match serde_json::from_str::<Value>(out.trim()) {
            Ok(Value::Object(said)) if said.get("error").is_some() => Err(LaunchError(format!(
                "herdr-app do {command}: {}",
                text(said.get("error"))
            ))),
            _ => Ok(()),
        }
    }
}

/// Whether hc answered that it knows no such terminal.
pub fn is_not_found(error: &LaunchError) -> bool {
    error.0.contains(": not_found: ")
}

/// What a call's errors name it by: its command, and what it acts on unless that is a flag.
fn words(args: &[&str]) -> String {
    match args {
        [command, target, ..] if !target.starts_with('-') => format!("{command} {target}"),
        [command, ..] => command.to_string(),
        [] => String::new(),
    }
}

/// The hc and Herdr app binaries `env` names, run with `env`'s variables in its working directory.
pub struct Binary {
    hc: Result<Vec<String>, String>,
    app: Result<Vec<String>, String>,
    env: Env,
}

impl Binary {
    pub fn of(env: &Env) -> Binary {
        Binary {
            hc: probes::split(env.var("HC_BIN").unwrap_or("hc")),
            app: probes::split(env.var("HERDR_APP_BIN").unwrap_or("herdr-app")),
            env: env.clone(),
        }
    }

    fn command(
        &self,
        named: &Result<Vec<String>, String>,
        var: &str,
        args: &[&str],
    ) -> Result<Command, LaunchError> {
        let named = named
            .as_ref()
            .map_err(|why| LaunchError(format!("{var}: {why}")))?;
        let (program, rest) = named
            .split_first()
            .ok_or_else(|| LaunchError(format!("{var} names no command")))?;
        let mut command = Command::new(program);
        command
            .args(rest)
            .args(args)
            .env_clear()
            .envs(&self.env.vars)
            .current_dir(&self.env.cwd);
        Ok(command)
    }
}

impl Hc for Binary {
    fn run(&self, args: &[&str]) -> Result<String, LaunchError> {
        let mut command = self.command(&self.hc, "HC_BIN", args)?;
        let done = probes::within(&mut command, PATIENCE)
            .map_err(|why| LaunchError(format!("hc {}: {why}", words(args))))?;
        answer("hc", args, done)
    }

    fn feed(&self, args: &[&str], input: &str) -> Result<String, LaunchError> {
        let mut command = self.command(&self.hc, "HC_BIN", args)?;
        let done = probes::within_fed(&mut command, input.as_bytes(), PATIENCE)
            .map_err(|why| LaunchError(format!("hc {}: {why}", words(args))))?;
        answer("hc", args, done)
    }

    fn app(&self, args: &[&str]) -> Result<String, LaunchError> {
        let mut command = self.command(&self.app, "HERDR_APP_BIN", args)?;
        let done = probes::within(&mut command, PATIENCE)
            .map_err(|why| LaunchError(format!("herdr-app {}: {why}", words(args))))?;
        answer("herdr-app", args, done)
    }

    fn caller(&self) -> Option<String> {
        self.env
            .var("HC_TERMINAL_ID")
            .filter(|t| !t.is_empty())
            .map(str::to_string)
    }

    fn board(&self) -> String {
        board_of(&self.env)
    }
}

/// The [`BOARD_LABEL`] of the board `env` reaches: the main checkout's folder name, then the
/// board's database (`TASKS_DATABASE`, else the live board's).
pub fn board_of(env: &Env) -> String {
    let root = probes::repo_root(&env.cwd).unwrap_or_else(|| env.cwd.clone());
    let project = root
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let database = env.var("TASKS_DATABASE").unwrap_or(crate::DATABASE);
    format!("{project}/{database}")
}

/// What a call printed, or its failure: hc's error (`{"error": {kind, message}}` on stderr) as
/// `hc WORDS: KIND: MESSAGE`, the window's (`{"error": "..."}` on stdout) as its text.
fn answer(tool: &str, args: &[&str], done: std::process::Output) -> Result<String, LaunchError> {
    if done.status.success() {
        return Ok(String::from_utf8_lossy(&done.stdout).into_owned());
    }
    let said = |bytes: &[u8]| String::from_utf8_lossy(bytes).trim().to_string();
    let (stderr, stdout) = (said(&done.stderr), said(&done.stdout));
    let why = [&stderr, &stdout]
        .into_iter()
        .find_map(|said| {
            let error = serde_json::from_str::<Value>(said)
                .ok()?
                .get("error")?
                .clone();
            Some(match error {
                Value::Object(error) => format!(
                    "{}: {}",
                    text_of(&error, "kind"),
                    text_of(&error, "message")
                ),
                other => text(Some(&other)),
            })
        })
        .unwrap_or_else(|| {
            if stderr.is_empty() {
                stdout.clone()
            } else {
                stderr.clone()
            }
        });
    Err(LaunchError(format!("{tool} {}: {why}", words(args))))
}

/// `value` as text: a JSON string as it is, a number as written, nothing for null or missing.
pub fn text(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
    }
}

/// `object[key]` as [`text`].
pub fn text_of(object: &Object, key: &str) -> String {
    text(object.get(key))
}

/// The status word of a terminal, whether hc gives it bare (`ls`) or as `{app, state}` (`get`);
/// empty when it gives none, or something that is no word.
pub fn status(terminal: &Object) -> String {
    let word = match terminal.get("status") {
        Some(Value::Object(status)) => status.get("state"),
        other => other,
    };
    word.and_then(Value::as_str).unwrap_or_default().to_string()
}

/// `claude` or `codex` for an agent's terminal, from what hc calls it or what runs in it; None for
/// a shell or anything else.
pub fn agent(terminal: &Object) -> Option<&'static str> {
    let app = match terminal.get("status") {
        Some(Value::Object(status)) => text_of(status, "app"),
        _ => String::new(),
    };
    let foreground = terminal
        .get("foreground")
        .and_then(|f| f.get("name"))
        .map(|n| text(Some(n)))
        .unwrap_or_default();
    [text_of(terminal, "agent"), app, foreground]
        .iter()
        .find_map(|name| match name.as_str() {
            "claude" | "claude-code" => Some("claude"),
            "codex" => Some("codex"),
            _ => None,
        })
}

/// The argv of the terminal's foreground program (`hc get`), else of the program it started.
pub fn argv(terminal: &Object) -> Vec<String> {
    let listed = |value: Option<&Value>| -> Vec<String> {
        value
            .and_then(Value::as_array)
            .map(|all| all.iter().map(|a| text(Some(a))).collect())
            .unwrap_or_default()
    };
    let foreground = listed(terminal.get("foreground").and_then(|f| f.get("argv")));
    if foreground.is_empty() {
        listed(terminal.get("argv"))
    } else {
        foreground
    }
}

/// The pid of the terminal's foreground program, else of the program it started.
pub fn pid(terminal: &Object) -> Option<i64> {
    let foreground = terminal.get("foreground").and_then(|f| f.get("pid"));
    [foreground, terminal.get("pid")]
        .into_iter()
        .flatten()
        .find_map(Value::as_i64)
        .filter(|pid| *pid > 0)
}

/// A label of the terminal (`hc get`), or "".
pub fn label(terminal: &Object, key: &str) -> String {
    text(terminal.get("labels").and_then(|l| l.get(key)))
}

/// The conversation an argv resumes or starts: claude's `--session-id`/`--resume` id, codex's
/// `resume ID` as `codex:ID`.
pub fn conversation(argv: &[String]) -> Option<String> {
    let run = program(argv);
    let base = run.first().map(|p| p.rsplit('/').next().unwrap_or(p));
    let after = |flag: &str| {
        run.iter()
            .position(|a| a == flag)
            .and_then(|at| run.get(at + 1))
            .filter(|id| !id.is_empty() && !id.starts_with('-'))
    };
    match base? {
        "claude" => after("--session-id").or_else(|| after("--resume")).cloned(),
        "codex" => after("resume").map(|id| format!("codex:{id}")),
        _ => None,
    }
}

/// The program an argv runs, past `env` and its `KEY=VALUE` settings.
pub fn program(argv: &[String]) -> &[String] {
    let mut at = 0;
    if argv
        .first()
        .is_some_and(|a| a.rsplit('/').next() == Some("env"))
    {
        at = 1;
        while argv.get(at).is_some_and(|a| a.contains('=')) {
            at += 1;
        }
    }
    &argv[at.min(argv.len())..]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn object(value: Value) -> Object {
        value.as_object().cloned().unwrap()
    }

    #[test]
    fn status_and_agent_read_both_the_listed_and_the_detailed_shape() {
        let listed = object(json!({"agent": "claude-code", "status": "blocked"}));
        assert_eq!(status(&listed), "blocked");
        assert_eq!(agent(&listed), Some("claude"));
        let detailed = object(json!({
            "status": {"app": "codex", "state": "working"},
            "foreground": {"name": "node"},
        }));
        assert_eq!(status(&detailed), "working");
        assert_eq!(agent(&detailed), Some("codex"));
        let shell = object(json!({"agent": "zsh", "foreground": {"name": "zsh"}}));
        assert_eq!(status(&shell), "");
        assert_eq!(agent(&shell), None);
        assert_eq!(
            agent(&object(json!({"foreground": {"name": "claude"}}))),
            Some("claude")
        );
    }

    #[test]
    fn the_argv_and_pid_prefer_the_foreground_program() {
        let t = object(json!({
            "argv": ["env", "A=1", "claude"], "pid": 10,
            "foreground": {"argv": ["claude", "--resume", "abc"], "pid": 12},
        }));
        assert_eq!(argv(&t), ["claude", "--resume", "abc"]);
        assert_eq!(pid(&t), Some(12));
        let started = object(json!({"argv": ["zsh"], "pid": 10, "foreground": null}));
        assert_eq!(argv(&started), ["zsh"]);
        assert_eq!(pid(&started), Some(10));
        assert_eq!(pid(&object(json!({"pid": 0}))), None);
    }

    #[test]
    fn a_conversation_is_read_from_what_starts_or_resumes_it() {
        let words = |line: &str| line.split(' ').map(str::to_string).collect::<Vec<_>>();
        for (line, expected) in [
            (
                "env TASKS_SESSION=a claude --session-id s1 -n a",
                Some("s1"),
            ),
            ("/usr/bin/claude --resume s2 -n a", Some("s2")),
            (
                "env X=1 codex --no-daemon -s workspace-write resume th1 hi",
                Some("codex:th1"),
            ),
            ("codex --no-daemon -m m prompt", None),
            ("claude --resume --model x", None),
            ("zsh -l", None),
            ("env", None),
        ] {
            assert_eq!(conversation(&words(line)).as_deref(), expected, "{line}");
        }
    }

    #[test]
    fn a_failure_names_the_call_and_hcs_error_kind() {
        use std::os::unix::process::ExitStatusExt;
        let failed = |stdout: &str, stderr: &str| std::process::Output {
            status: std::process::ExitStatus::from_raw(1 << 8),
            stdout: stdout.as_bytes().to_vec(),
            stderr: stderr.as_bytes().to_vec(),
        };
        let e = answer(
            "hc",
            &["get", "t9", "--json"],
            failed(
                "",
                r#"{"error":{"kind":"not_found","message":"no terminal","hint":""}}"#,
            ),
        )
        .unwrap_err();
        assert_eq!(e.0, "hc get t9: not_found: no terminal");
        assert!(is_not_found(&e));
        let window = answer(
            "herdr-app",
            &["ls"],
            failed(r#"{"error": "the window did not answer"}"#, ""),
        )
        .unwrap_err();
        assert_eq!(window.0, "herdr-app ls: the window did not answer");
        assert!(!is_not_found(&window));
        let plain = answer("hc", &["ls"], failed("", "socket gone")).unwrap_err();
        assert_eq!(plain.0, "hc ls: socket gone");
    }
}
