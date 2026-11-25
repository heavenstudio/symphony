//! The Symphony's asks: ephemeral questions the owner asks about a place in a document.
//!
//! A comment ([`crate::edit`]) is written into the markdown and stays. An ask is a chat: the owner asks
//! about a document, a heading anchor or a card slug in the Symphony (`just symphony`), its registered
//! native chat recipient answers with `symphony ask
//! answer`, and the answer shows beside the question. A follow-up from the owner is another question on
//! the same thread and opens it again. Nothing here is written under `docs/` or committed: the store is
//! one SQLite file in the main checkout's ignored `artifacts/asks/`, shared by every worktree like the
//! task board, and a thread with no activity for a day is deleted on the next write.
//!
//! The SQLite ask store is shared across worktrees. Board registration selects
//! the listening native chat recipient.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::children::command;
use crate::clock::stamp_at;
use crate::text::{is_space, os_error, prefix_chars, repr, strip};

/// Characters in one question or answer.
pub const MAX_TEXT: usize = 4000;
/// Seconds without activity before a thread is pruned.
pub const MAX_AGE: f64 = 24.0 * 3600.0;
/// How long a write waits for another writer's lock.
const BUSY_TIMEOUT_MS: u32 = 30_000;

#[cfg(test)]
thread_local! {
    pub(crate) static SQLITE_RUNS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The asks file's schema, made if missing by whichever writer opens the file first.
const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS asks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    path TEXT NOT NULL,
    anchor TEXT NOT NULL DEFAULT '',
    quote TEXT NOT NULL DEFAULT '',
    asked REAL NOT NULL,
    updated REAL NOT NULL
);
CREATE TABLE IF NOT EXISTS messages (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    ask INTEGER NOT NULL,
    kind TEXT NOT NULL,          -- 'question' (the owner) or 'answer' (an agent)
    by TEXT NOT NULL,
    text TEXT NOT NULL,
    at REAL NOT NULL
);
CREATE INDEX IF NOT EXISTS messages_ask ON messages (ask, seq);
CREATE TABLE IF NOT EXISTS replies (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    tree TEXT NOT NULL DEFAULT '', -- the worktree's branch, '' on the main checkout
    path TEXT NOT NULL,           -- the document's route
    kind TEXT NOT NULL,           -- 'card' or 'comment'
    target TEXT NOT NULL,         -- the card's slug, or the comment's title
    by TEXT NOT NULL,
    at REAL NOT NULL
)
";

/// The name of the check a post to a missing thread fails, so its refusal reads as one.
const NO_SUCH_ASK: &str = "no_such_ask";

/// Who asks on this machine: the person at it, the author of every question a page asks with no
/// other actor named ([`AskStore::ask_by`]).
const OWNER: &str = "owner";

/// Why an ask was not stored or read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AskError {
    /// The ask itself is refused: nothing was written.
    Refused(String),
    /// The store could not be opened or read.
    Store(String),
}

impl std::fmt::Display for AskError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AskError::Refused(said) | AskError::Store(said) => f.write_str(said),
        }
    }
}

/// An agent's answer on a thread.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Answer {
    pub text: String,
    pub by: String,
    pub at: f64,
}

/// One message of a thread: a `question` (the owner's) or an `answer` (an agent's).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Message {
    pub seq: i64,
    pub kind: String,
    pub by: String,
    pub text: String,
    pub at: f64,
}

/// One ask and everything said on it, in the shape the Symphony reads.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Thread {
    pub id: i64,
    pub path: String,
    pub anchor: String,
    pub quote: String,
    /// The first question, which the thread keeps as its title.
    pub question: String,
    pub asked: f64,
    pub updated: f64,
    pub answers: Vec<Answer>,
    pub messages: Vec<Message>,
    /// `open` while the last message is a question, else `answered`.
    pub status: String,
}

