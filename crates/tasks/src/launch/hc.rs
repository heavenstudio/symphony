//! hc (herdr-core) and the Herdr app, over their command lines: the binaries `HC_BIN` (default `hc`)
//! and `HERDR_APP_BIN` (default `herdr-app`) name, split as a POSIX shell splits them, with the
//! caller's environment but its session ([`Binary`]) and its working directory. Every call the launcher, the cleanup and the idle
//! sweep make goes through [`Hc`]; the tests point both at the crate's `fake-hc`.
//!
//! A terminal the board starts carries the labels `symphony.board` ([`Hc::board`]: the project and
//! the board's database, so a scratch board's cleanup never reaches the live board's terminals) and
//! `symphony.handle`, beside the Herdr app's `herdr.space` and `herdr.tab`, which place it in a
//! tab.

use super::LaunchError;
use crate::cli::{Env, probes};
use crate::sys::{self, Command};
use serde_json::{Map, Value};
use std::time::{Duration, Instant};

/// A JSON object hc answered.
pub type Object = Map<String, Value>;

/// How long hc or the Herdr app may take to answer one call.
const PATIENCE: Duration = Duration::from_secs(120);

/// How long a killed terminal is given to end before its remove is left to a later sweep
/// ([`Hc::end`]), and how often hc is asked whether it has in that time.
const ENDING: Duration = Duration::from_secs(10);
const ENDING_LOOK: Duration = Duration::from_millis(100);
/// What sets, in seconds, how long a [`Binary`] gives a killed terminal in place of [`ENDING`]: a
/// test's, whose stand-in hc takes a loaded machine's time to answer each look.
pub const ENDING_VAR: &str = "TASKS_HC_ENDING";

/// The label naming the board a terminal was started for.
pub const BOARD_LABEL: &str = "symphony.board";
/// The label naming the board session a terminal was started for.
pub const HANDLE_LABEL: &str = "symphony.handle";

