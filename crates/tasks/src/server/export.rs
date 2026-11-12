//! What a refresh reads of a board's database as it runs, never stopping or starting it:
//! [`Home::export`], a snapshot of one of its databases, `surreal export` over the running server's
//! own connection into a SurrealQL file, which `surreal import` restores; and [`Home::schema`], the
//! schema its board is at. Neither needs a stop or sends a signal: a store whose server is not
//! running is refused, and nothing is started, so a snapshot never stands in for a stop of the
//! board's database.

use super::{Home, Running, ServerError, io};
use crate::rpc::{self, Client};
use crate::sys::command;
use crate::{NAMESPACE, Store};
use std::fs;
use std::path::Path;
use std::time::Duration;

/// The longest an export may take: past it, the `surreal export` client it started is stopped and
/// nothing is written. The database's own server is never signalled.
pub const EXPORT_LIMIT: Duration = Duration::from_secs(5 * 60);

impl Home {
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
        let running = self.running_now()?;
        Store::unchecked(
            Client::new(&running.addr, NAMESPACE, database),
            Box::new(crate::store::now),
            Box::new(|_| true),
            "",
        )
        .board_version()
        .map_err(|e| {
            ServerError::Io(format!(
                "the board's schema in {NAMESPACE}/{database} at {} cannot be read: {e}",
                running.addr
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
        if !self.surreal.is_file() {
            return Err(ServerError::NoSurreal(self.surreal.clone()));
        }
        let running = self.running_now()?;
        let folder = file.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(folder).map_err(io("could not make", folder))?;
        let name = file
            .file_name()
            .map_or_else(|| "export".into(), |name| name.to_string_lossy());
        let partial = folder.join(format!(".{name}.{}.partial", std::process::id()));
        let endpoint = format!("http://{}", running.addr);
        let done = crate::cli::probes::within(
            command(&self.surreal)
                .args([
                    "export",
                    "--endpoint",
                    &endpoint,
                    "--namespace",
                    NAMESPACE,
                    "--database",
                    database,
                ])
                .arg(&partial),
            limit,
        )
        .map_err(|why| {
            let _ = fs::remove_file(&partial);
            ServerError::Io(format!(
                "the export of {NAMESPACE}/{database} at {} with {} did not start or end within {} \
                 s ({why}): nothing was written",
                running.addr,
                self.surreal.display(),
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
