//! The board's PostgreSQL on a scratch board: `Home` makes its data, starts it on a Unix socket
//! alone and stops it, leaving no process; a second start takes the data the first made; a board
//! under a folder too long for a socket path starts; and a server ends with the process that
//! started it. Each test's board is a folder of its own under the temporary folder, its server
//! the machine's PostgreSQL the repository's project file names (`[board] postgresql`).

use std::path::{Path, PathBuf};
use std::time::Duration;
use symphony_board::server::postgres::{self, SOCKET_FILE, SOCKET_PATH_MAX, USER};
use symphony_board::server::{Home, Postgres, Serving};
use symphony_board::sys::{self, command, pid_alive};

/// How long a server's processes are given to be gone once it has stopped: they end at once, its
/// supervisor's shells behind it.
const GONE: Duration = test_time::backstop(Duration::from_secs(10));

/// The folder of the machine's PostgreSQL programs, as the repository's project file names it.
fn programs() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    postgres::programs(&root).unwrap().expect(
        "the repository's symphony.toml names the machine's PostgreSQL ([board] postgresql)",
    )
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
        let _ = self.pg.stop();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
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