/// The thread as the Symphony's API sends it, its messages not drawn yet: field for field, so a
/// reader of the store deserializes no wire shape (and the server links none of the blocks').
impl From<Thread> for symphony_api::Ask {
    fn from(thread: Thread) -> Self {
        symphony_api::Ask {
            id: thread.id,
            path: thread.path,
            anchor: thread.anchor,
            quote: thread.quote,
            question: thread.question,
            asked: thread.asked,
            updated: thread.updated,
            answers: thread
                .answers
                .into_iter()
                .map(|answer| symphony_api::AskAnswer {
                    text: answer.text,
                    by: answer.by,
                    at: answer.at,
                })
                .collect(),
            messages: thread
                .messages
                .into_iter()
                .map(|message| symphony_api::AskMessage {
                    seq: message.seq,
                    kind: message.kind,
                    by: message.by,
                    text: message.text,
                    at: message.at,
                    blocks: Vec::new(),
                    html: String::new(),
                    media: Vec::new(),
                })
                .collect(),
            status: thread.status,
        }
    }
}

/// Who answered the owner on a card or a comment, as `symphony reply --as HANDLE` names itself: the
/// register's lines say only `agent`, so the owner's ask is credited from here
/// ([`crate::owner_asks`]). Kept a day, as threads are.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Reply {
    /// The worktree's branch the reply was written in; empty on the main checkout.
    pub tree: String,
    /// The document's route.
    pub path: String,
    /// `card` or `comment`.
    pub kind: String,
    /// The card's slug, or the comment's title.
    pub target: String,
    pub by: String,
    pub at: f64,
}

/// An owner's question (a first one or a follow-up), with its thread's place.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Question {
    pub seq: i64,
    pub ask: i64,
    pub text: String,
    pub at: f64,
    pub path: String,
    pub anchor: String,
}

/// A row of a thread read: the ask, and one of its messages (none for an ask without any).
#[derive(Deserialize)]
struct Row {
    id: i64,
    path: String,
    anchor: String,
    quote: String,
    asked: f64,
    updated: f64,
    seq: Option<i64>,
    kind: Option<String>,
    by: Option<String>,
    text: Option<String>,
    at: Option<f64>,
    #[serde(default)]
    last: i64,
}

/// The id a new ask was given.
#[derive(Deserialize)]
struct Id {
    id: i64,
}

/// How many threads a prune deleted.
#[derive(Deserialize)]
struct Gone {
    gone: usize,
}

/// The seq of the last message.
#[derive(Deserialize)]
struct Last {
    last: i64,
}

/// A thread that has a message after some seq: its id and document, and the last seq there is.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Moved {
    pub id: i64,
    pub path: String,
    #[serde(skip_serializing)]
    pub last: i64,
}

/// What every thread read selects; its condition and order follow.
const THREADS: &str = "SELECT a.id, a.path, a.anchor, a.quote, a.asked, a.updated, m.seq, m.kind, m.by, \
                       m.text, m.at FROM asks a LEFT JOIN messages m ON m.ask = a.id";

/// `text` as an SQL text value, spelled in hex so nothing in it can end the statement.
pub(crate) fn text_sql(text: &str) -> String {
    use std::fmt::Write as _;
    if text.is_empty() {
        return "''".to_string();
    }
    let mut hex = String::with_capacity(text.len() * 2 + 18);
    hex.push_str("CAST(X'");
    for byte in text.bytes() {
        let _ = write!(hex, "{byte:02X}");
    }
    hex.push_str("' AS TEXT)");
    hex
}

/// `value` as an SQL number.
fn real_sql(value: f64) -> String {
    if value.is_finite() {
        format!("{value:?}")
    } else {
        "NULL".to_string()
    }
}

/// The statements that delete every thread quiet since `cutoff`.
fn prune_sql(cutoff: f64) -> String {
    let cutoff = real_sql(cutoff);
    format!(
        "DELETE FROM replies WHERE at < {cutoff};\n\
         DELETE FROM messages WHERE ask IN (SELECT id FROM asks WHERE updated < {cutoff});\n\
         DELETE FROM asks WHERE updated < {cutoff};\n"
    )
}

fn now_seconds() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |since| since.as_secs_f64())
}

