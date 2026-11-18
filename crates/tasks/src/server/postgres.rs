//! The board's PostgreSQL, beside its SurrealDB ([`Postgres`]): the machine's own, installed by its
//! package manager, from the folder of programs the project file names (`[board] postgresql`,
//! [`programs`]; [its card](../../../../docs/DECISIONS.md#postgresql-binary-source)). It serves
//! only a board kept in a scratch place, whose server ends with the process that started it;
//! [`Home::board_server`] selects it by the home's durable engine identity. No live home starts it.
//!
//! Its data is `postgresql/` in the board's folder, made by `initdb` at the first start in a folder
//! beside it and renamed into place, so an `initdb` cut short never leaves a half-made one where a
//! start looks: UTF-8, PostgreSQL's own `C.UTF-8` locale (no ICU and no library's collation, the
//! same on every machine), the superuser [`USER`], trusted on the socket alone. A later start finds
//! it and makes nothing. Its settings are this module's ([`settings`]), written over the data's
//! `postgresql.conf` at each start, so a server started by hand on it has them too.
//!
//! It listens on a Unix socket alone, never on TCP, in a folder of its own under `/tmp`
//! ([`socket_folder`]), private to this user: macOS refuses a socket path over 103 bytes, which a
//! board's folder under a checkout passes. Its server runs under a supervisor of its own session
//! ([`SUPERVISED`]), which writes the server's log, says when the server is ready to accept
//! connections, and ends the server once the starter's [`Lifeline`] reads its end: a scratch
//! SurrealDB's rule. A start waits for that word, never polls; a stop is PostgreSQL's fast shutdown,
//! waited for by `pg_ctl`. Both take the board's starters' lock, as SurrealDB's do.

use super::scratch::{Lifeline, scratch};
use super::starters::Doing;
use super::{Home, ServerError, io, last_argument_is};
use crate::sys::{Stdio, command, output, pid_alive, wait_output};
use std::fs;
use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::sync::mpsc;

mod lifecycle;
pub use lifecycle::Holding;

/// The project file's `[board]` key naming the folder of the machine's PostgreSQL programs.
pub const KEY: &str = "postgresql";
/// The oldest major release the board starts: the one it was tried on, which has the builtin
/// `C.UTF-8` locale.
pub const OLDEST_MAJOR: u32 = 18;
/// The superuser `initdb` makes, whom a connection over the socket names.
pub const USER: &str = "postgres";
/// In the board's folder: the server's data and its log.
pub(crate) const DATA: &str = "postgresql";
const LOG: &str = "postgresql.log";
/// Where the socket folders are, each named for the data it serves ([`socket_folder`]).
const SOCKETS: &str = "/tmp";
/// The longest socket path macOS takes (`sun_path` is 104 bytes with its closing zero).
pub const SOCKET_PATH_MAX: usize = 103;
/// The socket's file in its folder, PostgreSQL's name for port 5432.
pub const SOCKET_FILE: &str = ".s.PGSQL.5432";

/// The supervisor of a board's PostgreSQL, run by `sh -c` with the server `"$0"`, its data `"$1"`
/// and its log `"$2"`, the starter's [`Lifeline`] as its input and a pipe of the starter's as its
/// output. That output hears two lines, then its end: the server's pid, then `ready` once the
/// server logs that it is ready to accept connections (in the `C` locale its settings set), or
/// `ended` once it ends without. The server's output goes through a
/// reader that appends it to the log. Its parent waits for it and ends the watchdog once it has
/// ended; the watchdog reads the lifeline, and once that reads its end asks the server to end fast
/// (SIGINT), then smart (SIGTERM), as a job `sh` starts in the background ignores SIGINT until
/// PostgreSQL sets its own handlers, where SIGTERM still ends it, and a fast shutdown under way
/// takes no smart one. Nothing but those two writers holds the starter's pipe, each until its line
/// is said, so the starter reads to its end: no wait by a number.
const SUPERVISED: &str = r#"exec 3<&0 </dev/null 4>&1 >/dev/null 2>&1
{
    "$0" -D "$1" 2>&1 3<&- 4>&- &
    server=$!
    echo "$server" >&4
    exec 4>&-
    ( while IFS= read -r line; do :; done <&3; kill -INT "$server"; kill -TERM "$server" ) &
    watchdog=$!
    exec 3<&-
    wait "$server"
    kill "$watchdog"
} | {
    exec 3<&-
    ready=
    while IFS= read -r line; do
        printf '%s\n' "$line" >>"$2"
        if [ -z "$ready" ]; then
            case $line in
            *"database system is ready to accept connections"*)
                ready=1
                echo ready >&4
                exec 4>&-
                ;;
            esac
        fi
    done
    [ -n "$ready" ] || echo ended >&4
} &"#;

