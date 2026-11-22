//! The board's PostgreSQL, beside its SurrealDB ([`Postgres`]): the machine's own, installed by its
//! package manager, from the folder of programs the project file names (`[board] postgresql`)
//! or found on `PATH` ([`discover_programs`];
//! [its card](../../../../docs/analysis/postgresql-binary-source.md#postgresql-binary-source)).
//! It serves the board whose
//! home's durable engine identity names it ([`Home::board_server`]): a scratch home's by
//! [`Home::initialize_backend`], any other's by the move alone ([`super::moving`]).
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
//! ([`SUPERVISED`]), which writes the server's log and says when the server is ready to accept
//! connections. A board's in a scratch place ends once the starter's [`Lifeline`] reads its end,
//! a scratch SurrealDB's rule; any other board's outlives its starter, as its SurrealDB does, and
//! ends at a stop or a hold. A start waits for the supervisor's word, never polls; a stop is
//! PostgreSQL's fast shutdown, waited for by `pg_ctl`, and a stop that meets a server ending on
//! its own, gone by the time `pg_ctl` reads its record, waits for its end
//! ([`Postgres::stop_failed`]). Both take the board's starters' lock, as
//! SurrealDB's do. A start that finds a server of the data on its way out, as a scratch board's
//! is in the moment after the command that started it ended, waits for that server's end
//! ([`Postgres::outgoing_ended`]) and then starts its own: one whose record says it is stopping,
//! and one whose record still says it is ready though its starter has ended ([`STARTER`]), as it
//! does until its watchdog's signal reaches it.

use super::scratch::{Lifeline, scratch};
use super::starters::Doing;
use super::{Home, ServerError, io, last_argument_is};
use crate::sys::{Stdio, command, gone, load_said, output, pid_alive, wait_output};
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
/// In the board's folder: the server a start started on its [`Lifeline`] and the process it ends
/// with, as `SERVER STARTER` pids on one line ([`starter_of`]). Written once that server is ready,
/// under the starters' lock, and removed by every start before it starts one, so it names the last
/// server the board started on a lifeline. It counts only for a running server of the pid it
/// names: one of another pid, as a server started by hand on the data, is never taken for it.
const STARTER: &str = "postgresql.starter";
/// Where the socket folders are, each named for the data it serves ([`socket_folder`]).
const SOCKETS: &str = "/tmp";
/// The longest socket path macOS takes (`sun_path` is 104 bytes with its closing zero).
pub const SOCKET_PATH_MAX: usize = 103;
/// The socket's file in its folder, PostgreSQL's name for port 5432.
pub const SOCKET_FILE: &str = ".s.PGSQL.5432";

/// The supervisor of a board's PostgreSQL, run by `sh -c` with the server `"$0"`, its data `"$1"`,
/// its log `"$2"` and whose it is `"$3"` ([`OWNED`], or [`APART`] with no watchdog and nothing as
/// its input), the starter's [`Lifeline`] as its input and a pipe of the starter's as its
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
    watchdog=
    if [ "$3" = owned ]; then
        ( while IFS= read -r line; do :; done <&3; kill -INT "$server"; kill -TERM "$server" ) &
        watchdog=$!
    fi
    exec 3<&-
    wait "$server"
    [ -z "$watchdog" ] || kill "$watchdog"
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