/// A document's route, relative, normal and Markdown, or the refusal. Which entrypoint serves it
/// is the caller's to check against its project ([`crate::project::Project::serving`]).
pub fn check_path(path: &str) -> Result<&str, AskError> {
    let normal = !path.is_empty()
        && !path.starts_with('/')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..");
    if normal && path.ends_with(".md") {
        return Ok(path);
    }
    Err(AskError::Refused(format!(
        "path must be a markdown file, not {}",
        repr(path)
    )))
}

/// `text` stripped, or the refusal of an empty or overlong `what`.
fn said<'a>(text: &'a str, what: &str) -> Result<&'a str, AskError> {
    let text = strip(text);
    let length = text.chars().count();
    if length == 0 {
        return Err(AskError::Refused(format!("an empty {what}")));
    }
    if length > MAX_TEXT {
        return Err(AskError::Refused(format!(
            "the {what} is {length} characters; at most {MAX_TEXT}"
        )));
    }
    Ok(text)
}

/// The asks, in their SQLite file.
pub struct AskStore {
    path: PathBuf,
    clock: Box<dyn Fn() -> f64>,
}

impl AskStore {
    /// The store at `path`, made (its folder, its tables) when it is not there yet.
    pub fn open(path: &Path) -> Result<AskStore, AskError> {
        AskStore::with_clock(path, Box::new(now_seconds))
    }

