//! The board's PostgreSQL on a scratch board: `Home` makes its data, starts it on a Unix socket
//! alone and stops it, leaving no process; a second start takes the data the first made; a board
//! under a folder too long for a socket path starts; and a server ends with the process that
//! started it. Each test's board is a folder of its own under the temporary folder, its server
//! the machine's PostgreSQL selected by the project file or discovered on PATH.

use std::path::{Path, PathBuf};
use std::time::Duration;
use symphony_board::server::postgres::{self, SOCKET_FILE, SOCKET_PATH_MAX, USER};
use symphony_board::server::{Home, Postgres, Serving};
use symphony_board::sys::{self, command, pid_alive};

/// How long a server's processes are given to be gone once it has stopped: they end at once, its
/// supervisor's shells behind it.
const GONE: Duration = test_time::backstop(Duration::from_secs(10));

/// The folder of the machine's PostgreSQL programs, configured or discovered on PATH.
fn programs() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    postgres::discover_programs(&root).unwrap()
}

/// The folder of the board of the test named `name`, under the temporary folder.
fn folder(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("pg-{}-{name}", std::process::id()))
}

/// A board of a test's own under the temporary folder, its PostgreSQL stopped and its folder
/// removed when it goes, however the test ends.
struct Board {
    dir: PathBuf,
    pg: Postgres,
}

impl Board {
    fn new(name: &str) -> Board {
        Board::at(&folder(name))
    }

    fn at(dir: &Path) -> Board {
        Board::served(dir, |_| programs())
    }

    /// A board of a test's own whose `pg_ctl` is a script of `lines` ([`but_pg_ctl`]).
    fn with_pg_ctl(name: &str, lines: &str) -> Board {
        Board::served(&folder(name), |dir| but_pg_ctl(dir, lines))
    }

    /// A board made anew at `dir`, its PostgreSQL served by the programs in the folder
    /// `programs` makes or names in it.
    fn served(dir: &Path, programs: impl FnOnce(&Path) -> PathBuf) -> Board {
        if dir.exists() {
            std::fs::remove_dir_all(dir).unwrap();
        }
        std::fs::create_dir_all(dir).unwrap();
        let pg = Home::new(&dir.join("board"), Path::new("/no/surreal"))
            .telling(|_| {})
            .postgresql(&programs(dir));
        Board {
            dir: dir.into(),
            pg,
        }
    }

    /// The board's home, as its PostgreSQL was made on it.
    fn home(&self) -> Home {
        Home::new(&self.dir.join("board"), Path::new("/no/surreal"))
    }

    /// What `sql` answers over the server's socket, through one connection of `psql`'s.
    fn psql(&self, serving: &Serving, sql: &str) -> String {
        let said = sys::output(
            command(self.pg.program("psql"))
                .args(["-X", "-A", "-t", "-q", "-v", "ON_ERROR_STOP=1", "-h"])
                .arg(&serving.socket)
                .args(["-U", USER, "-d", "postgres", "-c", sql]),
        )
        .unwrap();
        assert!(
            said.status.success(),
            "{sql}: {}",
            String::from_utf8_lossy(&said.stderr)
        );
        String::from_utf8_lossy(&said.stdout).trim().to_string()
    }

    /// The processes whose command line names this board's data: its server's, and its
    /// supervisor's shells.
    fn processes(&self) -> Vec<String> {
        let data = self.pg.data().display().to_string();
        let said = sys::output(command("ps").args(["-axww", "-o", "pid=,command="])).unwrap();
        String::from_utf8_lossy(&said.stdout)
            .lines()
            .filter(|line| line.contains(&data))
            .map(str::to_string)
            .collect()
    }

    /// Wait until no process names this board's data.
    fn none_left(&self) {
        test_time::until_true("the board's PostgreSQL processes gone", GONE, || {
            self.processes().is_empty()
        })
        .unwrap_or_else(|stall| panic!("{stall}: {:?}", self.processes()));
    }
}