/// A board's PostgreSQL, serving: its postmaster, the folder its socket is in (a connection's
/// `host`), and the release of the programs that serve it.
#[derive(Debug, Clone, PartialEq)]
pub struct Serving {
    pub pid: i64,
    pub socket: PathBuf,
    pub version: String,
}

/// The PostgreSQL of the board in a [`Home`]'s folder, served by the programs in `programs`.
pub struct Postgres {
    home: Home,
    programs: PathBuf,
}

impl Home {
    /// This board's PostgreSQL, served by the programs in the folder `programs` (`[board]
    /// postgresql`, [`programs`]): beside its SurrealDB in its folder, under its starters' lock.
    pub fn postgresql(&self, programs: &Path) -> Postgres {
        Postgres {
            home: self.clone(),
            programs: programs.into(),
        }
    }
}

impl Postgres {
    /// Where its data is: `postgresql/` in the board's folder.
    pub fn data(&self) -> PathBuf {
        self.home.dir.join(DATA)
    }

    /// The folder its socket is in, which a connection names as its `host`.
    pub fn socket(&self) -> PathBuf {
        socket_folder(&self.data())
    }

    /// Its server's log: `postgresql.log` in the board's folder.
    pub fn log(&self) -> PathBuf {
        self.home.dir.join(LOG)
    }

    /// The program `name` of the PostgreSQL that serves it (`psql`, `pg_ctl`).
    pub fn program(&self, name: &str) -> PathBuf {
        self.programs.join(name)
    }

    /// The release of its programs, as `postgres --version` says it (`18.7`): refused when the
    /// programs are not there or older than [`OLDEST_MAJOR`].
    pub fn version(&self) -> Result<String, ServerError> {
        let postgres = self.program("postgres");
        if !postgres.is_file() {
            return Err(ServerError::Io(format!(
                "PostgreSQL is not at {} (`[board] {KEY}` in the project file names the folder of \
                 its programs): install it with the machine's package manager, `brew install \
                 postgresql@{OLDEST_MAJOR}` on a Mac, and name its programs' folder there",
                self.programs.display()
            )));
        }
        let said =
            output(command(&postgres).arg("--version")).map_err(io("could not run", &postgres))?;
        let text = String::from_utf8_lossy(&said.stdout);
        let version = version_of(&text).ok_or_else(|| {
            ServerError::Io(format!(
                "{} --version said {:?}, no release of PostgreSQL",
                postgres.display(),
                text.trim()
            ))
        })?;
        match major(&version) {
            Some(major) if major >= OLDEST_MAJOR => Ok(version),
            _ => Err(ServerError::Io(format!(
                "{} is PostgreSQL {version}; the board runs on {OLDEST_MAJOR} or later",
                postgres.display()
            ))),
        }
    }

    /// Its server, answering: the one running on its data, else one started now. A board outside
    /// a scratch place is refused before anything is made: no live board runs on PostgreSQL yet.
    /// The first start makes the data ([`Postgres::make`]); every start writes its settings and
    /// its socket's folder, starts the server supervised and waits, the board's patience at most,
    /// for its word that it is ready, or that it ended. One that says neither by then is left
    /// running, never signalled, and the caller told so: it is replaying its log, or stuck, and
    /// ends with this process. Under the starters' lock.
    pub fn ensure(&self) -> Result<Serving, ServerError> {
        self.scratch_only()?;
        let _starters = self.home.lock(Doing::Starting)?;
        self.home.allow_postgresql()?;
        self.home.unheld()?;
        let version = self.version()?;
        if let Some(serving) = self.running(&version)? {
            return Ok(serving);
        }
        self.make()?;
        let data = self.data();
        let conf = data.join("postgresql.conf");
        fs::write(&conf, settings(&self.socket())).map_err(io("could not write", &conf))?;
        self.socket_made()?;
        (self.home.tell)(&format!(
            "· the board's PostgreSQL is not running: it starts now, on its data at {}",
            data.display()
        ));
        self.start(version)
    }

    pub(super) fn scratch_only(&self) -> Result<(), ServerError> {
        if !self
            .home
            .supervised
            .unwrap_or_else(|| scratch(&self.home.dir))
        {
            return Err(ServerError::Io(format!(
                "the board at {} is not in a scratch place: PostgreSQL serves only a board under \
                 the temporary folder or a checkout's artifacts/, whose server ends with the \
                 process that started it",
                self.home.dir.display()
            )));
        }
        Ok(())
    }