/// What the board starts Codex through, its program and arguments after it, so the commands Codex
/// runs know the terminal they run in: hc gives a terminal its address only once it runs, so `sh`
/// hands the terminal's own `HC_TERMINAL_ID` to Codex's shell
/// (`shell_environment_policy.set`, which Codex applies after its inherited baseline, a
/// `core` one included), then becomes Codex (`exec`), which keeps the pid and leaves Codex the
/// terminal's foreground. [`program`] reads past it.
pub const IN_ITS_TERMINAL: [&str; 3] = [
    "sh",
    "-c",
    r#"exec "$0" -c "shell_environment_policy.set.HC_TERMINAL_ID=\"$HC_TERMINAL_ID\"" "$@""#,
];

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

    /// Bring the Herdr app's window in front of the others on this machine: whether it did. Only a
    /// macOS app bundle can be brought forward ([`Binary`]); a stand-in has none, and nothing is.
    fn raise(&self) -> Result<bool, LaunchError> {
        Ok(false)
    }

    /// The terminal the caller runs in (`HC_TERMINAL_ID`), never closed by the board.
    fn caller(&self) -> Option<String>;

    /// The value of the [`BOARD_LABEL`] of this board's terminals: the project's folder name and
    /// the board's database.
    fn board(&self) -> String;

    /// The id of the project whose board this is, as the board itself names it
    /// ([`crate::project::find`]): its file's, else the registry's, else its main checkout's
    /// folder name, the same from the main checkout and from a linked worktree. What the board's
    /// local attempts are scoped by ([`crate::execution::board_scope`]). An hc that reads neither
    /// the project's file nor the registry names the folder [`Hc::board`] does.
    fn project(&self) -> String {
        labelled(&self.board())
    }

    /// Queue a resumed Codex conversation's next message through its native channel. Acceptance
    /// does not say the resumed turn ended or that a turn on this text has started.
    fn queue_codex(&self, _thread: &str, _text: &str) -> Result<(), LaunchError> {
        Err(LaunchError("native Codex queue unavailable".into()))
    }

    /// The JSON `hc ARGS --json` printed. `--json` is hc's own: it goes before a `--`, past which
    /// the words are the program's.
    fn json(&self, args: &[&str]) -> Result<Value, LaunchError> {
        let mut all = args.to_vec();
        let at = args.iter().position(|a| *a == "--").unwrap_or(args.len());
        all.insert(at, "--json");
        let out = self.run(&all)?;
        serde_json::from_str(out.trim())
            .map_err(|why| LaunchError(format!("hc {} printed no JSON: {why}", words(args))))
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
    /// One hc no longer knows is already gone. What the board started and could not use; a terminal
    /// the owner may be looking at closes only through [`super::screen::close_unseen`].
    fn close(&self, terminal: &str) -> Result<(), LaunchError> {
        let live = match self.get(terminal) {
            Ok(None) => return Ok(()),
            Ok(Some(found)) => text(found.get("state")) == "live",
            Err(_) => true,
        };
        self.end(terminal, live)
    }

    /// Kill the terminal when it is `live`, then remove it, with no look first: one hc no longer
    /// knows is already gone. hc removes no terminal still running, and a killed one may take a
    /// while to end: unless the kill's answer finds it ended, the remove waits until a look finds it
    /// so ([`Hc::ended`]), and is never tried while the last look found it live. One still live
    /// when the wait is over is killed and left: the next sweep finds its program ended and
    /// removes it.
    fn end(&self, terminal: &str, live: bool) -> Result<(), LaunchError> {
        if live {
            let killed = match self.json(&["kill", terminal]) {
                Err(e) if is_not_found(&e) => return Ok(()),
                other => other?,
            };
            let state = killed.get("state").and_then(Value::as_str);
            if !state.is_some_and(|state| state != "live") && !self.ended(terminal) {
                return Ok(());
            }
        }
        match self.json(&["rm", terminal]) {
            Err(e) if is_not_found(&e) => Ok(()),
            other => other.map(|_| ()),
        }
    }

    /// Wait for a killed terminal to end, by hc's own look (`hc get`) every [`ENDING_LOOK`], for
    /// at most [`Hc::ending`]: true once a look finds it ended, the one answer the remove follows.
    /// False when hc knows it no more (there is nothing left to remove), when a look fails, and
    /// when it is still live once the wait is over: the last look then said live, or nothing.
    fn ended(&self, terminal: &str) -> bool {
        let until = Instant::now() + self.ending();
        loop {
            match self.get(terminal) {
                Ok(Some(found)) if text(found.get("state")) != "live" => return true,
                #[expect(
                    clippy::disallowed_methods,
                    reason = "no-timed-waits baseline: looks again for a terminal's state, which its app announces to no one"
                )]
                Ok(Some(_)) if Instant::now() < until => std::thread::sleep(ENDING_LOOK),
                Ok(_) | Err(_) => return false,
            }
        }
    }

    /// How long [`Hc::ended`] waits for a killed terminal to end: [`ENDING`]. A test's hc waits
    /// less, to show a wait that runs out.
    fn ending(&self) -> Duration {
        ENDING
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

/// The hc and Herdr app binaries `env` names, run with `env`'s variables in its working directory,
/// but its session ([`sys::SESSION_VARS`]): no call is a child of the caller's terminal, nor scoped
/// to the caller's core. Every call names the terminal it acts on; the caller's own terminal is
/// read from `env` ([`Hc::caller`]), never handed to hc. A launched agent's `TASKS_SESSION` is a
/// word of the command line `hc run` starts (`env TASKS_SESSION=HANDLE ...`), not hc's.
pub struct Binary {
    hc: Result<Vec<String>, String>,
    app: Result<Vec<String>, String>,
    env: Env,
    deadline: Option<Deadline>,
}

/// When every call of a [`Binary`] must have answered by, all of them together, and the time it
/// was given for them: a page's call, which gives up long before [`PATIENCE`].
#[derive(Clone, Copy)]
struct Deadline {
    at: Instant,
    given: Duration,
}

impl Binary {
    pub fn of(env: &Env) -> Binary {
        Binary {
            hc: probes::split(env.var("HC_BIN").unwrap_or("hc")),
            app: probes::split(env.var("HERDR_APP_BIN").unwrap_or("herdr-app")),
            env: env.clone(),
            deadline: None,
        }
    }

    /// This hc with every call answered within `given` from now, all its calls together: each is
    /// given what is left of it (never more than [`PATIENCE`]) and killed when that runs out, and
    /// one started once it is spent is refused without being run.
    pub fn within(mut self, given: Duration) -> Binary {
        self.deadline = Some(Deadline {
            at: Instant::now() + given,
            given,
        });
        self
    }

    /// What `command` printed, fed `input`, within [`PATIENCE`] or what is left of the deadline:
    /// a call that does not end in that time is killed, and its error, as every error here, names
    /// it (`named`), and the limit that ran out: the deadline's time given only when what was left
    /// of it was the shorter.
    fn answered(
        &self,
        command: &mut Command,
        input: &[u8],
        named: impl FnOnce() -> String,
    ) -> Result<std::process::Output, LaunchError> {
        let (limit, by_deadline) = match self.deadline {
            None => (PATIENCE, None),
            Some(deadline) => {
                let left = deadline.at.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    return Err(LaunchError(format!(
                        "{}: not asked: the {} s it is given are spent",
                        named(),
                        deadline.given.as_secs()
                    )));
                }
                let limit = left.min(PATIENCE);
                (limit, (limit == left).then_some(deadline))
            }
        };
        probes::within_fed(command, input, limit).map_err(|why| {
            let why = match by_deadline {
                Some(deadline) if why.starts_with("no answer in ") => {
                    format!(
                        "no answer within the {} s it is given",
                        deadline.given.as_secs()
                    )
                }
                _ => why,
            };
            LaunchError(format!("{}: {why}", named()))
        })
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
        let mut command = sys::command_with(program, &self.env.vars);
        command.args(rest).args(args).current_dir(&self.env.cwd);
        Ok(command)
    }
}

