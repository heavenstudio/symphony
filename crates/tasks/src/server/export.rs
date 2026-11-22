//! What a refresh reads of a board's database as it runs, never stopping or starting it:
//! [`Home::export`], a snapshot of one of its databases, taken over the running server's own
//! connection (`surreal export` into a SurrealQL file, which `surreal import` restores, or on a
//! PostgreSQL board `pg_dump` into an SQL file, which `psql` restores); [`Home::schema`], the
//! schema its board is at; and [`Home::writers`], who writes it. Each reads the board on the
//! engine its home names ([`Home::backend`]); a PostgreSQL board's programs are its project's
//! (`[board] postgresql`, [`super::postgres::discover_programs`]). None needs a stop or sends a
//! signal: a store whose server is not running is refused, and nothing is started, so a snapshot
//! never stands in for a stop of the board's database.

use super::{Engine, Home, Postgres, Running, ServerError, io};
use crate::rpc::{self, Client};
use crate::store::Database;
use crate::sys::command;
use crate::{NAMESPACE, Store, Writer};
use std::fs;
use std::path::Path;
use std::time::Duration;

/// The longest an export may take: past it, the `surreal export` client it started is stopped and
/// nothing is written. The database's own server is never signalled.
pub const EXPORT_LIMIT: Duration = Duration::from_secs(5 * 60);

impl Home {
    /// This board's PostgreSQL and its server's record, when the home names that engine and its
    /// server runs: none for a SurrealDB board. Nothing is started.
    fn postgresql_now(&self) -> Result<Option<(Postgres, Running)>, ServerError> {
        if self.backend()? != Engine::PostgreSql {
            return Ok(None);
        }
        let root = crate::sweep::trigger::main_checkout(&self.within).ok_or_else(|| {
            ServerError::Io(format!(
                "the board in {} is on PostgreSQL, and {} is no main checkout's git folder to \
                 read its project's PostgreSQL programs from",
                self.dir.display(),
                self.within.display()
            ))
        })?;
        let postgres = self.project_postgresql(root)?;
        let Some(serving) = postgres.current()? else {
            return Err(ServerError::NotUp(format!(
                "the board's PostgreSQL in {} is not running: nothing is read or exported, and \
                 nothing is started",
                self.dir.display()
            )));
        };
        let running = Running {
            addr: format!("postgresql:{}", serving.socket.display()),
            pid: serving.pid,
            version: serving.version,
        };
        Ok(Some((postgres, running)))
    }

    /// `database` of the board's running server, on the engine the home names, and where it is.
    fn reached(&self, database: &str) -> Result<(Database, String), ServerError> {
        Ok(match self.postgresql_now()? {
            Some((postgres, running)) => (
                crate::pg::Client::new(&postgres.socket(), database).into(),
                running.addr,
            ),
            None => {
                let running = self.running_now()?;
                (
                    Client::new(&running.addr, NAMESPACE, database).into(),
                    running.addr,
                )
            }
        })
    }

    /// The recorded server, when it runs as recorded: on this board's data, and answering.
    fn running_now(&self) -> Result<Running, ServerError> {
        let Some(running) = self.recorded() else {
            return Err(ServerError::NotUp(format!(
                "the board's database in {} is not running (no address is recorded): nothing is \
                 read or exported, and nothing is started",
                self.dir.display()
            )));
        };
        if !self.serves(running.pid) || !rpc::healthy(&running.addr) {
            return Err(ServerError::NotUp(format!(
                "the board's database in {} is not running as recorded (process {} at {}): \
                 nothing is read or exported, and nothing is started",
                self.dir.display(),
                running.pid,
                running.addr
            )));
        }
        Ok(running)
    }

    /// The schema of the board in the database `database`, as the recorded server serves it now:
    /// none for a board not made yet. One read, which defines and writes nothing.
    ///
    /// # Errors
    ///
    /// When the board's database is not running, or the read fails.
    pub fn schema(&self, database: &str) -> Result<Option<i64>, ServerError> {
        let (reached, addr) = self.reached(database)?;
        Store::unchecked_on(
            reached,
            Box::new(crate::store::now),
            Box::new(|_| true),
            "",
            &crate::Opening::write(),
        )
        .board_version()
        .map_err(|e| {
            ServerError::Io(format!(
                "the board's schema in {NAMESPACE}/{database} at {} cannot be read: {e}",
                addr
            ))
        })
    }