    /// The same, its times read from `clock` (seconds since the epoch).
    pub fn with_clock(path: &Path, clock: Box<dyn Fn() -> f64>) -> Result<AskStore, AskError> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)
                .map_err(|problem| AskError::Store(os_error(&problem, parent)))?;
        }
        let store = AskStore {
            path: path.to_path_buf(),
            clock,
        };
        store.run(&format!(
            "PRAGMA journal_mode = WAL;\n{SCHEMA};\n{}",
            crate::callbacks::SCHEMA
        ))?;
        Ok(store)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// `script` through `sqlite3` on the file: what it printed (JSON arrays), or what it said failed.
    pub(crate) fn run(&self, script: &str) -> Result<String, AskError> {
        #[cfg(test)]
        SQLITE_RUNS.with(|runs| runs.set(runs.get() + 1));
        let failed = |problem: std::io::Error| AskError::Store(format!("sqlite3: {problem}"));
        let mut child = symphony_board::sys::spawn_piped(
            command("sqlite3")
                .args(["-bail", "-batch", "-json"])
                .arg(&self.path),
        )
        .map_err(failed)?;
        if let Some(mut input) = child.stdin.take() {
            let _ = write!(input, ".timeout {BUSY_TIMEOUT_MS}\n{script}");
        }
        let done = symphony_board::sys::wait_output(child).map_err(failed)?;
        if done.status.success() {
            return Ok(String::from_utf8_lossy(&done.stdout).into_owned());
        }
        let complaint = String::from_utf8_lossy(&done.stderr);
        let first = complaint
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .unwrap_or("exited without a word");
        Err(AskError::Store(format!("sqlite3: {first}")))
    }

    /// The rows `script` selected, every array it printed in turn.
    pub(crate) fn rows<T: serde::de::DeserializeOwned>(
        &self,
        script: &str,
    ) -> Result<Vec<T>, AskError> {
        arrays(&self.run(script)?)
    }

    /// `body` as one write: it waits for the lock, prunes first, and is all or nothing.
    fn write(&self, now: f64, body: &str) -> Result<String, AskError> {
        self.run(&format!(
            "BEGIN IMMEDIATE;\n{}{body}COMMIT;\n",
            prune_sql(now - MAX_AGE)
        ))
    }

    /// A message on thread `id`, refused when there is none.
    fn post(&self, id: i64, kind: &str, by: &str, text: &str) -> Result<(), AskError> {
        let now = (self.clock)();
        let body = format!(
            "CREATE TEMP TABLE IF NOT EXISTS guard (ok INTEGER CONSTRAINT {NO_SUCH_ASK} CHECK (ok));\n\
             INSERT INTO guard SELECT EXISTS (SELECT 1 FROM asks WHERE id = {id});\n\
             INSERT INTO messages (ask, kind, by, text, at) VALUES ({id}, {}, {}, {}, {});\n\
             UPDATE asks SET updated = {} WHERE id = {id};\n",
            text_sql(kind),
            text_sql(by),
            text_sql(text),
            real_sql(now),
            real_sql(now),
        );
        match self.write(now, &body) {
            Err(AskError::Store(problem)) if problem.contains(NO_SUCH_ASK) => {
                Err(AskError::Refused(format!(
                    "no ask {id} (asks go after a day without activity)"
                )))
            }
            other => other.map(drop),
        }
    }

    /// A new thread on `path` (and a heading anchor or card slug); its id.
    pub fn ask(
        &self,
        path: &str,
        question: &str,
        anchor: &str,
        quote: &str,
    ) -> Result<i64, AskError> {
        self.ask_by(path, question, anchor, quote, OWNER)
    }

    /// [`Self::ask`] by `by`, the short name of the person who asked (`symphony_plugin::Actor`):
    /// `owner` for the person at the machine.
    pub fn ask_by(
        &self,
        path: &str,
        question: &str,
        anchor: &str,
        quote: &str,
        by: &str,
    ) -> Result<i64, AskError> {
        let path = check_path(path)?;
        let question = said(question, "question")?;
        let quote = prefix_chars(strip(quote), MAX_TEXT);
        let now = real_sql((self.clock)());
        let body = format!(
            "INSERT INTO asks (path, anchor, quote, asked, updated) VALUES ({}, {}, {}, {now}, {now});\n\
             INSERT INTO messages (ask, kind, by, text, at) VALUES (last_insert_rowid(), 'question', {}, {}, {now});\n\
             SELECT ask AS id FROM messages WHERE seq = last_insert_rowid();\n",
            text_sql(path),
            text_sql(strip(anchor)),
            text_sql(quote),
            text_sql(by),
            text_sql(question),
        );
        let printed = self.write((self.clock)(), &body)?;
        serde_json::from_slice::<Vec<Id>>(printed.as_bytes())
            .ok()
            .and_then(|ids| ids.first().map(|row| row.id))
            .ok_or_else(|| AskError::Store(format!("sqlite3 gave no id: {printed:?}")))
    }

    /// The owner's follow-up: another question on the thread, which is open again.
    pub fn reply(&self, id: i64, text: &str) -> Result<(), AskError> {
        self.reply_by(id, text, OWNER)
    }

    /// [`Self::reply`] by `by`, the short name of the person who asked ([`Self::ask_by`]).
    pub fn reply_by(&self, id: i64, text: &str, by: &str) -> Result<(), AskError> {
        let text = said(text, "question")?;
        self.post(id, "question", by, text)
    }

    /// An agent's markdown answer; the thread is answered until the owner follows up.
    pub fn answer(&self, id: i64, text: &str, by: &str) -> Result<(), AskError> {
        let text = said(text, "answer")?;
        let by = match strip(by) {
            "" => "agent",
            by => by,
        };
        self.post(id, "answer", by, text)
    }

    /// Who wrote a reply now: `reply`'s place and session, its time the store's clock.
    pub fn record_reply(&self, reply: &Reply) -> Result<(), AskError> {
        let now = (self.clock)();
        let body = format!(
            "INSERT INTO replies (tree, path, kind, target, by, at) VALUES ({}, {}, {}, {}, {}, {});\n",
            text_sql(&reply.tree),
            text_sql(&reply.path),
            text_sql(&reply.kind),
            text_sql(&reply.target),
            text_sql(&reply.by),
            real_sql(now),
        );
        self.write(now, &body).map(drop)
    }

    /// Every reply recorded in the last day, the oldest first.
    pub fn replies(&self) -> Result<Vec<Reply>, AskError> {
        self.rows("SELECT tree, path, kind, target, by, at FROM replies ORDER BY seq;\n")
    }

    /// Delete the threads with no activity for `max_age` seconds; how many went.
    pub fn prune(&self, max_age: f64) -> Result<usize, AskError> {
        let now = (self.clock)();
        let printed = self.run(&format!(
            "BEGIN IMMEDIATE;\n{}SELECT changes() AS gone;\nCOMMIT;\n",
            prune_sql(now - max_age)
        ))?;
        Ok(serde_json::from_slice::<Vec<Gone>>(printed.as_bytes())
            .ok()
            .and_then(|rows| rows.first().map(|row| row.gone))
            .unwrap_or(0))
    }

    /// The threads `rows` hold, in the order they come.
    fn threads(rows: Vec<Row>) -> Vec<Thread> {
        let mut threads: Vec<Thread> = Vec::new();
        for row in rows {
            if threads.last().is_none_or(|thread| thread.id != row.id) {
                threads.push(Thread {
                    id: row.id,
                    path: row.path.clone(),
                    anchor: row.anchor.clone(),
                    quote: row.quote.clone(),
                    question: String::new(),
                    asked: row.asked,
                    updated: row.updated,
                    answers: Vec::new(),
                    messages: Vec::new(),
                    status: "answered".to_string(),
                });
            }
            let thread = threads.last_mut().expect("the thread just pushed");
            if let (Some(seq), Some(kind), Some(by), Some(text), Some(at)) =
                (row.seq, row.kind, row.by, row.text, row.at)
            {
                if kind == "answer" {
                    thread.answers.push(Answer {
                        text: text.clone(),
                        by: by.clone(),
                        at,
                    });
                }
                if thread.messages.is_empty() {
                    thread.question.clone_from(&text);
                }
                thread.status = if kind == "question" {
                    "open"
                } else {
                    "answered"
                }
                .to_string();
                thread.messages.push(Message {
                    seq,
                    kind,
                    by,
                    text,
                    at,
                });
            }
        }
        threads
    }

    fn select(&self, condition: &str) -> Result<Vec<Thread>, AskError> {
        Ok(AskStore::threads(
            self.rows(&format!("{THREADS} {condition}, m.seq;\n"))?,
        ))
    }

    /// One thread, refused when there is none.
    pub fn thread(&self, id: i64) -> Result<Thread, AskError> {
        self.select(&format!("WHERE a.id = {id} ORDER BY a.id"))?
            .into_iter()
            .next()
            .ok_or_else(|| {
                AskError::Refused(format!(
                    "no ask {id} (asks go after a day without activity)"
                ))
            })
    }

    /// Every thread, the most recently active first.
    pub fn all(&self) -> Result<Vec<Thread>, AskError> {
        self.select("ORDER BY a.updated DESC, a.id DESC")
    }

    /// This document's threads, newest first.
    pub fn for_doc(&self, path: &str) -> Result<Vec<Thread>, AskError> {
        self.select(&format!(
            "WHERE a.path = {} ORDER BY a.asked DESC, a.id DESC",
            text_sql(path)
        ))
    }

    /// The threads waiting for an answer, the longest waiting first.
    pub fn open_asks(&self) -> Result<Vec<Thread>, AskError> {
        Ok(self.waiting()?.0)
    }

    /// The threads waiting for an answer, and the last message's seq, read at one moment: what a
    /// listener starts from, so a question asked as it starts is said once.
    fn waiting(&self) -> Result<(Vec<Thread>, i64), AskError> {
        let rows: Vec<Row> = self.rows(
            "SELECT a.id, a.path, a.anchor, a.quote, a.asked, a.updated, m.seq, m.kind, m.by, m.text, m.at, \
             (SELECT COALESCE(MAX(seq), 0) FROM messages) AS last FROM asks a LEFT JOIN messages m ON m.ask = a.id \
             ORDER BY a.updated, a.id, m.seq;\n",
        )?;
        let last = rows.first().map_or(0, |row| row.last);
        let open = AskStore::threads(rows)
            .into_iter()
            .filter(|thread| thread.status == "open")
            .collect();
        Ok((open, last))
    }

    /// Each owner question (a first one or a follow-up) after message `seq`, oldest first.
    pub fn questions_since(&self, seq: i64) -> Result<Vec<Question>, AskError> {
        self.rows(&format!(
            "SELECT m.seq, m.ask, m.text, m.at, a.path, a.anchor FROM messages m JOIN asks a ON a.id = m.ask \
             WHERE m.kind = 'question' AND m.seq > {seq} ORDER BY m.seq;\n"
        ))
    }

    /// The seq of the last message there is, 0 when none.
    pub fn last_seq(&self) -> Result<i64, AskError> {
        let rows: Vec<Last> = self.rows("SELECT COALESCE(MAX(seq), 0) AS last FROM messages;\n")?;
        Ok(rows.first().map_or(0, |row| row.last))
    }

    /// Each thread with a message after `seq`, by id, each carrying the last seq there is: what a
    /// reader that saw up to `seq` reads again. None when nothing was said since.
    pub fn moved_since(&self, seq: i64) -> Result<Vec<Moved>, AskError> {
        self.rows(&format!(
            "SELECT a.id, a.path, (SELECT MAX(seq) FROM messages) AS last FROM asks a \
             WHERE a.id IN (SELECT ask FROM messages WHERE seq > {seq}) ORDER BY a.id;\n"
        ))
    }

    /// The store at `path` to read only, when its file is there: unlike [`AskStore::open`] it makes
    /// no folder, file or table, so a watcher looking at it never writes.
    pub fn existing(path: &Path) -> Option<AskStore> {
        path.is_file().then(|| AskStore {
            path: path.to_path_buf(),
            clock: Box::new(now_seconds),
        })
    }
}

