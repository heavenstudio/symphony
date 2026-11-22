//! Who writes a board: each program that opens it to write keeps a line in the `writer` table
//! (`writer:[host, path]`), naming the program, where it is, the schema it was built at, its host and
//! when it was last seen, so `tasks migrate` can name each writer the move would leave behind before
//! it moves anything ([`Store::writers_behind`]).
//!
//! The line arrives with schema [`SINCE`]: a build below it never writes one, and a board below it
//! has no table to hold one. It is read in the opening's one read of the board's schema
//! ([`sql::OPENING`]) and written only when it is missing, names another schema, or is [`EVERY`] old, so a
//! board command adds no request in the common case and "last seen" is good to the hour. A writer
//! refused a board behind its build (a project's live board that only `tasks migrate` moves) still
//! writes its line, so a rebuilt writer stops counting as behind once it has run; one refused a board
//! ahead of its build writes nothing, refused before any write. The table is schemaless and its line
//! is written without the schema guard, as builds of other schemas write it; it has no event, so a
//! line written moves no board marker. Its times are this machine's clock ([`seen_now`]).

use super::statement::both;
use super::surreal::writers as sql;
use super::{Failure, Store};
use crate::board_health::ago;
use crate::db::DbError;
use crate::schema::{Access, Schema};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::Path;
use std::sync::OnceLock;

/// The schema the record arrives with: a build below it writes no line.
pub const SINCE: i64 = 18;
// A build whose schema is below the record's would write no line at all.
const _: () = assert!(crate::schema::VERSION >= SINCE);

/// How old a line is, in seconds, before an opening writes it again: an hour.
pub const EVERY: f64 = 3600.0;

/// How recently a writer was seen, in seconds, for `tasks migrate` to count it: a week.
pub const RECENT: f64 = 7.0 * 24.0 * 3600.0;

/// The time a line is seen at and a migrate reads lines by: this machine's clock, never a store's
/// own ([`super::Clock`], which a test or `TASKS_NOW` pins), since a line says when a program on
/// the machine last ran, and an opening never reads the store's clock: a rule's first read of it is
/// its own.
pub(super) fn seen_now() -> f64 {
    super::now()
}

/// A program that opens a board, as its line names it: its file name and where it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    pub name: String,
    pub path: String,
}

impl Program {
    /// This process's: the executable it runs, its links resolved, else the name it was started by.
    pub fn this() -> Program {
        static THIS: OnceLock<Program> = OnceLock::new();
        THIS.get_or_init(|| {
            let exe = std::env::current_exe()
                .ok()
                .map(|exe| exe.canonicalize().unwrap_or(exe))
                .or_else(|| std::env::args_os().next().map(Into::into));
            match exe {
                Some(exe) => Program::at(&exe),
                None => Program::named("unknown", "unknown"),
            }
        })
        .clone()
    }

    /// The program at `path`, named by its file name.
    pub fn at(path: &Path) -> Program {
        let name = path.file_name().unwrap_or(path.as_os_str());
        Program::named(&name.to_string_lossy(), &path.to_string_lossy())
    }

    pub fn named(name: &str, path: &str) -> Program {
        Program {
            name: name.into(),
            path: path.into(),
        }
    }
}

/// A writer's line, as the board keeps it.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Writer {
    /// The program's file name.
    pub program: String,
    /// Where the program is, on `host`.
    pub path: String,
    pub host: String,
    /// The schema the program was built at.
    pub schema: i64,
    /// When it last opened the board to write, by this machine's clock ([`seen_now`]).
    pub seen: f64,
}

/// What the opening's read found of its own line: no table to hold one (a board below [`SINCE`], or
/// not made yet), or the table and the line in it, if any.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Line {
    NoTable,
    Held(Option<Kept>),
}

/// The part of a line that says whether to write it again.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub(super) struct Kept {
    schema: i64,
    seen: f64,
}

/// The opening's read's variables, for `program` on `host`.
pub(super) fn opening_vars(program: &Program, host: &str) -> Value {
    json!({"host": host, "path": program.path})
}

/// The board's version, the line and the board's additive steps [`sql::OPENING`] answered with.
pub(super) fn opened(answer: &[Value]) -> (Option<i64>, Line, i64) {
    let read = answer.last().and_then(Value::as_array);
    let found = read.and_then(|read| read.first()).and_then(Value::as_i64);
    let line = match read.and_then(|read| read.get(1)) {
        Some(Value::Bool(false)) => Line::NoTable,
        Some(kept) => Line::Held(crate::objects::read(kept).ok()),
        None => Line::NoTable,
    };
    let additions = (read.and_then(|read| read.get(2)))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    (found, line, additions)
}

