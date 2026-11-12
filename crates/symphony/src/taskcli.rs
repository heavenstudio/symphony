//! The task board, reached through the project's build of the `tasks` binary, found as the heartbeat
//! finds it: the checkout's `.target/bin/tasks` link (else, from a linked worktree, the main
//! checkout's), else `tasks` on `PATH` ([`crate::builds`]). Its API (`tasks api`, crates/tasks/src/cli/api.rs) is for reading, its
//! commands for writing. The board is the one the environment reaches: `TASKS_SERVER` and
//! `TASKS_DATABASE` name a test's, else the server in the checkout's git common dir; `TASKS_NOW` pins
//! its clock. `SYMPHONY_TASKS_CLI` names another command to run in the binary's place (split as a
//! shell splits it), as the tests of the board's own tools name theirs.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Output;

use serde_json::{Value, json};

use crate::children::command;
use crate::verdicts::{Board, BoardTask, VerdictError};

pub struct CliBoard {
    cli: Vec<String>,
    env: Vec<(String, String)>,
    cwd: PathBuf,
    /// What a run that cannot start adds: how to give the project a build to run, when none was found.
    missing: Option<String>,
}

/// The board's binary.
const TASKS: &str = "tasks";

/// `command` split as a shell splits it: on blanks, with single and double quotes and backslashes keeping
/// a word whole, as Python's `shlex.split` does.
pub fn shell_words(command: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut quote: Option<char> = None;
    let mut chars = command.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('"'), '\\') => match chars.next() {
                Some(next) if next == '"' || next == '\\' => word.push(next),
                Some(next) => {
                    word.push('\\');
                    word.push(next);
                }
                None => word.push('\\'),
            },
            (Some(_), c) => word.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                started = true;
            }
            (None, '\\') => {
                started = true;
                word.extend(chars.next());
            }
            (None, c) if c.is_whitespace() => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            (None, c) => {
                started = true;
                word.push(c);
            }
        }
    }
    if started {
        words.push(word);
    }
    words
}

fn words(env: symphony_env::Var, default: Vec<String>) -> Vec<String> {
    match env.text() {
        Some(command) if !command.trim().is_empty() => shell_words(&command),
        _ => default,
    }
}

/// The project's build of the board's binary from the checkout at `root`: its link (the main
/// checkout's from a linked worktree), as the heartbeat runs it. None leaves `tasks` to the search
/// path.
pub fn tasks_binary(root: &Path) -> Option<PathBuf> {
    crate::builds::linked(root, TASKS)
}

impl CliBoard {
    /// The board the environment reaches from the checkout at `root`, through the project's build
    /// of the board's binary ([`tasks_binary`]), else `tasks` on the search path.
    pub fn new(root: &Path) -> CliBoard {
        CliBoard::named_else_found(words(symphony_env::TASKS_CLI, Vec::new()), root)
    }

    /// The board through `named` (`SYMPHONY_TASKS_CLI`'s words) when it names a command, else
    /// through the project's build of the board's binary from `root`.
    fn named_else_found(named: Vec<String>, root: &Path) -> CliBoard {
        if !named.is_empty() {
            return CliBoard::with_command(named, &[], root);
        }
        if let Some(found) = tasks_binary(root) {
            return CliBoard::with_command(vec![found.to_string_lossy().into_owned()], &[], root);
        }
        let mut board = CliBoard::with_command(vec![TASKS.to_string()], &[], root);
        board.missing = Some(format!(
            "link the project's build of {TASKS} at {}, or put {TASKS} on PATH",
            root.join(crate::builds::LINKS).join(TASKS).display()
        ));
        board
    }

    /// The board reached through `cli` (the `tasks` binary), run in `cwd` with `env` on top of this
    /// process's own environment.
    pub fn with_command(cli: Vec<String>, env: &[(String, String)], cwd: &Path) -> CliBoard {
        CliBoard {
            cli,
            env: env.to_vec(),
            cwd: cwd.to_path_buf(),
            missing: None,
        }
    }

    /// `key` as the binary will see it: this board's own, else this process's.
    fn var(&self, key: &str) -> Option<String> {
        self.env
            .iter()
            .rev()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
            .or_else(|| std::env::var(key).ok())
    }

    /// The binary with `extra`, `stdin` on its stdin (none if None), run to its end. Its input and
    /// output go through [`symphony_board::sys`]'s pipes, which no child another thread spawns holds open.
    fn output(&self, extra: &[&str], stdin: Option<&str>) -> Result<Output, VerdictError> {
        let mut command = command(&self.cli[0]);
        command
            .args(&self.cli[1..])
            .args(extra)
            .envs(self.env.iter().map(|(k, v)| (k, v)))
            .current_dir(&self.cwd);
        let mut child = if stdin.is_some() {
            symphony_board::sys::spawn_piped(&mut command)
        } else {
            symphony_board::sys::spawn_output(&mut command)
        }
        .map_err(|e| {
            let fix = self
                .missing
                .as_ref()
                .filter(|_| e.kind() == std::io::ErrorKind::NotFound)
                .map_or_else(String::new, |fix| format!("; {fix}"));
            VerdictError(format!("cannot run {}: {e}{fix}", self.cli.join(" ")))
        })?;
        if let Some(text) = stdin {
            let mut pipe = child.stdin.take().expect("a piped stdin");
            let text = text.to_string();
            std::thread::spawn(move || {
                let _ = pipe.write_all(text.as_bytes());
            });
        }
        symphony_board::sys::wait_output(child).map_err(|e| VerdictError(e.to_string()))
    }