/// Every JSON array `printed` holds, one after another, as `sqlite3 -json` prints each statement's
/// rows. Read as bytes, through the one reader the crate reads JSON with: the same rows, and the same
/// error in the same words and at the same line and column, as a read of the text.
fn arrays<T: serde::de::DeserializeOwned>(printed: &str) -> Result<Vec<T>, AskError> {
    let mut rows = Vec::new();
    for array in serde_json::Deserializer::from_slice(printed.as_bytes()).into_iter::<Vec<T>>() {
        rows.extend(array.map_err(|problem| {
            AskError::Store(format!("sqlite3 printed what does not read: {problem}"))
        })?);
    }
    Ok(rows)
}

/// `ASKS_DB` when set, else, for a folder this process serves, the journal found from it when it
/// began to serve it ([`crate::taskview::serve`]), else the one found from it now ([`found_path`]).
pub fn default_path(root: &Path, asks_db: Option<&str>) -> PathBuf {
    if let Some(db) = asks_db.filter(|db| !db.is_empty()) {
        return PathBuf::from(db);
    }
    crate::taskview::served_journal(root).unwrap_or_else(|| found_path(root))
}

/// `asks/asks.db` in the artifacts folder (`[project] artifacts`, else `artifacts/`, read alone:
/// [`symphony_board::project::artifacts`]) of the main checkout every worktree of `root`'s
/// repository shares, else under `root`. A folder left without its `.git` finds the repository of
/// the checkout that holds it, and so that checkout's journal.
pub(crate) fn found_path(root: &Path) -> PathBuf {
    let mut base = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let common = symphony_board::sys::output(crate::children::git().arg("-C").arg(root).args([
        "rev-parse",
        "--path-format=absolute",
        "--git-common-dir",
    ]))
    .ok()
    .filter(|done| done.status.success())
    .map(|done| String::from_utf8_lossy(&done.stdout).trim().to_string())
    .unwrap_or_default();
    let common = PathBuf::from(common);
    if common.file_name().is_some_and(|name| name == ".git") {
        let common = std::fs::canonicalize(&common).unwrap_or(common);
        if let Some(checkout) = common.parent() {
            base = checkout.to_path_buf();
        }
    }
    // The artifacts folder read alone and quietly, as the board finds the store: no plugin's
    // process starts for a path, and a project that does not load keeps its store where it is.
    let artifacts = symphony_board::project::artifacts(&base).unwrap_or_else(|| "artifacts".into());
    base.join(format!("{artifacts}/asks/asks.db"))
}