impl Hc for Binary {
    fn queue_codex(&self, thread: &str, text: &str) -> Result<(), LaunchError> {
        let mut command = sys::command_with("codex", &self.env.vars);
        command.current_dir(&self.env.cwd);
        let patience = crate::codex_queue::patience(&self.env);
        crate::codex_queue::queue(command, thread, text, patience).map_err(LaunchError)
    }

    fn run(&self, args: &[&str]) -> Result<String, LaunchError> {
        let mut command = self.command(&self.hc, "HC_BIN", args)?;
        let done = self.answered(&mut command, &[], || format!("hc {}", words(args)))?;
        answer("hc", args, done)
    }

    fn feed(&self, args: &[&str], input: &str) -> Result<String, LaunchError> {
        let mut command = self.command(&self.hc, "HC_BIN", args)?;
        let done = self.answered(&mut command, input.as_bytes(), || {
            format!("hc {}", words(args))
        })?;
        answer("hc", args, done)
    }

    fn app(&self, args: &[&str]) -> Result<String, LaunchError> {
        let mut command = self.command(&self.app, "HERDR_APP_BIN", args)?;
        let done = self.answered(&mut command, &[], || format!("herdr-app {}", words(args)))?;
        answer("herdr-app", args, done)
    }

    /// [`ENDING`], unless [`ENDING_VAR`] says.
    fn ending(&self) -> Duration {
        self.env.seconds(ENDING_VAR, ENDING)
    }