impl Drop for Board {
    fn drop(&mut self) {
        if let Some(hold) = self.pg.held() {
            let _ = self.pg.release(&hold.by, None);
        }
        let _ = self.pg.stop();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn holds_snapshots_and_recovery_keep_the_stopped_cluster_and_the_holder_contract() {
    let board = Board::new("lifecycle");
    let serving = board.pg.ensure().unwrap();
    board.psql(
        &serving,
        "CREATE TABLE kept (n int); INSERT INTO kept VALUES (1)",
    );
    assert!(
        board.pg.snapshot(1.0).is_err(),
        "never copy a running cluster"
    );
    let held = board.pg.hold("amber-heron", "snapshot").unwrap();
    assert_eq!(held.stopped.unwrap().pid, serving.pid);
    board.none_left();
    assert!(board.pg.ensure().is_err());
    assert!(board.pg.stop().is_err());
    assert!(board.pg.reclaim("postgres").is_err());
    assert!(board.pg.hold("other", "take over").is_err());
    assert!(board.pg.release("other", None).is_err());
    assert_eq!(board.pg.hold("amber-heron", "retry").unwrap().stopped, None);

    let snapshot = board.pg.snapshot(1.0).unwrap();
    let partial = snapshot.parent().unwrap().join(".taking");
    std::fs::create_dir(&partial).unwrap();
    std::fs::write(partial.join("PG_VERSION"), "partial").unwrap();
    assert_eq!(
        board.pg.snapshots().unwrap().as_slice(),
        std::slice::from_ref(&snapshot)
    );
    let second = board.pg.snapshot(1.0).unwrap();
    assert_ne!(snapshot, second, "two in one second keep both");
    assert!(!partial.exists());
    assert_eq!(board.pg.snapshots().unwrap().len(), 2);
    board.pg.release("other", Some("amber-heron")).unwrap();
    assert_eq!(board.pg.held(), None);
    let resumed = board.pg.ensure().unwrap();
    board.psql(&resumed, "INSERT INTO kept VALUES (2)");

    let recovered = Board::new("recovered");
    recovered.pg.restore(&snapshot).unwrap();
    assert!(
        recovered.pg.restore(&snapshot).is_err(),
        "never replace existing data"
    );
    let restored = recovered.pg.ensure().unwrap();
    assert_eq!(recovered.psql(&restored, "SELECT sum(n) FROM kept"), "1");
    assert_eq!(board.psql(&resumed, "SELECT sum(n) FROM kept"), "3");
    assert_eq!(
        std::fs::read_to_string(snapshot.join("PG_VERSION"))
            .unwrap()
            .trim(),
        serving.version.split('.').next().unwrap()
    );
}

#[test]
fn reclaim_returns_dead_space_without_changing_the_database_rows() {
    let board = Board::new("vacuum");
    assert!(board.pg.reclaim("postgres").is_err());
    let serving = board.pg.ensure().unwrap();
    board.psql(&serving, "CREATE TABLE bloat AS SELECT g, md5(g::text) AS value FROM generate_series(1, 10000) g; DELETE FROM bloat WHERE g > 10");
    let size = || {
        board
            .psql(&serving, "SELECT pg_relation_size('bloat')")
            .parse::<u64>()
            .unwrap()
    };
    let before = size();
    board.pg.reclaim("postgres").unwrap();
    assert!(size() < before, "VACUUM FULL returned the dead pages");
    assert_eq!(
        board.psql(&serving, "SELECT count(*), sum(g) FROM bloat"),
        "10|55"
    );
}

#[test]
fn snapshots_refuse_unmade_clusters_and_external_tablespaces_and_recovery_checks_the_major() {
    let board = Board::new("snapshot-refusals");
    assert!(board.pg.snapshots().unwrap().is_empty());
    assert!(board.pg.snapshot(1.0).is_err());
    board.pg.ensure().unwrap();
    board.pg.stop().unwrap();
    let external = board.pg.data().join("pg_tblspc").join("outside");
    std::fs::write(&external, "not part of this cluster").unwrap();
    assert!(
        board
            .pg
            .snapshot(1.0)
            .unwrap_err()
            .to_string()
            .contains("tablespaces")
    );
    std::fs::remove_file(external).unwrap();
    let snapshot = board.pg.snapshot(1.0).unwrap();
    std::fs::write(snapshot.join("PG_VERSION"), "17").unwrap();
    let target = Board::new("wrong-major");
    assert!(
        target
            .pg
            .restore(&snapshot)
            .unwrap_err()
            .to_string()
            .contains("same major")
    );
    assert!(!target.pg.data().exists());
}

#[test]
fn a_scratch_boards_postgresql_starts_serves_one_connection_on_its_socket_and_stops_leaving_nothing()
 {
    let board = Board::new("serves");
    let serving = board.pg.ensure().unwrap();
    assert!(pid_alive(serving.pid), "{serving:?}");
    let major: u32 = serving.version.split('.').next().unwrap().parse().unwrap();
    assert!(major >= postgres::OLDEST_MAJOR, "{serving:?}");
    assert_eq!(serving.socket, board.pg.socket());
    assert!(serving.socket.join(SOCKET_FILE).exists(), "{serving:?}");
    let answered = board.psql(
        &serving,
        "select current_setting('listen_addresses') = '', \
         current_setting('max_connections'), current_setting('shared_buffers'), \
         (select datlocale from pg_database where datname = 'postgres'), upper('ünïcödé')",
    );
    assert_eq!(answered, "t|64|128MB|C.UTF-8|ÜNÏCÖDÉ");
    // Found again, not started again.
    assert_eq!(board.pg.ensure().unwrap(), serving);
    // A running cluster whose home names no engine (as a build from before the identity left
    // one) is named PostgreSQL's by the start that finds its server, before it serves it.
    let identity = board.dir.join("board/backend.json");
    let written = std::fs::read_to_string(&identity).unwrap();
    assert_eq!(written, r#"{"format":1,"engine":"postgresql"}"#);
    std::fs::remove_file(&identity).unwrap();
    assert_eq!(board.pg.ensure().unwrap(), serving);
    assert_eq!(std::fs::read_to_string(&identity).unwrap(), written);
    let stopped = board.pg.stop().unwrap().expect("it ran");
    assert_eq!(stopped.pid, serving.pid);
    board.none_left();
    assert!(!serving.socket.exists(), "its socket's folder goes with it");
    assert_eq!(board.pg.stop().unwrap(), None, "nothing runs to stop");
    let log = std::fs::read_to_string(board.pg.log()).unwrap();
    assert!(
        log.contains("database system is ready to accept connections"),
        "{log}"
    );
    assert!(log.contains("database system is shut down"), "{log}");
}

#[test]
fn a_second_start_takes_the_data_the_first_made() {
    let board = Board::new("again");
    let first = board.pg.ensure().unwrap();
    board.psql(
        &first,
        "create table kept (said text); insert into kept values ('first')",
    );
    let made = std::fs::metadata(board.pg.data().join("PG_VERSION"))
        .unwrap()
        .modified()
        .unwrap();
    board.pg.stop().unwrap();
    board.none_left();
    let second = board.pg.ensure().unwrap();
    assert_ne!(second.pid, first.pid);
    assert_eq!(board.psql(&second, "select said from kept"), "first");
    let still = std::fs::metadata(board.pg.data().join("PG_VERSION"))
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(made, still, "no second initdb");
    let log = std::fs::read_to_string(board.pg.log()).unwrap();
    let made_by = "The files belonging to this database system will be owned by user";
    assert_eq!(log.matches(made_by).count(), 1, "one initdb: {log}");
    assert_eq!(
        log.matches("ready to accept connections").count(),
        2,
        "{log}"
    );
}

#[test]
fn a_board_whose_folder_is_too_long_for_a_socket_path_starts() {
    let long = std::env::temp_dir().join(format!(
        "pg-{}-long/{}",
        std::process::id(),
        "a-folder-name-that-makes-the-path-long-".repeat(3)
    ));
    let board = Board::at(&long);
    assert!(
        board.pg.data().join(SOCKET_FILE).as_os_str().len() > SOCKET_PATH_MAX,
        "the data's own folder could not hold the socket"
    );
    let serving = board.pg.ensure().unwrap();
    assert_eq!(board.psql(&serving, "select 1"), "1");
    board.pg.stop().unwrap();
    board.none_left();
    drop(board);
    let _ = std::fs::remove_dir_all(long.parent().unwrap());
}

/// `board`'s server, serving, asked to end once its clients have gone (PostgreSQL's smart
/// shutdown, not waited for), and the one connection that keeps it stopping until it is dropped:
/// the state a scratch board's server is in for a moment after the command that started it ended.
fn stopping(board: &Board) -> (Serving, symphony_board::pg::Client) {
    let serving = board.pg.ensure().unwrap();
    let client = symphony_board::pg::Client::new(&serving.socket, "postgres");
    client.simple("SELECT 1").unwrap();
    let asked = sys::output(
        command(board.pg.program("pg_ctl"))
            .args(["stop", "-m", "smart", "-W", "-s", "-D"])
            .arg(board.pg.data()),
    )
    .unwrap();
    assert!(
        asked.status.success(),
        "{}",
        String::from_utf8_lossy(&asked.stderr)
    );
    let record = board.pg.data().join("postmaster.pid");
    test_time::until_true("the server says it is stopping", GONE, || {
        std::fs::read_to_string(&record)
            .is_ok_and(|text| text.lines().nth(7).map(str::trim) == Some("stopping"))
    })
    .unwrap();
    (serving, client)
}

/// A start that meets the server of its data on its way out waits for that server's end, however
/// long its patience, and then serves from one of its own: it is told as it begins to wait, and
/// only then does the test let the old server go.
#[test]
fn a_start_that_meets_a_stopping_server_waits_for_its_end_and_starts_its_own() {
    let board = Board::new("stopping");
    let (serving, client) = stopping(&board);
    let (said, heard) = std::sync::mpsc::channel();
    let pg = board
        .home()
        .telling(move |line| {
            let _ = said.send(line.to_string());
        })
        .postgresql(&programs());
    let starting = std::thread::spawn(move || pg.ensure());
    let told = heard.recv_timeout(GONE).expect("the start says it waits");
    assert_eq!(
        told,
        format!(
            "· the board's PostgreSQL (pid {}) is stopping: this start waits for its end, then \
             starts its own",
            serving.pid
        )
    );
    assert!(pid_alive(serving.pid), "its client still holds it");
    drop(client);
    let next = starting.join().unwrap().unwrap();
    assert_ne!(next.pid, serving.pid);
    assert!(!pid_alive(serving.pid), "the start waited for its end");
    assert_eq!(board.psql(&next, "SELECT 1"), "1");
}

/// One still stopping when the start's patience is met is left to finish, never signalled, and
/// the refusal says it was the wait that stalled, the load, and to run the command again; a
/// start after it has ended answers.
#[test]
fn a_server_still_stopping_at_the_backstop_is_left_and_the_refusal_names_the_retry() {
    let board = Board::new("stopping-stall");
    let (serving, client) = stopping(&board);
    let pg = board
        .home()
        .telling(|_| {})
        .patience(Duration::from_millis(50))
        .postgresql(&programs());
    let refused = pg.ensure().unwrap_err().to_string();
    assert!(
        refused.contains(&format!(
            "the board's PostgreSQL (pid {}) on {} is stopping, and has not ended in the 0 s \
             this start waited for it (load average ",
            serving.pid,
            board.pg.data().display()
        )),
        "{refused}"
    );
    assert!(
        refused.contains(
            "it is left to finish, never signalled. Run the command again once it has ended"
        ),
        "{refused}"
    );
    assert!(pid_alive(serving.pid), "it was left as it was");
    drop(client);
    assert!(sys::gone(serving.pid, GONE).unwrap(), "it ends by itself");
    assert_ne!(board.pg.ensure().unwrap().pid, serving.pid);
}

/// `programs/` in `dir`: the machine's PostgreSQL programs, each a link to its own, but
/// `pg_ctl`, a script of `lines` with `$REAL` naming the machine's `pg_ctl`. A server started from
/// it runs as `programs/postgres`, which the board takes for its own.
fn but_pg_ctl(dir: &Path, lines: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt as _;
    let real = programs();
    let bin = dir.join("programs");
    std::fs::create_dir_all(&bin).unwrap();
    for name in ["postgres", "initdb", "psql"] {
        std::os::unix::fs::symlink(real.join(name), bin.join(name)).unwrap();
    }
    let pg_ctl = bin.join("pg_ctl");
    std::fs::write(
        &pg_ctl,
        format!(
            "#!/bin/sh\nREAL='{}'\n{lines}\nexec \"$REAL\" \"$@\"\n",
            real.join("pg_ctl").display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&pg_ctl, std::fs::Permissions::from_mode(0o755)).unwrap();
    bin
}

/// A stop that finds its server gone by the time `pg_ctl` reads its record has stopped it: that
/// is a scratch board's server ending with the command that started it, between the stop's read
/// of the record and pg_ctl's, and a move that met it then was refused as a server that had not
/// stopped within the board's patience, though nothing was waited for. Here the stop's own
/// `pg_ctl stop` is preceded by one that stops the server first, so pg_ctl finds no record.
#[test]
fn a_stop_whose_server_ended_before_pg_ctl_read_its_record_has_stopped_it() {
    let board = Board::with_pg_ctl(
        "stop-ended",
        "[ \"$1\" != stop ] || \"$REAL\" \"$@\" >/dev/null 2>&1",
    );
    let serving = board.pg.ensure().unwrap();
    let stopped = board.pg.stop().unwrap().expect("it ran");
    assert_eq!(stopped.pid, serving.pid);
    board.none_left();
    assert!(!pid_alive(serving.pid), "it ended");
    assert!(!serving.socket.exists(), "its socket's folder goes with it");
    assert_eq!(board.pg.stop().unwrap(), None, "nothing runs to stop");
}

/// A stop `pg_ctl` says has not ended, whose server still runs on its data, is refused, saying
/// pg_ctl's words and the machine's load, and the server is left serving, never signalled again;
/// the next stop stops it. pg_ctl refuses the first stop here (a marker it removes as it does).
#[test]
fn a_stop_pg_ctl_refuses_while_its_server_runs_is_refused_with_the_load_and_left_serving() {
    let once = folder("stop-refused").join("refuse-once");
    let board = Board::with_pg_ctl(
        "stop-refused",
        &format!(
            "if [ \"$1\" = stop ] && [ -e '{0}' ]; then rm -f '{0}'; \
             echo 'pg_ctl: server does not shut down' >&2; exit 1; fi",
            once.display()
        ),
    );
    let serving = board.pg.ensure().unwrap();
    std::fs::write(&once, "").unwrap();
    let refused = board.pg.stop().unwrap_err().to_string();
    assert!(
        refused.contains(&format!(
            "the board's PostgreSQL (pid {}) was asked to stop, and pg_ctl, which waits 60 s at \
             most for its end, says it has not ended (pg_ctl: server does not shut down; load \
             average ",
            serving.pid
        )),
        "{refused}"
    );
    assert!(
        refused.contains("it is left to finish, never signalled again; its log is "),
        "{refused}"
    );
    assert_eq!(board.psql(&serving, "SELECT 1"), "1", "it still serves");
    assert_eq!(board.pg.stop().unwrap().map(|s| s.pid), Some(serving.pid));
    board.none_left();
}

/// The variable naming the board a helper ([`helper_starts_and_ends`]) starts.
const HELPER_BOARD: &str = "SYMPHONY_PG_FIXTURE_BOARD";

/// The helper test `name` run as a process of its own on `board`, which starts the board's server
/// on its own lifeline and ends: the pid of the server it started, once it has ended.
fn started_by_a_helper(board: &Board, name: &str) -> i64 {
    let said = sys::output(
        command(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                &format!("postgres::{name}"),
                "--test-threads=1",
                "--nocapture",
            ])
            .env(HELPER_BOARD, board.pg.data().parent().unwrap()),
    )
    .unwrap();
    let out = String::from_utf8_lossy(&said.stdout);
    assert!(
        said.status.success(),
        "{out}{}",
        String::from_utf8_lossy(&said.stderr)
    );
    out.lines()
        // After libtest's own words on the test's line.
        .find_map(|line| line.split_once("serving ").map(|(_, pid)| pid))
        .expect("the helper said its server")
        .parse()
        .unwrap()
}

#[test]
fn a_server_ends_with_the_process_that_started_it() {
    let board = Board::new("lifeline");
    let pid = started_by_a_helper(&board, "helper_starts_and_ends");
    board.none_left();
    assert!(!pid_alive(pid), "the server {pid} ended with its starter");
    let log = std::fs::read_to_string(board.pg.log()).unwrap();
    assert!(log.contains("database system is shut down"), "{log}");
}

#[test]
#[ignore = "a helper process: starts the board's PostgreSQL its variable names and ends"]
fn helper_starts_and_ends() {
    println!("serving {}", helper_started().pid);
}

/// The server of the board [`HELPER_BOARD`] names, started by this process on its lifeline.
fn helper_started() -> Serving {
    let dir = PathBuf::from(std::env::var(HELPER_BOARD).expect("the board to start"));
    Home::new(&dir, Path::new("/no/surreal"))
        .postgresql(&programs())
        .ensure()
        .unwrap()
}

/// `signal` (`STOP`, `CONT`) sent to process `pid` by `kill`.
fn signalled(pid: i64, signal: &str) {
    let sent = sys::output(
        command("kill")
            .arg(format!("-{signal}"))
            .arg(pid.to_string()),
    )
    .unwrap();
    assert!(
        sent.status.success(),
        "kill -{signal} {pid}: {}",
        String::from_utf8_lossy(&sent.stderr)
    );
}

/// A server held stopped (SIGSTOP), let go on as the test lets it go or however the test ends.
struct Frozen(i64);

impl Frozen {
    fn thaw(&self) {
        signalled(self.0, "CONT");
    }
}

impl Drop for Frozen {
    fn drop(&mut self) {
        let _ = sys::output(command("kill").arg("-CONT").arg(self.0.to_string()));
    }
}

/// A start that meets a server whose record says it is ready but whose starter has ended takes
/// it for one on its way out: that is a scratch board's server in the moment after the command
/// that started it ended, before its watchdog's signal reaches it, and a command that took it then
/// was refused by its shutdown (`57P03`). The helper holds its server stopped before it ends, so
/// the server reads ready for as long as the test needs; the start says it waits, waits while the
/// server is held, and once the server is let go on to its watchdog's signal, serves from one of
/// its own.
#[test]
fn a_start_that_meets_a_ready_server_whose_starter_has_ended_waits_for_its_end() {
    let board = Board::new("starter-ended");
    let outgoing = Frozen(started_by_a_helper(
        &board,
        "helper_starts_holds_its_server_stopped_and_ends",
    ));
    let record = std::fs::read_to_string(board.pg.data().join("postmaster.pid")).unwrap();
    assert_eq!(
        record.lines().nth(7).map(str::trim),
        Some("ready"),
        "{record}"
    );
    let (said, heard) = std::sync::mpsc::channel();
    let pg = board
        .home()
        .telling(move |line| {
            let _ = said.send(line.to_string());
        })
        .postgresql(&programs());
    let starting = std::thread::spawn(move || pg.ensure());
    let told = heard.recv_timeout(GONE).expect("the start says it waits");
    assert!(
        told.starts_with(&format!(
            "· the board's PostgreSQL (pid {}) ends with its starter (pid ",
            outgoing.0
        )) && told
            .ends_with("), which has ended: this start waits for its end, then starts its own"),
        "{told}"
    );
    assert!(pid_alive(outgoing.0), "held, so not ended");
    outgoing.thaw();
    let next = starting.join().unwrap().unwrap();
    assert_ne!(next.pid, outgoing.0);
    assert!(!pid_alive(outgoing.0), "the start waited for its end");
    assert_eq!(board.psql(&next, "SELECT 1"), "1");
}

#[test]
#[ignore = "a helper process: starts the board's PostgreSQL its variable names, holds the server \
            stopped and ends"]
fn helper_starts_holds_its_server_stopped_and_ends() {
    let serving = helper_started();
    signalled(serving.pid, "STOP");
    println!("serving {}", serving.pid);
}

/// A store on `board`'s database `board`, answering to the board home's own recovery authority.
fn bound_store(board: &Board, serving: &Serving) -> symphony_board::Store {
    let client = symphony_board::pg::Client::new(&serving.socket, "board");
    symphony_board::Store::opening_on(
        client.into(),
        Box::new(|| 1_800_000_000.0),
        Box::new(|_| false),
        "this-host",
        &symphony_board::Opening::write(),
    )
    .unwrap()
    .recovering(std::sync::Arc::new(board.home().recovery("board")))
}

#[test]
fn a_restored_board_is_bound_to_no_generation_and_takes_no_token_of_its_source_once_rebound() {
    use symphony_board::recovery::Authority as _;
    use symphony_board::{Error, NewTask};
    let refused = |result: Result<symphony_board::recovery::BoundTask, Error>| match result {
        Err(Error::Refused(why)) => why,
        other => panic!("not refused: {other:?}"),
    };
    let board = Board::new("bound-source");
    let serving = board.pg.ensure().unwrap();
    let store = bound_store(&board, &serving);
    store.rebind().unwrap();
    let new = |slug: &str| NewTask::new(slug, "Original", "owner");
    store.create(&new("probe/original")).unwrap();
    let kept = store.bound("probe/original").unwrap().bound;
    drop(store);
    board.pg.hold("amber-heron", "snapshot").unwrap();
    let snapshot = board.pg.snapshot(1.0).unwrap();
    board.pg.release("amber-heron", None).unwrap();
    let resumed = board.pg.ensure().unwrap();
    let store = bound_store(&board, &resumed);
    let lost = store.create(&new("probe/lost")).unwrap();
    let lost_at = store.bound("probe/lost").unwrap().bound;
    let later = store
        .amend_bound("probe/original", &kept, "After the snapshot", &[], "owner")
        .unwrap()
        .bound;

    // A restore that cannot revoke the target's bindings publishes no data.
    let blocked = Board::new("bound-blocked");
    std::fs::create_dir_all(blocked.dir.join("board")).unwrap();
    std::fs::write(blocked.dir.join("board/recovery"), "not a folder").unwrap();
    let why = blocked.pg.restore(&snapshot).unwrap_err().to_string();
    assert!(why.contains("recovery"), "{why}");
    assert!(!blocked.pg.data().exists(), "nothing was published");
    std::fs::remove_file(blocked.dir.join("board/recovery")).unwrap();
    blocked.pg.restore(&snapshot).unwrap();
    assert!(
        blocked.pg.data().join("PG_VERSION").is_file(),
        "retried whole"
    );

    // A binding the target home held, the very generation the snapshot carries, is revoked
    // before the data is in place: the restored board is bound to none.
    let recovered = Board::new("bound-target");
    std::fs::create_dir_all(recovered.dir.join("board")).unwrap();
    let authority = recovered.home().recovery("board");
    authority.publish(&kept.generation).unwrap();
    recovered
        .home()
        .recovery("other")
        .publish("another")
        .unwrap();
    recovered.pg.restore(&snapshot).unwrap();
    assert_eq!(authority.bound(), Ok(None));
    assert_eq!(recovered.home().recovery("other").bound(), Ok(None));
    assert_eq!(
        board.home().recovery("board").bound(),
        Ok(Some(kept.generation.clone())),
        "the source's own binding is not touched"
    );
    let restored = recovered.pg.ensure().unwrap();
    let again = bound_store(&recovered, &restored);
    assert_eq!(
        again.recovery_generation().unwrap(),
        Some(kept.generation.clone()),
        "the copied cluster holds the generation its source had"
    );
    for why in [
        refused(again.bound("probe/original")),
        refused(again.amend_bound("probe/original", &kept, "Before the rebind", &[], "owner")),
    ] {
        assert!(why.contains("bound to no recovery generation"), "{why}");
    }

    // Rebound, it holds a generation of its own: the snapshot's token, which names the same task
    // at the same version here, and the tokens made after it are all refused.
    let rebound = again.rebind().unwrap();
    assert_ne!(rebound, kept.generation);
    assert_eq!(authority.bound(), Ok(Some(rebound.clone())));
    let replacement = again.create(&new("probe/replacement")).unwrap();
    assert_eq!(
        replacement.id, lost.id,
        "the number given after the snapshot, given again"
    );
    let now = again.bound("probe/original").unwrap().bound;
    assert_eq!((now.id, now.version), (kept.id, kept.version));
    for (slug, at) in [
        ("probe/original", &kept),
        ("probe/original", &later),
        ("probe/replacement", &lost_at),
    ] {
        let why = refused(again.amend_bound(slug, at, "From the source", &[], "owner"));
        assert!(
            why.contains("copied or restored since this was read"),
            "{slug}: {why}"
        );
    }
    assert_eq!(again.get("probe/original").unwrap().title, "Original");
    again
        .amend_bound(
            "probe/original",
            &now,
            "On the restored board",
            &[],
            "owner",
        )
        .unwrap();
    // The source goes on as it was, its own tokens its own.
    store
        .amend_bound("probe/original", &later, "Still the source", &[], "owner")
        .unwrap();
}