    /// The data made by `initdb` when it is not there (it holds no `PG_VERSION`), in a folder
    /// beside it, which an earlier `initdb` cut short may have left and which is made anew, then
    /// renamed into place. What `initdb` says goes to the log.
    fn make(&self) -> Result<(), ServerError> {
        let data = self.data();
        if data.join("PG_VERSION").is_file() {
            return Ok(());
        }
        let making = self.home.dir.join(format!("{DATA}.making"));
        if making.exists() {
            fs::remove_dir_all(&making).map_err(io("could not remove", &making))?;
        }
        let initdb = self.program("initdb");
        let said = output(
            command(&initdb)
                .arg("-D")
                .arg(&making)
                .args([
                    "--encoding=UTF8",
                    "--no-locale",
                    "--locale-provider=builtin",
                    "--builtin-locale=C.UTF-8",
                    "--auth-local=trust",
                    "--auth-host=reject",
                    "--no-instructions",
                ])
                .arg(format!("--username={USER}")),
        )
        .map_err(io("could not run", &initdb))?;
        let log = self.log();
        let mut text = said.stdout;
        text.extend_from_slice(&said.stderr);
        let mut kept = fs::read(&log).unwrap_or_default();
        kept.extend_from_slice(&text);
        fs::write(&log, kept).map_err(io("could not write", &log))?;
        if !said.status.success() {
            return Err(ServerError::NotUp(format!(
                "initdb did not make the board's PostgreSQL data at {}: {}; its log is {}",
                making.display(),
                String::from_utf8_lossy(&text).trim(),
                log.display()
            )));
        }
        fs::rename(&making, &data).map_err(io("could not move into place", &data))
    }

    /// The socket's folder, made private to this user where it is not there; refused where it is
    /// there and is not a folder of this board's user alone, which another user could listen in.
    fn socket_made(&self) -> Result<(), ServerError> {
        let folder = self.socket();
        match fs::DirBuilder::new().mode(0o700).create(&folder) {
            Ok(()) => return Ok(()),
            Err(problem) if problem.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(problem) => return Err(io("could not make", &folder)(problem)),
        }
        let there = fs::symlink_metadata(&folder).map_err(io("could not read", &folder))?;
        let ours = fs::metadata(&self.home.dir).map_err(io("could not read", &self.home.dir))?;
        if there.is_dir() && there.uid() == ours.uid() && there.permissions().mode() & 0o077 == 0 {
            Ok(())
        } else {
            Err(ServerError::Io(format!(
                "{} is there and is not a folder this user alone reads: the board's PostgreSQL \
                 keeps its socket nowhere else",
                folder.display()
            )))
        }
    }

    /// The server PostgreSQL's own record names (`postmaster.pid` in the data), when it runs on
    /// this data and is ready; one starting or stopping is not. Refused when the record names a
    /// server of this data that is not ready, so a start never joins one.
    fn running(&self, version: &str) -> Result<Option<Serving>, ServerError> {
        let Some(record) = self.recorded() else {
            return Ok(None);
        };
        if record.ready {
            return Ok(Some(Serving {
                pid: record.pid,
                socket: record.socket,
                version: version.into(),
            }));
        }
        Err(ServerError::NotUp(format!(
            "the board's PostgreSQL (pid {}) runs on {} and is not ready: it is starting or \
             stopping, and is left to; its log is {}",
            record.pid,
            self.data().display(),
            self.log().display()
        )))
    }

    /// What `postmaster.pid` says, when the process it names is a live server of this data: a
    /// record a server that was killed left, or one whose pid went to another process, is none.
    fn recorded(&self) -> Option<PidFile> {
        let record = pid_file(&fs::read_to_string(self.data().join("postmaster.pid")).ok()?)?;
        (pid_alive(record.pid) && self.serves(record.pid)).then_some(record)
    }

    /// Whether `pid` runs this board's `postgres` on its data, as the supervisor starts it.
    fn serves(&self, pid: i64) -> bool {
        let Ok(said) =
            output(command("ps").args(["-ww", "-o", "command=", "-p", &pid.to_string()]))
        else {
            return false;
        };
        let line = String::from_utf8_lossy(&said.stdout);
        let line = line.trim();
        let data = self.data().display().to_string();
        last_argument_is(line, &data)
            && line
                .strip_suffix(&data)
                .and_then(|rest| rest.strip_suffix(" -D "))
                == Some(&self.program("postgres").display().to_string())
    }