/// `ASK <id> <path>#<anchor> — <question>` on one line.
pub fn watch_line(ask: i64, path: &str, anchor: &str, text: &str) -> String {
    let place = if anchor.is_empty() {
        path.to_string()
    } else {
        format!("{path}#{anchor}")
    };
    let words: Vec<&str> = text.split(is_space).filter(|w| !w.is_empty()).collect();
    format!("ASK {ask} {place} — {}", words.join(" "))
}

impl Question {
    pub fn watch_line(&self) -> String {
        watch_line(self.ask, &self.path, &self.anchor, &self.text)
    }
}

/// What a listener hears of the asks: first every open ask's last question, then each new question
/// once.
#[derive(Default)]
pub struct Poller {
    seen: Option<i64>,
}

impl Poller {
    pub fn new() -> Poller {
        Poller::default()
    }

    /// One look at the store, each line said to `emit`.
    pub fn poll(&mut self, store: &AskStore, emit: &mut dyn FnMut(&str)) -> Result<(), AskError> {
        let Some(seen) = self.seen else {
            let (open, last) = store.waiting()?;
            self.seen = Some(last);
            for thread in open {
                if let Some(message) = thread.messages.last() {
                    emit(&watch_line(
                        thread.id,
                        &thread.path,
                        &thread.anchor,
                        &message.text,
                    ));
                }
            }
            return Ok(());
        };
        for question in store.questions_since(seen)? {
            emit(&question.watch_line());
            self.seen = Some(question.seq);
        }
        Ok(())
    }
}