    /// Every writer's line on the board in the database `database`, as the recorded server serves
    /// it now (`crate::store::Writer`): none for a board below the writer record or not made yet.
    /// One read, which defines and writes nothing.
    ///
    /// # Errors
    ///
    /// When the board's database is not running, or the read fails.
    pub fn writers(&self, database: &str) -> Result<Vec<Writer>, ServerError> {
        let (reached, addr) = self.reached(database)?;
        Store::unchecked_on(
            reached,
            Box::new(crate::store::now),
            Box::new(|_| true),
            "",
            &crate::Opening::write(),
        )
        .writers()
        .map_err(|e| {
            ServerError::Io(format!(
                "the board's writers in {NAMESPACE}/{database} at {} cannot be read: {e}",
                addr
            ))
        })
    }

    /// Write the database `database` of the board's namespace, as the recorded server serves it now,
    /// to `file` (whole: its folder made, a partial export never left at its name): the server it
    /// read. Refused, writing nothing, when no recorded server of this board's data answers, or when
    /// the pinned `surreal` is not there; nothing is started, stopped or signalled.
    ///
    /// # Errors
    ///
    /// When the board's database is not running, the binary is missing, or the export fails.
    pub fn export(&self, database: &str, file: &Path) -> Result<Running, ServerError> {
        self.export_within(database, file, EXPORT_LIMIT)
    }

    /// [`Home::export`] within `limit` in place of [`EXPORT_LIMIT`]: an export still running then
    /// is stopped (the `surreal export` client this started, never the database's server), its
    /// partial file removed, and refused.
    ///
    /// # Errors
    ///
    /// As [`Home::export`], and when the export does not end within `limit`.
    pub fn export_within(
        &self,
        database: &str,
        file: &Path,
        limit: Duration,
    ) -> Result<Running, ServerError> {
        let postgresql = self.postgresql_now()?;
        if postgresql.is_none() && !self.surreal.is_file() {
            return Err(ServerError::NoSurreal(self.surreal.clone()));
        }
        // Before anything is made: a board whose database does not run leaves no folder.
        let served = match postgresql {
            Some((postgres, running)) => (Some(postgres), running),
            None => (None, self.running_now()?),
        };
        let folder = file.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(folder).map_err(io("could not make", folder))?;
        let name = file
            .file_name()
            .map_or_else(|| "export".into(), |name| name.to_string_lossy());
        let partial = folder.join(format!(".{name}.{}.partial", std::process::id()));
        let (mut export, program, running) = match served {
            (Some(postgres), running) => {
                assert!(crate::db::plain(database), "a database is a plain name");
                let program = postgres.program("pg_dump");
                let mut export = command(&program);
                export
                    .args([
                        "--no-owner",
                        "--no-password",
                        "-U",
                        super::postgres::USER,
                        "-h",
                    ])
                    .arg(postgres.socket())
                    .args(["-d", database, "-f"])
                    .arg(&partial);
                (export, program, running)
            }
            (None, running) => {
                let mut export = command(&self.surreal);
                export
                    .args(["export", "--endpoint", &format!("http://{}", running.addr)])
                    .args(["--namespace", NAMESPACE, "--database", database])
                    .arg(&partial);
                (export, self.surreal.clone(), running)
            }
        };
        let done = crate::cli::probes::within(&mut export, limit).map_err(|why| {
            let _ = fs::remove_file(&partial);
            ServerError::Io(format!(
                "the export of {NAMESPACE}/{database} at {} with {} did not start or end within {} \
                 s ({why}): nothing was written",
                running.addr,
                program.display(),
                limit.as_secs()
            ))
        })?;
        if !done.status.success() {
            let _ = fs::remove_file(&partial);
            return Err(ServerError::Io(format!(
                "the export of {NAMESPACE}/{database} at {} failed: {}",
                running.addr,
                String::from_utf8_lossy(&done.stderr).trim()
            )));
        }
        fs::rename(&partial, file).map_err(|e| {
            let _ = fs::remove_file(&partial);
            io("could not write", file)(e)
        })?;
        Ok(running)
    }
}
