//! The board as the exchange reaches it: a [`Board`], five things it reads or writes, and
//! [`Api`], which does each through the board's JSON calls (`tasks api`, `docs/BOARD_API.md`) and
//! nothing else. It reads no database and links no board library; another board answers the same
//! trait.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// A task as the exchange reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Task {
    pub slug: String,
    /// Its number, which names it only with the generation of its token.
    pub id: i64,
    pub title: String,
    pub body: String,
}

/// The board's token for one task at one version, kept as given and handed back to write by.
/// The exchange reads two of its fields, the board's recovery generation and the task's number,
/// to know a mapping kept under another generation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Token(pub Value);

impl Token {
    #[must_use]
    pub fn generation(&self) -> &str {
        self.0["generation"].as_str().unwrap_or_default()
    }

    #[must_use]
    pub fn id(&self) -> i64 {
        self.0["id"].as_i64().unwrap_or_default()
    }
}

/// A task's title and brief with the token to write them by.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bound {
    pub token: Token,
    pub title: String,
    pub body: String,
}

/// Why the board did not do what was asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BoardError {
    /// The board's own refusal, which wrote nothing.
    Refused(String),
    /// The call failed: whether it wrote is not known.
    Failed(String),
}

impl BoardError {
    #[must_use]
    pub fn said(&self) -> &str {
        match self {
            BoardError::Refused(why) | BoardError::Failed(why) => why,
        }
    }
}

/// What the exchange asks of a board.
pub trait Board {
    /// The folder the integration keeps its state in.
    ///
    /// # Errors
    ///
    /// As every call: [`BoardError`].
    fn state_folder(&self) -> Result<PathBuf, BoardError>;

    /// Every task.
    ///
    /// # Errors
    ///
    /// As every call.
    fn tasks(&self) -> Result<Vec<Task>, BoardError>;

    /// A new proposal in the backlog, filed by `by`.
    ///
    /// # Errors
    ///
    /// Refused when the slug is taken, among the board's other refusals.
    fn propose(&self, slug: &str, title: &str, body: &str, by: &str) -> Result<Task, BoardError>;

    /// The task's title and brief with its token.
    ///
    /// # Errors
    ///
    /// Refused where the board has no recovery binding.
    fn bound(&self, slug: &str) -> Result<Bound, BoardError>;

    /// The task's title and brief rewritten, only if it is still the task and version `token`
    /// names under the generation it names.
    ///
    /// # Errors
    ///
    /// Refused when it is not, having written nothing.
    fn edit(
        &self,
        slug: &str,
        token: &Token,
        title: &str,
        body: &str,
        by: &str,
    ) -> Result<Bound, BoardError>;

    /// A note on the task's thread, written once for `key` however often it is asked.
    ///
    /// # Errors
    ///
    /// As every call.
    fn note(
        &self,
        slug: &str,
        text: &str,
        to: Option<&str>,
        key: &str,
        by: &str,
    ) -> Result<(), BoardError>;
}

/// What carries one JSON call to the board and brings its answer back.
pub trait Caller {
    /// The answer to `{"method": method, "args": args}`: an object holding `ok`, `refused` or
    /// `error`.
    ///
    /// # Errors
    ///
    /// The call was not carried.
    fn call(&self, method: &str, args: Value) -> Result<Value, String>;
}

/// A board reached through its JSON calls.
pub struct Api<C: Caller>(pub C);

impl<C: Caller> Api<C> {
    fn ok(&self, method: &str, args: Value) -> Result<Value, BoardError> {
        let mut answer = self
            .0
            .call(method, args)
            .map_err(|why| BoardError::Failed(format!("tasks api {method}: {why}")))?;
        if let Some(why) = answer["refused"].as_str() {
            return Err(BoardError::Refused(why.into()));
        }
        if let Some(why) = answer.get("error") {
            let why = why.as_str().map_or_else(|| why.to_string(), str::to_string);
            return Err(BoardError::Failed(format!("tasks api {method}: {why}")));
        }
        match answer.get_mut("ok") {
            Some(ok) => Ok(ok.take()),
            None => Err(BoardError::Failed(format!(
                "tasks api {method}: an answer with no result"
            ))),
        }
    }
}