/// Whether a writer of `schema` whose line is `line` writes it again at `now`.
pub(super) fn due(line: &Line, schema: i64, now: f64) -> bool {
    match line {
        Line::NoTable => false,
        Line::Held(None) => true,
        Line::Held(Some(kept)) => kept.schema != schema || now - kept.seen >= EVERY,
    }
}

/// The writers of `lines` behind `target` seen since `now` less [`RECENT`], `program` on `host` left
/// out (the migrate itself, rebuilt at `target` since its line was written): by program, host and
/// path.
pub(super) fn behind(
    lines: Vec<Writer>,
    target: i64,
    now: f64,
    program: &Program,
    host: &str,
) -> Vec<Writer> {
    let mut behind: Vec<Writer> = lines
        .into_iter()
        .filter(|line| line.schema < target && now - line.seen <= RECENT)
        .filter(|line| !(line.host == host && line.path == program.path))
        .collect();
    crate::order::sort_by(&mut behind, &|a, b| {
        (&a.program, &a.host, &a.path).cmp(&(&b.program, &b.host, &b.path))
    });
    behind
}

/// `tasks migrate`'s refusal to move `board` (as a refusal names it) to `target` while `behind`, never
/// empty, would be left behind, said at `now`.
pub(super) fn left_behind(board: &str, target: i64, behind: &[Writer], now: f64) -> String {
    let each: Vec<String> = behind
        .iter()
        .map(|w| {
            format!(
                "{} at schema {} ({} on {}, last seen {} ago)",
                w.program,
                w.schema,
                w.path,
                w.host,
                ago(now - w.seen)
            )
        })
        .collect();
    format!(
        "{board} has {} seen in the last week behind schema {target}, the one this migrate moves it \
         to, and each would be refused the board from then on: {}. Nothing was moved: rebuild each at \
         schema {target} and run it once, which records it, then `just tasks migrate` again; or \
         `just tasks migrate --leave-behind` moves it now and leaves them refused",
        match behind.len() {
            1 => "a writer".to_string(),
            n => format!("{n} writers"),
        },
        each.join("; ")
    )
}

impl Store {
    /// Every writer's line on this board, none where the table, or the board's database itself, is
    /// not there: one read.
    pub fn writers(&self) -> Result<Vec<Writer>, super::Error> {
        self.retrying(|| self.writer_lines())
    }

    fn writer_lines(&self) -> Result<Vec<Writer>, Failure> {
        let lines = both(sql::LINES, super::postgres::writers::LINES);
        let found = match self.request(lines, json!({})) {
            Ok(found) => found,
            Err(DbError::Failed(said)) if self.db.undefined(&said) => {
                return Ok(Vec::new());
            }
            Err(other) => return Err(other.into()),
        };
        let lines = found.last().cloned().unwrap_or(Value::Null);
        crate::objects::read(&lines)
            .map_err(|e| Failure::Stop(super::Error::Db(format!("a writer's line: {e}"))))
    }

    /// The writers seen in the last week that are behind `target`, the schema a migrate moves this
    /// board to, `program` (the migrate's) left out, as [`seen_now`] reads the time.
    pub(super) fn writers_behind(
        &self,
        target: i64,
        program: &Program,
    ) -> Result<Vec<Writer>, Failure> {
        let lines = self.writer_lines()?;
        Ok(behind(lines, target, seen_now(), program, &self.host))
    }

    /// `program`'s line, as a build of `schema` opening this board for `access`, written when `line`
    /// says it is due ([`due`]). Only a writer of [`SINCE`] or later writes one, and a line that is
    /// not written leaves the opening as it was: the board is opened all the same.
    pub(super) fn record_writer(
        &self,
        program: &Program,
        schema: &Schema,
        access: Access,
        line: &Line,
    ) {
        if access != Access::Write || schema.version() < SINCE {
            return;
        }
        let now = seen_now();
        if !due(line, schema.version(), now) {
            return;
        }
        let _ = self.request(
            both(sql::WRITE_LINE, super::postgres::writers::WRITE_LINE),
            json!({
                "host": self.host,
                "path": program.path,
                "program": program.name,
                "schema": schema.version(),
                "seen": now,
            }),
        );
    }
}

#[cfg(test)]
#[path = "writers_tests.rs"]
mod tests;