    /// The server started under its supervisor ([`SUPERVISED`]) on this process's lifeline, and
    /// waited for until it says it is ready or ended, the board's patience at most.
    fn start(&self, version: String) -> Result<Serving, ServerError> {
        let log = self.log();
        let from = fs::metadata(&log).map_or(0, |meta| meta.len());
        let lifeline = Lifeline::of_this_process()
            .map_err(io("could not make a lifeline for", &self.home.dir))?;
        let input = lifeline
            .reader()
            .map_err(io("could not hand a lifeline to", &self.data()))?;
        let postgres = self.program("postgres");
        let child = command("/bin/sh")
            .new_session()
            .default_signals()
            .arg("-c")
            .arg(SUPERVISED)
            .arg(&postgres)
            .arg(self.data())
            .arg(&log)
            .stdin(Stdio::from(input))
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(io("could not start", &postgres))?;
        let (said, heard) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = said.send(wait_output(child));
        });
        let out = match heard.recv_timeout(self.home.patience) {
            Ok(out) => out.map_err(io("could not read the supervisor of", &postgres))?,
            Err(_) => {
                return Err(ServerError::NotUp(format!(
                    "the board's PostgreSQL has not said it is ready within {} s: it is still \
                     starting (a server replaying a long log takes minutes) or stuck, and is \
                     left running, never signalled; it ends with this process. Its log is {}",
                    self.home.patience.as_secs(),
                    log.display()
                )));
            }
        };
        let said = Said::of(&String::from_utf8_lossy(&out.stdout));
        match (said.pid, said.ready) {
            (Some(pid), Some(true)) => Ok(Serving {
                pid,
                socket: self.socket(),
                version,
            }),
            (pid, _) => Err(ServerError::NotUp(format!(
                "the board's PostgreSQL{} did not start: {}; its log is {}",
                pid.map(|pid| format!(" (pid {pid})")).unwrap_or_default(),
                super::since(&log, from).trim(),
                log.display()
            ))),
        }
    }

    /// Its server stopped, when it runs: PostgreSQL's fast shutdown (its clients cut off, what
    /// they had not committed rolled back, the rest written), waited for by `pg_ctl` the board's
    /// patience at most, then its socket's folder removed. What it was, if anything. One still
    /// stopping by then is left to finish, the caller told so, and never sent a second signal.
    /// Under the starters' lock.
    pub fn stop(&self) -> Result<Option<Serving>, ServerError> {
        self.scratch_only()?;
        let _starters = self.home.lock(Doing::Stopping)?;
        self.home.allow_postgresql()?;
        self.home.unheld()?;
        self.stop_held()
    }

    /// The selected scratch server already running, without starting it.
    pub fn current(&self) -> Result<Option<Serving>, ServerError> {
        self.scratch_only()?;
        self.home.allow_postgresql()?;
        self.running(&self.version()?)
    }

    /// Stop under the caller's starters' lock, also while that caller holds the board stopped.
    fn stop_held(&self) -> Result<Option<Serving>, ServerError> {
        let Some(record) = self.recorded() else {
            self.socket_removed();
            return Ok(None);
        };
        let version = self.version().unwrap_or_default();
        let pg_ctl = self.program("pg_ctl");
        let said = output(
            command(&pg_ctl)
                .arg("stop")
                .arg("-D")
                .arg(self.data())
                .args(["-m", "fast", "-w", "-s", "-t"])
                .arg(self.home.patience.as_secs().max(1).to_string()),
        )
        .map_err(io("could not run", &pg_ctl))?;
        if !said.status.success() {
            return Err(ServerError::NotUp(format!(
                "the board's PostgreSQL (pid {}) was asked to stop and has not within {} s: it \
                 is left to finish, never signalled again ({}); its log is {}",
                record.pid,
                self.home.patience.as_secs(),
                String::from_utf8_lossy(&said.stderr).trim(),
                self.log().display()
            )));
        }
        self.socket_removed();
        Ok(Some(Serving {
            pid: record.pid,
            socket: self.socket(),
            version,
        }))
    }

    /// The socket's folder removed once it holds nothing: a server that stopped cleanly took its
    /// socket and lock with it; one that was killed left them, for its next start to take over.
    fn socket_removed(&self) {
        let _ = fs::remove_dir(self.socket());
    }
}

/// The folder the socket of the server on `data` is in: `/tmp/symphony-pg-` and the first 16 hex
/// digits of the SHA-256 of the data's path, so a socket path is 47 bytes for any board, under
/// macOS's [`SOCKET_PATH_MAX`], and two boards never share one.
pub fn socket_folder(data: &Path) -> PathBuf {
    use sha2::{Digest as _, Sha256};
    let digest = Sha256::digest(data.as_os_str().as_encoded_bytes());
    let hex: String = digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Path::new(SOCKETS).join(format!("symphony-pg-{hex}"))
}