fn task(value: &Value) -> Option<Task> {
    Some(Task {
        slug: value["slug"].as_str()?.into(),
        id: value["id"].as_i64()?,
        title: value["title"].as_str()?.into(),
        body: value["body"].as_str().unwrap_or_default().into(),
    })
}

fn bound(value: &Value) -> Result<Bound, BoardError> {
    let read = || {
        Some(Bound {
            token: Token(value.get("bound")?.clone()),
            title: value["title"].as_str()?.into(),
            body: value["body"].as_str()?.into(),
        })
    };
    read().ok_or_else(|| BoardError::Failed("tasks api: a bound answer of another shape".into()))
}

impl<C: Caller> Board for Api<C> {
    fn state_folder(&self) -> Result<PathBuf, BoardError> {
        let answer = self.ok("integration_state", json!({"name": crate::NAME}))?;
        answer["path"]
            .as_str()
            .map(PathBuf::from)
            .ok_or_else(|| BoardError::Failed("tasks api integration_state: no path".into()))
    }

    fn tasks(&self) -> Result<Vec<Task>, BoardError> {
        let answer = self.ok("tasks", json!({}))?;
        answer
            .as_array()
            .map(|tasks| tasks.iter().filter_map(task).collect())
            .ok_or_else(|| BoardError::Failed("tasks api tasks: not a list".into()))
    }

    fn propose(&self, slug: &str, title: &str, body: &str, by: &str) -> Result<Task, BoardError> {
        let answer = self.ok(
            "create",
            json!({"slug": slug, "title": title, "body": body, "by": by, "status": "backlog"}),
        )?;
        task(&answer)
            .ok_or_else(|| BoardError::Failed("tasks api create: a task of another shape".into()))
    }

    fn bound(&self, slug: &str) -> Result<Bound, BoardError> {
        bound(&self.ok("bound", json!({"slug": slug}))?)
    }

    fn edit(
        &self,
        slug: &str,
        token: &Token,
        title: &str,
        body: &str,
        by: &str,
    ) -> Result<Bound, BoardError> {
        bound(&self.ok(
            "edit_bound",
            json!({"slug": slug, "bound": token.0, "by": by, "title": title, "body": body}),
        )?)
    }

    fn note(
        &self,
        slug: &str,
        text: &str,
        to: Option<&str>,
        key: &str,
        by: &str,
    ) -> Result<(), BoardError> {
        self.ok(
            "say",
            json!({"slug": slug, "sender": by, "text": text, "to": to, "key": key}),
        )
        .map(|_| ())
    }
}

/// The calls carried by running `PROGRAM api` in `cwd`, one process a call.
pub struct Process {
    pub program: PathBuf,
    pub cwd: PathBuf,
}

impl Process {
    #[must_use]
    pub fn new(program: &Path, cwd: &Path) -> Process {
        Process {
            program: program.into(),
            cwd: cwd.into(),
        }
    }
}

impl Caller for Process {
    fn call(&self, method: &str, args: Value) -> Result<Value, String> {
        let program = self.program.display();
        let mut child = Command::new(&self.program)
            .arg("api")
            .current_dir(&self.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("{program} did not start: {error}"))?;
        let call = json!({"method": method, "args": args}).to_string();
        let wrote = child
            .stdin
            .take()
            .map(|mut stdin| stdin.write_all(call.as_bytes()));
        let done = child
            .wait_with_output()
            .map_err(|error| format!("{program}: {error}"))?;
        if let Some(Err(error)) = wrote {
            return Err(format!("{program} did not read the call: {error}"));
        }
        serde_json::from_slice(&done.stdout).map_err(|error| {
            format!(
                "{program} gave no answer ({}, {error}): {}",
                done.status,
                String::from_utf8_lossy(&done.stderr).trim()
            )
        })
    }
}

#[cfg(test)]
#[path = "board_tests.rs"]
mod tests;