    /// `open -a BUNDLE`, which brings a running app forward without starting another: the macOS
    /// app bundle `HERDR_APP_BIN`'s program lies in, its links followed. A program in no bundle (a
    /// stand-in, another system's build) has no window macOS can bring forward: nothing is run.
    fn raise(&self) -> Result<bool, LaunchError> {
        if !cfg!(target_os = "macos") {
            return Ok(false);
        }
        let Some(program) = self.app.as_ref().ok().and_then(|words| words.first()) else {
            return Ok(false);
        };
        let Some(bundle) = located(program, &self.env)
            .and_then(|found| std::fs::canonicalize(found).ok())
            .and_then(|found| bundle_of(&found))
        else {
            return Ok(false);
        };
        let mut command = sys::command_with("/usr/bin/open", &self.env.vars);
        command.arg("-a").arg(&bundle).current_dir(&self.env.cwd);
        let done = self.answered(&mut command, &[], || {
            format!("open -a {}", bundle.display())
        })?;
        let named = bundle.to_string_lossy();
        answer("open", &["-a", &named], done).map(|_| true)
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

    fn project(&self) -> String {
        let machine = crate::cli::machine(&self.env);
        crate::project::find(&self.env.cwd, machine.as_deref())
            .map_or_else(|_| labelled(&self.board()), |project| project.id)
    }
}

/// The project folder's name the [`BOARD_LABEL`] `board` (`PROJECT/DATABASE`) begins with.
fn labelled(board: &str) -> String {
    let (project, _) = board.rsplit_once('/').unwrap_or((board, ""));
    project.to_string()
}

/// Where `program` is: a path (one with a `/`) from `env`'s working directory, else the first
/// file of that name in `env`'s `PATH`.
fn located(program: &str, env: &Env) -> Option<std::path::PathBuf> {
    if program.contains('/') {
        return Some(env.cwd.join(program));
    }
    std::env::split_paths(env.var("PATH")?)
        .map(|dir| dir.join(program))
        .find(|candidate| candidate.is_file())
}

/// The macOS app bundle (a folder named `NAME.app`) the program at `path` lies in, if any.
fn bundle_of(path: &std::path::Path) -> Option<std::path::PathBuf> {
    path.ancestors()
        .skip(1)
        .find(|dir| dir.extension().is_some_and(|ext| ext == "app"))
        .map(std::path::Path::to_path_buf)
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

/// The pid of the program the terminal started (for an agent, the agent, which `env` became), not
/// of its foreground's; None when hc knows none.
pub fn top_pid(terminal: &Object) -> Option<i64> {
    terminal
        .get("pid")
        .and_then(Value::as_i64)
        .filter(|p| *p > 0)
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
        "codex" => after("resume").map(|id| format!("{}{id}", crate::launch::CODEX)),
        _ => None,
    }
}

/// The program an argv runs, past `env` and its `KEY=VALUE` settings, and past the board's
/// [`IN_ITS_TERMINAL`].
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
    let rest = &argv[at.min(argv.len())..];
    match rest.split_at_checked(IN_ITS_TERMINAL.len()) {
        Some((wrapper, run)) if wrapper == IN_ITS_TERMINAL => run,
        _ => rest,
    }
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
        // The board starts Codex through its wrapper: a finished terminal lists that argv.
        let wrapped = |run: &str| {
            let mut argv = words("env TASKS_SESSION=a");
            argv.extend(IN_ITS_TERMINAL.map(str::to_string));
            argv.extend(words(run));
            argv
        };
        let resumed = wrapped("codex --no-daemon -c x=1 resume th2");
        assert_eq!(program(&resumed)[0], "codex");
        assert_eq!(conversation(&resumed).as_deref(), Some("codex:th2"));
        assert_eq!(conversation(&wrapped("codex --no-daemon")), None);
        // Another shell's script is no wrapper of the board's: it runs `sh`.
        let other = words("env A=1 sh -c true codex resume th3");
        assert_eq!(program(&other)[0], "sh");
        assert_eq!(conversation(&other), None);
        assert_eq!(program(&words("env A=1 sh -c")), ["sh", "-c"]);
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

    #[test]
    fn the_bundle_a_program_lies_in_is_the_nearest_folder_named_app() {
        use std::path::{Path, PathBuf};
        let bundle = |path: &str| bundle_of(Path::new(path));
        assert_eq!(
            bundle("/Applications/Herdr.app/Contents/MacOS/Herdr"),
            Some(PathBuf::from("/Applications/Herdr.app"))
        );
        assert_eq!(
            bundle("/Users/x/Apps/Outer.app/Contents/Helpers/Inner.app/Contents/MacOS/inner"),
            Some(PathBuf::from(
                "/Users/x/Apps/Outer.app/Contents/Helpers/Inner.app"
            ))
        );
        // The program itself is never its own bundle, and a build folder is none.
        assert_eq!(bundle("/tmp/Odd.app"), None);
        assert_eq!(bundle("/repo/.target/debug/fake-hc"), None);
        assert_eq!(bundle("herdr-app"), None);
    }

    #[test]
    fn a_program_is_located_by_its_path_or_on_path_and_a_stand_in_is_never_raised() {
        let dir = std::env::temp_dir().join(format!("hc-located-{}", std::process::id()));
        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("herdr-app"), "").unwrap();
        let env = |vars: &[(&str, String)]| Env {
            vars: vars
                .iter()
                .map(|(k, v)| ((*k).to_string(), v.clone()))
                .collect(),
            cwd: dir.clone(),
        };
        let path = format!("/nowhere:{}", bin.display());
        assert_eq!(
            located("herdr-app", &env(&[("PATH", path.clone())])),
            Some(bin.join("herdr-app"))
        );
        assert_eq!(located("hc", &env(&[("PATH", path)])), None);
        assert_eq!(located("herdr-app", &env(&[])), None);
        assert_eq!(
            located("bin/herdr-app", &env(&[])),
            Some(dir.join("bin/herdr-app"))
        );
        // A program in no bundle has no window to bring forward: nothing runs.
        let stand_in = Binary::of(&env(&[(
            "HERDR_APP_BIN",
            format!("{} app", bin.join("herdr-app").display()),
        )]));
        assert_eq!(stand_in.raise(), Ok(false));
        let unnamed = Binary::of(&env(&[("HERDR_APP_BIN", String::new())]));
        assert_eq!(unnamed.raise(), Ok(false));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