/// [`SUPERVISED`]'s word for a server that ends with the process that started it.
const OWNED: &str = "owned";
/// [`SUPERVISED`]'s word for a server that outlives its starter.
const APART: &str = "apart";

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
        version_at(&self.programs).map_err(ServerError::Io)
    }

    /// Its server, answering: the one running on its data, else one started now. The first start
    /// makes the data ([`Postgres::make`]); every start writes its settings and its socket's
    /// folder, starts the server supervised and waits, the board's patience at most, for its word
    /// that it is ready, or that it ended. One that says neither by then is left running, never
    /// signalled, and the caller told so: it is replaying its log, or stuck. A scratch board's
    /// ends with this process ([`Postgres::owned`]); any other's outlives it. Refused while the
    /// board is held. Under the starters' lock.
    pub fn ensure(&self) -> Result<Serving, ServerError> {
        let _starters = self.home.lock(Doing::Starting)?;
        self.home.allow_postgresql()?;
        self.home.unheld()?;
        self.serve()
    }

    /// [`Postgres::ensure`] under the caller's starters' lock, also while that caller holds the
    /// board stopped: the move's own server on the held data ([`super::moving`]).
    pub(super) fn serve(&self) -> Result<Serving, ServerError> {
        let version = self.version()?;
        // The record is read once for a server that serves, as every command of a board whose
        // server outlives its starter finds it, and again only after a wait.
        let mut record = self.recorded();
        if let Some((outgoing, going)) = record.as_ref().and_then(|record| {
            let going = if record.stopping {
                Some("is stopping".to_string())
            } else {
                self.starter_ended(record).map(|starter| {
                    format!("ends with its starter (pid {starter}), which has ended")
                })
            };
            going.map(|going| (record, going))
        }) {
            self.outgoing_ended(outgoing, &going)?;
            record = self.recorded();
        }
        if let Some(serving) = self.serving(record, &version)? {
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

    /// Whether its server ends with the process that starts it: a board in a scratch place
    /// ([`scratch`]), or one a caller says so of ([`Home::supervised`]).
    fn owned(&self) -> bool {
        self.home
            .supervised
            .unwrap_or_else(|| scratch(&self.home.dir))
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
        self.serving(self.recorded(), version)
    }

    /// [`Postgres::running`], of the record `record` read of it.
    fn serving(
        &self,
        record: Option<PidFile>,
        version: &str,
    ) -> Result<Option<Serving>, ServerError> {
        let Some(record) = record else {
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
            "the board's PostgreSQL (pid {}) runs on {} and is not ready: it is {}, and is left \
             to, never signalled. Run the command again once it is ready or has ended; its log \
             is {}",
            record.pid,
            self.data().display(),
            if record.stopping {
                "stopping"
            } else {
                "starting"
            },
            self.log().display()
        )))
    }

    /// The server `record` names, which is on its way out (`going` says how: it is stopping, or
    /// its starter has ended), waited for until it has ended: a scratch board's is asked to end as
    /// the command that started it ends ([`SUPERVISED`]'s watchdog), and the next command meets it
    /// on its way out, its record saying it is ready until that signal reaches it. The wait is for
    /// the end of its process, as the kernel says it ([`gone`]), the board's patience its
    /// backstop, which only a stall reaches and which says the load; then the start goes on, to a
    /// server of its own. It is never signalled, and the wait is told before it is made. One that
    /// is starting is not waited for: [`Postgres::running`] refuses it.
    fn outgoing_ended(&self, record: &PidFile, going: &str) -> Result<(), ServerError> {
        let left = |why: String| {
            ServerError::NotUp(format!(
                "the board's PostgreSQL (pid {}) on {} {going}, {why}: it is left to finish, \
                 never signalled. Run the command again once it has ended; its log is {}",
                record.pid,
                self.data().display(),
                self.log().display()
            ))
        };
        (self.home.tell)(&format!(
            "· the board's PostgreSQL (pid {}) {going}: this start waits for its end, then \
             starts its own",
            record.pid
        ));
        match gone(record.pid, self.home.patience) {
            Ok(true) => Ok(()),
            Ok(false) => Err(left(format!(
                "and has not ended in the {} s this start waited for it ({})",
                self.home.patience.as_secs(),
                load_said()
            ))),
            Err(why) => Err(left(format!("and its end could not be waited for ({why})"))),
        }
    }

    /// What `postmaster.pid` says, when the process it names is a live server of this data: a
    /// record a server that was killed left, or one whose pid went to another process, is none.
    fn recorded(&self) -> Option<PidFile> {
        let record = pid_file(&fs::read_to_string(self.data().join("postmaster.pid")).ok()?)?;
        (pid_alive(record.pid) && self.serves(record.pid)).then_some(record)
    }

    /// The starter of the server `record` names, when this board started that server on the
    /// starter's [`Lifeline`] ([`STARTER`]) and the starter has ended. Such a server is on its way
    /// out: nothing but its starter holds the lifeline open, so its watchdog asks it to end.
    /// None for a server whose starter still runs, one started apart or by hand, or where the
    /// record cannot be read. A starter's pid taken since by another process reads as running,
    /// so a server is never taken for outgoing while its starter runs.
    fn starter_ended(&self, record: &PidFile) -> Option<i64> {
        let text = fs::read_to_string(self.home.dir.join(STARTER)).ok()?;
        let (server, starter) = starter_of(&text)?;
        (server == record.pid && !pid_alive(starter)).then_some(starter)
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

    /// The server started under its supervisor ([`SUPERVISED`]), on this process's lifeline when
    /// it is this process's own ([`Postgres::owned`]), and waited for until it says it is ready or
    /// ended, the board's patience at most. One of this process's own, once ready, is recorded with
    /// this process as its starter ([`STARTER`]); the record of any earlier one goes first.
    fn start(&self, version: String) -> Result<Serving, ServerError> {
        let log = self.log();
        let from = fs::metadata(&log).map_or(0, |meta| meta.len());
        let owned = self.owned();
        let starter = self.home.dir.join(STARTER);
        match fs::remove_file(&starter) {
            Err(problem) if problem.kind() != std::io::ErrorKind::NotFound => {
                return Err(io("could not remove", &starter)(problem));
            }
            _ => {}
        }
        let input = if owned {
            let lifeline = Lifeline::of_this_process()
                .map_err(io("could not make a lifeline for", &self.home.dir))?;
            Stdio::from(
                lifeline
                    .reader()
                    .map_err(io("could not hand a lifeline to", &self.data()))?,
            )
        } else {
            Stdio::null()
        };
        let postgres = self.program("postgres");
        let child = command("/bin/sh")
            .new_session()
            .default_signals()
            .arg("-c")
            .arg(SUPERVISED)
            .arg(&postgres)
            .arg(self.data())
            .arg(&log)
            .arg(if owned { OWNED } else { APART })
            .stdin(input)
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
                     left running, never signalled{}. Its log is {}",
                    self.home.patience.as_secs(),
                    if owned {
                        "; it ends with this process"
                    } else {
                        ""
                    },
                    log.display()
                )));
            }
        };
        let said = Said::of(&String::from_utf8_lossy(&out.stdout));
        match (said.pid, said.ready) {
            (Some(pid), Some(true)) => {
                if owned {
                    fs::write(&starter, format!("{pid} {}\n", std::process::id()))
                        .map_err(io("could not write", &starter))?;
                }
                Ok(Serving {
                    pid,
                    socket: self.socket(),
                    version,
                })
            }
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
    /// One that ended on its own before pg_ctl read its record is waited for to its end, and is
    /// stopped ([`Postgres::stop_failed`]). Under the starters' lock.
    pub fn stop(&self) -> Result<Option<Serving>, ServerError> {
        let _starters = self.home.lock(Doing::Stopping)?;
        self.home.allow_postgresql()?;
        self.home.unheld()?;
        self.stop_held()
    }

    /// The selected server already running, without starting it.
    pub fn current(&self) -> Result<Option<Serving>, ServerError> {
        self.home.allow_postgresql()?;
        self.running(&self.version()?)
    }

    /// Stop under the caller's starters' lock, also while that caller holds the board stopped.
    pub(super) fn stop_held(&self) -> Result<Option<Serving>, ServerError> {
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
            self.stop_failed(&record, String::from_utf8_lossy(&said.stderr).trim())?;
        }
        self.socket_removed();
        Ok(Some(Serving {
            pid: record.pid,
            socket: self.socket(),
            version,
        }))
    }

    /// What a stop whose `pg_ctl` failed comes to, `said` what pg_ctl said, judged by the server
    /// `record` named when the stop read it. One whose record is gone ended on its own since: a
    /// scratch board's does as the command that started it ends ([`SUPERVISED`]'s watchdog), and
    /// pg_ctl, reading the record after the stop did, finds none to signal. Its end is waited for
    /// as the kernel says it ([`gone`]), the board's patience its backstop, which only a stall
    /// reaches and which says the load; then it is stopped. One whose record still names a live
    /// server of the data still runs: pg_ctl's wait met its bound, or it could not signal it.
    /// Neither is signalled again.
    fn stop_failed(&self, record: &PidFile, said: &str) -> Result<(), ServerError> {
        let left = |why: String| {
            ServerError::NotUp(format!(
                "the board's PostgreSQL (pid {}) {why}: it is left to finish, never signalled \
                 again; its log is {}",
                record.pid,
                self.log().display()
            ))
        };
        if self.recorded().is_some() {
            return Err(left(format!(
                "was asked to stop, and pg_ctl, which waits {} s at most for its end, says it has \
                 not ended ({said}; {})",
                self.home.patience.as_secs().max(1),
                load_said()
            )));
        }
        let took = format!("took its record with it before pg_ctl read it ({said})");
        match gone(record.pid, self.home.patience) {
            Ok(true) => Ok(()),
            Ok(false) => Err(left(format!(
                "{took}, and has not ended in the {} s this stop waited for it ({})",
                self.home.patience.as_secs(),
                load_said()
            ))),
            Err(why) => Err(left(format!(
                "{took}, and its end could not be waited for ({why})"
            ))),
        }
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
    use std::fmt::Write as _;
    const PREFIX: &str = "symphony-pg-";
    let digest = Sha256::digest(data.as_os_str().as_encoded_bytes());
    let mut name = String::with_capacity(PREFIX.len() + 16);
    name.push_str(PREFIX);
    for byte in &digest[..8] {
        write!(name, "{byte:02x}").expect("writing to a String cannot fail");
    }
    Path::new(SOCKETS).join(name)
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

/// The variable that names the folder of PostgreSQL 16's programs for the hosted board's check.
pub const HOSTED_16: &str = "SYMPHONY_POSTGRESQL_16";

/// Where the hosted board's check finds PostgreSQL 16's programs
/// (docs/POSTGRESQL.md, "A hosted board"): the folder [`HOSTED_16`] names, otherwise Homebrew's
/// keg, which changes no `postgres` on `PATH`. The one rule: the check's servers and the release
/// run's record of what it ran on both ask here. It inspects nothing and starts nothing.
pub fn hosted_programs_16() -> PathBuf {
    hosted_16_from(std::env::var_os(HOSTED_16))
}

fn hosted_16_from(named: Option<std::ffi::OsString>) -> PathBuf {
    named.filter(|folder| !folder.is_empty()).map_or_else(
        || "/opt/homebrew/opt/postgresql@16/bin".into(),
        PathBuf::from,
    )
}

/// The machine's PostgreSQL installation: the project file's explicit [`programs`] folder first,
/// otherwise the first executable `postgres` on `PATH`. All tools come from that one folder;
/// an invalid override or incompatible first installation is refused, never skipped. The server
/// must be at least [`OLDEST_MAJOR`], and `initdb`, `pg_ctl` and `psql` must be executable there.
/// This only inspects programs; it starts no database and writes no state.
pub fn discover_programs(root: &Path) -> Result<PathBuf, String> {
    discover_with_path(root, std::env::var_os("PATH").as_deref())
}

fn discover_with_path(root: &Path, path: Option<&std::ffi::OsStr>) -> Result<PathBuf, String> {
    let folder = match programs(root)? {
        Some(folder) => folder,
        None => {
            let folder = path
                .into_iter()
                .flat_map(std::env::split_paths)
                .find(|folder| executable(&folder.join("postgres")))
                .ok_or_else(|| {
                    format!(
                        "{}: no executable postgres on PATH; {}",
                        root.join(crate::project::FILE).display(),
                        setup_help()
                    )
                })?;
            std::path::absolute(&folder)
                .map_err(|error| format!("cannot resolve {}: {error}", folder.display()))?
        }
    };
    version_at(&folder)?;
    for tool in ["initdb", "pg_ctl", "psql"] {
        if !executable(&folder.join(tool)) {
            return Err(format!(
                "{} is missing or not executable; all PostgreSQL tools must be in {}; {}",
                folder.join(tool).display(),
                folder.display(),
                setup_help()
            ));
        }
    }
    Ok(folder)
}

fn executable(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|file| file.is_file() && file.permissions().mode() & 0o111 != 0)
}