// -- the command line: `symphony ask …` -----------------------------------------------------------

const USAGE: &str = "usage: symphony ask [-h] [--root ROOT] [--db PATH] {list,show,answer} ...";
const HELP: &str = "The Symphony's asks: ephemeral questions the owner asks about a place in a doc.

  symphony ask list [--open]                      every ask, most recently active first
  symphony ask show ID                            one thread
  symphony ask answer ID \"markdown\" [--as HANDLE] answer an ask ([[doc#heading]] links and images render)

options:
  -h, --help   show this help message and exit
  --root ROOT  the checkout to find the store from (ASKS_DB names it outright)
  --db PATH    the exact asks store (also used by scratch-server callbacks)
  --open       list only the asks waiting for an answer
  --as HANDLE  your task-board handle (default: agent); --by HANDLE reads the same";

fn when(stamp: f64) -> String {
    // the minute it fell in: whole seconds, as Python's strftime drops the fraction
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a time in seconds since the epoch fits an i64 many times over"
    )]
    let seconds = stamp.floor() as i64;
    stamp_at(seconds)
}

fn place(thread: &Thread) -> String {
    if thread.anchor.is_empty() {
        thread.path.clone()
    } else {
        format!("{}#{}", thread.path, thread.anchor)
    }
}

fn summary(thread: &Thread) -> String {
    let last = thread.messages.last().map_or(String::new(), |m| {
        m.text
            .split(is_space)
            .filter(|w| !w.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    });
    format!(
        "{}  {:<8} {}  {} — {}",
        thread.id,
        thread.status,
        when(thread.updated),
        place(thread),
        prefix_chars(&last, 120)
    )
}

fn show(thread: &Thread) -> String {
    let mut lines = vec![format!(
        "ask {} · {} · {}#{}",
        thread.id, thread.status, thread.path, thread.anchor
    )];
    if !thread.quote.is_empty() {
        lines.push(format!("> {}", thread.quote.replace('\n', "\n> ")));
    }
    for m in &thread.messages {
        lines.push(format!("[{}] {} — {}", when(m.at), m.by, m.text));
    }
    lines.join("\n")
}

/// `symphony ask [--root ROOT] list [--open] | show ID | answer ID TEXT [--as HANDLE]` (`--by`
/// reads the same); `ASKS_DB` in `env`
/// names the store. Its exit code.
pub fn main(
    args: &[String],
    env: &BTreeMap<String, String>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    use crate::cli::{Flag, command_at, parse, refuse};
    let root_flag = [
        Flag {
            name: "--root",
            value: true,
        },
        Flag {
            name: "--db",
            value: true,
        },
    ];
    // as argparse's subcommands take them: `--root` before the command, its own options after it
    let at = command_at(args, &root_flag);
    let own: &[Flag] = match args.get(at).map(String::as_str) {
        Some("list") => &[Flag {
            name: "--open",
            value: false,
        }],
        Some("answer") => &[
            Flag {
                name: "--as",
                value: true,
            },
            Flag {
                name: "--by",
                value: true,
            },
        ],
        _ => &[],
    };
    let (top, parsed) = match (parse(&args[..at], &root_flag), parse(&args[at..], own)) {
        (Ok(top), Ok(parsed)) if top.help || parsed.help => {
            let _ = writeln!(out, "{USAGE}\n\n{HELP}");
            return 0;
        }
        (Ok(top), Ok(parsed)) => (top, parsed),
        (Err(problem), _) | (_, Err(problem)) => return refuse(err, USAGE, "ask", &problem),
    };
    let words: Vec<&str> = parsed.positionals.iter().map(String::as_str).collect();
    let id = |word: &str| -> Result<i64, String> {
        match word.parse::<i64>() {
            Ok(id) => Ok(id),
            Err(_) => Err(format!("argument id: invalid int value: {}", repr(word))),
        }
    };
    let command = match words.as_slice() {
        ["list"] => Ok(None),
        ["show", ask] => id(ask).map(|ask| Some((ask, None))),
        ["answer", ask, text] => id(ask).map(|ask| Some((ask, Some(*text)))),
        [] => Err("the following arguments are required: command".to_string()),
        [other, ..] if !["list", "show", "answer"].contains(other) => Err(format!(
            "argument command: invalid choice: {} (choose from 'list', 'show', 'answer')",
            repr(other)
        )),
        ["show"] | ["answer"] => Err("the following arguments are required: id".to_string()),
        ["answer", _] => Err("the following arguments are required: text".to_string()),
        [_, rest @ ..] => Err(format!("unrecognized arguments: {}", rest.join(" "))),
    };
    let command = match command {
        Ok(command) => command,
        Err(problem) => return refuse(err, USAGE, "ask", &problem),
    };
    let root = PathBuf::from(top.value("--root").unwrap_or("."));
    let db = top.value("--db").map_or_else(
        || default_path(&root, env.get("ASKS_DB").map(String::as_str)),
        PathBuf::from,
    );
    // Through `just symphony-ask`, which an agent runs unasked, the store (which opening makes)
    // is in the checkout, or from a linked worktree the main checkout's artifacts folder, however
    // `--db` or `--root` named it.
    let marker = symphony_board::confine::MARKER;
    if env.get(marker).is_some_and(|marked| !marked.is_empty()) {
        let inside = std::env::current_dir()
            .map_err(|e| format!("no folder to run in: {e}"))
            .and_then(|here| symphony_board::confine::inside(&here, &db));
        if let Err(problem) = inside {
            let _ = writeln!(err, "✖ {problem}");
            return 1;
        }
    }
    let done = AskStore::open(&db).and_then(|store| match command {
        None => {
            let threads = if parsed.flag("--open") {
                store.open_asks()?
            } else {
                store.all()?
            };
            for thread in &threads {
                let _ = writeln!(out, "{}", summary(thread));
            }
            if threads.is_empty() {
                let none = if parsed.flag("--open") {
                    "no open asks"
                } else {
                    "no asks"
                };
                let _ = writeln!(out, "{none}");
            }
            Ok(())
        }
        Some((ask, None)) => {
            let _ = writeln!(out, "{}", show(&store.thread(ask)?));
            Ok(())
        }
        Some((ask, Some(text))) => {
            let by = parsed.value("--as").or_else(|| parsed.value("--by"));
            store.answer(ask, text, by.unwrap_or("agent"))?;
            let _ = writeln!(out, "answered ask {ask}");
            Ok(())
        }
    });
    match done {
        Ok(()) => 0,
        Err(problem) => {
            let _ = writeln!(err, "✖ {problem}");
            1
        }
    }
}

#[cfg(test)]
#[path = "asks_tests.rs"]
mod tests;
