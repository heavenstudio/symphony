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

/// A board of a test's own under the temporary folder, its PostgreSQL stopped and its folder
/// removed when it goes, however the test ends.
struct Board {
    dir: PathBuf,
    pg: Postgres,
}

impl Board {
    fn new(name: &str) -> Board {
        Board::at(&std::env::temp_dir().join(format!("pg-{}-{name}", std::process::id())))
    }

    fn at(dir: &Path) -> Board {
        if dir.exists() {
            std::fs::remove_dir_all(dir).unwrap();
        }
        std::fs::create_dir_all(dir).unwrap();
        let pg = Home::new(&dir.join("board"), Path::new("/no/surreal"))
            .telling(|_| {})
            .postgresql(&programs());
        Board {
            dir: dir.into(),
            pg,
        }
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

/// The variable naming the board [`helper_starts_and_ends`] starts.
const HELPER_BOARD: &str = "SYMPHONY_PG_FIXTURE_BOARD";

#[test]
fn a_server_ends_with_the_process_that_started_it() {
    let board = Board::new("lifeline");
    let said = sys::output(
        command(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "postgres::helper_starts_and_ends",
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
    let pid: i64 = out
        .lines()
        // After libtest's own words on the test's line.
        .find_map(|line| line.split_once("serving ").map(|(_, pid)| pid))
        .expect("the helper said its server")
        .parse()
        .unwrap();
    board.none_left();
    assert!(!pid_alive(pid), "the server {pid} ended with its starter");
    let log = std::fs::read_to_string(board.pg.log()).unwrap();
    assert!(log.contains("database system is shut down"), "{log}");
}

#[test]
#[ignore = "a helper process: starts the board's PostgreSQL its variable names and ends"]
fn helper_starts_and_ends() {
    let dir = PathBuf::from(std::env::var(HELPER_BOARD).expect("the board to start"));
    let serving = Home::new(&dir, Path::new("/no/surreal"))
        .postgresql(&programs())
        .ensure()
        .unwrap();
    println!("serving {}", serving.pid);
}