fn setup_help() -> String {
    format!(
        "install PostgreSQL {OLDEST_MAJOR} or later with the machine's package manager \
         (`brew install postgresql@{OLDEST_MAJOR}` on a Mac); put its bin folder on PATH \
         or set [board] {KEY} in symphony.toml to that absolute folder"
    )
}

fn version_at(programs: &Path) -> Result<String, String> {
    let postgres = programs.join("postgres");
    if !executable(&postgres) {
        return Err(format!(
            "PostgreSQL is not at {} (postgres is missing or not executable); {}",
            programs.display(),
            setup_help()
        ));
    }
    let said = output(command(&postgres).arg("--version")).map_err(|error| {
        format!(
            "could not run {}: {error}; {}",
            postgres.display(),
            setup_help()
        )
    })?;
    if !said.status.success() {
        return Err(format!(
            "{} --version failed ({}): {}; {}",
            postgres.display(),
            said.status,
            String::from_utf8_lossy(&said.stderr).trim(),
            setup_help()
        ));
    }
    let text = String::from_utf8_lossy(&said.stdout);
    let version = version_of(&text).ok_or_else(|| {
        format!(
            "{} --version said {:?}, no release of PostgreSQL; {}",
            postgres.display(),
            text.trim(),
            setup_help()
        )
    })?;
    match major(&version) {
        Some(major) if major >= OLDEST_MAJOR => Ok(version),
        _ => Err(format!(
            "{} is PostgreSQL {version}; the board runs on {OLDEST_MAJOR} or later; {}",
            postgres.display(),
            setup_help()
        )),
    }
}