/// The folder of the machine's PostgreSQL programs the project file at the main checkout `root`
/// names (`[board] postgresql`): none when it names none, or there is no file. Refused, naming the
/// file, when the value is not an absolute path or the file does not parse. Only that key is read;
/// the rest of the file is the Symphony's.
pub fn programs(root: &Path) -> Result<Option<PathBuf>, String> {
    let path = root.join(crate::project::FILE);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("cannot read {}: {error}", path.display())),
    };
    let table: toml::Table = text
        .parse()
        .map_err(|error| format!("{}: {error}", path.display()))?;
    match table.get("board").and_then(|board| board.get(KEY)) {
        None => Ok(None),
        Some(toml::Value::String(folder)) if Path::new(folder).is_absolute() => {
            Ok(Some(folder.into()))
        }
        Some(other) => Err(format!(
            "{}: [board] {KEY} is {}, not the absolute folder of the machine's PostgreSQL \
             programs",
            path.display(),
            match other {
                toml::Value::String(text) => format!("{text:?}"),
                toml::Value::Integer(number) => number.to_string(),
                other => format!("a {}", other.type_str()),
            }
        )),
    }
}

/// The settings of a board's PostgreSQL whose socket is in `socket`, as its `postgresql.conf`. No
/// TCP, a socket this user alone reaches, messages in English (the start waits for one), times
/// in UTC; and sized for 512 MiB: 128 MiB of shared buffers, 4 MiB for a sort or a hash in each
/// of 64 connections and 32 MiB for each of two autovacuum workers come to 448 MiB, which leaves
/// out each backend's own memory, no parallel query (each worker a process of its own). What a
/// loaded board takes is measured by the slice that loads one.
pub fn settings(socket: &Path) -> String {
    format!(
        "# The board's PostgreSQL (crates/tasks/src/server/postgres.rs), written at each start.\n\
         listen_addresses = ''\n\
         unix_socket_directories = '{socket}'\n\
         unix_socket_permissions = 0700\n\
         max_connections = 64\n\
         shared_buffers = 128MB\n\
         work_mem = 4MB\n\
         maintenance_work_mem = 32MB\n\
         autovacuum_max_workers = 2\n\
         max_parallel_workers_per_gather = 0\n\
         max_parallel_maintenance_workers = 0\n\
         lc_messages = 'C'\n\
         timezone = 'UTC'\n\
         log_timezone = 'UTC'\n\
         log_line_prefix = '%m [%p] '\n\
         datestyle = 'iso, mdy'\n\
         default_text_search_config = 'pg_catalog.english'\n",
        socket = socket.display()
    )
}

/// The release `postgres --version` says (`postgres (PostgreSQL) 18.7 (Homebrew)`: `18.7`).
fn version_of(said: &str) -> Option<String> {
    let after = said.split_once("(PostgreSQL) ")?.1;
    let version = after.split_whitespace().next()?;
    major(version).map(|_| version.to_string())
}

/// The major release of `version` (`18.7`: 18; `19beta2`: 19).
fn major(version: &str) -> Option<u32> {
    let digits: String = version.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// What `postmaster.pid` says of its server: its pid (the first line), the folder of its socket
/// (the fifth), and whether it is ready to accept connections (the eighth, `ready` padded).
#[derive(Debug, PartialEq)]
struct PidFile {
    pid: i64,
    socket: PathBuf,
    ready: bool,
}

fn pid_file(text: &str) -> Option<PidFile> {
    let lines: Vec<&str> = text.lines().collect();
    Some(PidFile {
        pid: lines.first()?.trim().parse().ok()?,
        socket: PathBuf::from(lines.get(4)?.trim()),
        ready: lines.get(7).is_some_and(|status| status.trim() == "ready"),
    })
}

/// What the supervisor said ([`SUPERVISED`]): the server's pid, and whether it was ready (`ready`)
/// or ended first (`ended`); none of either where it did not say.
#[derive(Debug, PartialEq)]
struct Said {
    pid: Option<i64>,
    ready: Option<bool>,
}

impl Said {
    fn of(out: &str) -> Said {
        let mut said = Said {
            pid: None,
            ready: None,
        };
        for line in out.lines().map(str::trim) {
            match line {
                "ready" => said.ready = Some(true),
                "ended" => said.ready = Some(false),
                pid => said.pid = said.pid.or_else(|| pid.parse().ok()),
            }
        }
        said
    }
}

#[cfg(test)]
mod tests;