    /// One of the board's commands: what it printed, or why it refused.
    fn run(&self, extra: &[&str], stdin: Option<&str>) -> Result<String, VerdictError> {
        let done = self.output(extra, stdin)?;
        let out = String::from_utf8_lossy(&done.stdout).trim().to_string();
        if done.status.success() {
            return Ok(out);
        }
        let err = String::from_utf8_lossy(&done.stderr).trim().to_string();
        Err(VerdictError(if err.is_empty() { out } else { err }))
    }

    /// One call of the board's API: its result. A refusal is the board's own words; a board the binary
    /// cannot reach is said so.
    pub(crate) fn call(&self, method: &str, args: &Value) -> Result<Value, VerdictError> {
        let request = json!({"method": method, "args": args}).to_string();
        let done = self.output(&["api"], Some(&request))?;
        let unreachable = VerdictError::unreachable;
        let mut answer: Value = serde_json::from_slice(&done.stdout).map_err(|_| {
            let err = String::from_utf8_lossy(&done.stderr);
            unreachable(
                err.trim()
                    .lines()
                    .last()
                    .unwrap_or("the binary gave no answer"),
            )
        })?;
        if let Some(result) = answer.get_mut("ok") {
            return Ok(result.take());
        }
        let said = |key: &str| answer.get(key).and_then(Value::as_str).map(str::to_string);
        if let Some(why) = said("refused") {
            return Err(VerdictError(why));
        }
        Err(unreachable(
            &said("error").unwrap_or_else(|| answer.to_string()),
        ))
    }
}

fn text<'a>(row: &'a Value, key: &str) -> &'a str {
    row[key].as_str().unwrap_or("")
}

fn rows(value: &Value) -> Vec<Value> {
    value.as_array().cloned().unwrap_or_default()
}

impl Board for CliBoard {
    /// The time the board writes: `TASKS_NOW` when the environment pins it, else this machine's.
    fn now(&self) -> f64 {
        if let Some(pinned) = self
            .var("TASKS_NOW")
            .and_then(|t| t.trim().parse::<f64>().ok())
        {
            return pinned;
        }
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs_f64())
    }

    fn tasks(&self) -> Result<Vec<BoardTask>, VerdictError> {
        let tasks = rows(&self.call("tasks", &json!({}))?);
        let slugs: Vec<&str> = tasks.iter().map(|t| text(t, "slug")).collect();
        let threads = rows(&self.call("threads", &json!({ "slugs": slugs }))?);
        let now = self.now();
        Ok(tasks
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let roles = rows(&t["roles"]);
                let held = |role: &str| {
                    roles
                        .iter()
                        .filter(|r| text(r, "role") == role)
                        .filter(|r| {
                            role != "reserved" || r["expires"].as_f64().is_none_or(|e| e >= now)
                        })
                        .count()
                };
                BoardTask {
                    slug: text(t, "slug").to_string(),
                    title: text(t, "title").to_string(),
                    body: text(t, "body").to_string(),
                    status: text(t, "status").to_string(),
                    created: t["created"].as_f64().unwrap_or(0.0),
                    executors: held("executor"),
                    reserved: held("reserved"),
                    thread: threads
                        .get(i)
                        .map(rows)
                        .unwrap_or_default()
                        .iter()
                        .map(|m| (m["ts"].as_f64().unwrap_or(0.0), text(m, "text").to_string()))
                        .collect(),
                    links: rows(&t["links"])
                        .iter()
                        .filter_map(|link| link.as_str().map(str::to_string))
                        .collect(),
                }
            })
            .collect())
    }

    fn app_session(&self) -> Result<Option<String>, VerdictError> {
        let sessions = rows(&self.call("sessions", &json!({}))?);
        Ok(sessions
            .iter()
            .find(|s| text(s, "kind") == "app" && s["left_at"].is_null())
            .map(|s| text(s, "handle").to_string()))
    }

    fn register_app(&self) -> Result<String, VerdictError> {
        self.run(
            &["register", "--kind", "app", "--model", "review-app"],
            None,
        )
    }

    fn say(&self, slug: &str, by: &str, message: &str) -> Result<(), VerdictError> {
        self.run(&["--as", by, "say", slug, message], None)
            .map(|_| ())
    }

    fn create(
        &self,
        slug: &str,
        title: &str,
        by: &str,
        body: &str,
        links: &[String],
    ) -> Result<(), VerdictError> {
        let mut args = vec!["--as", by, "create", slug, title, "--body-file", "-"];
        for link in links {
            args.extend(["--link", link]);
        }
        self.run(&args, Some(body)).map(|_| ())
    }

    fn amend(
        &self,
        slug: &str,
        title: &str,
        links: &[String],
        by: &str,
    ) -> Result<(), VerdictError> {
        self.call(
            "amend",
            &json!({"slug": slug, "title": title, "links": links, "by": by}),
        )
        .map(|_| ())
    }
}

#[cfg(test)]
#[path = "taskcli_tests.rs"]
mod tests;