/// The settings of a board's PostgreSQL whose socket is in `socket`, as its `postgresql.conf`. No
/// TCP, a socket this user alone reaches, messages in English (the start waits for one), times
/// in UTC; and sized for 512 MiB: 128 MiB of shared buffers, 4 MiB for a sort or a hash in each
/// of 64 connections and 32 MiB for each of two autovacuum workers come to 448 MiB, which leaves
/// out each backend's own memory, no parallel query (each worker a process of its own). A
/// backend's own memory is what its largest statement took, kept until its connection ends,
/// which is why the store's long reads answer an item a row (`store/postgres/mod.rs`'s
/// `Pg::Listed`); what a loaded board takes is `examples/board_load.rs`'s to measure.
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
/// (the fifth), and whether it is ready to accept connections or on its way out (the eighth,
/// `ready` or `stopping`, padded).
#[derive(Debug, PartialEq)]
struct PidFile {
    pid: i64,
    socket: PathBuf,
    ready: bool,
    stopping: bool,
}

fn pid_file(text: &str) -> Option<PidFile> {
    let lines: Vec<&str> = text.lines().collect();
    let status = lines.get(7).map(|status| status.trim());
    Some(PidFile {
        pid: lines.first()?.trim().parse().ok()?,
        socket: PathBuf::from(lines.get(4)?.trim()),
        ready: status == Some("ready"),
        stopping: status == Some("stopping"),
    })
}

/// The server and its starter a [`STARTER`] record names, as `SERVER STARTER` pids on one line;
/// none for a record of any other shape, as a write cut short leaves.
fn starter_of(text: &str) -> Option<(i64, i64)> {
    let mut pids = text.trim_end_matches('\n').split(' ').map(str::parse);
    match (pids.next(), pids.next(), pids.next()) {
        (Some(Ok(server)), Some(Ok(starter)), None) if server > 0 && starter > 0 => {
            Some((server, starter))
        }
        _ => None,
    }
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
