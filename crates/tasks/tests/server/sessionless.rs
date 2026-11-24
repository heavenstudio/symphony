//! The children the board starts itself run without the session of the process that starts them
//! (`symphony_board::sys::SESSION_VARS`): an agent's command run in its hc terminal holds its board
//! handle and hc's terminal, socket, core and scope, and the board's database server it starts, or
//! an hc call it makes, holds none of them.

use crate::children::Child as _;
use crate::common::script;
use std::path::PathBuf;
use std::time::Duration;
use symphony_board::cli::Env;
use symphony_board::launch::hc::{Binary, Hc};
use symphony_board::server::{Home, ServerError};
use symphony_board::sys::{self, Command};

/// The session the helper below is started in, as an agent's hc terminal starts a command.
const SESSION: [(&str, &str); 5] = [
    ("TASKS_SESSION", "fixture-session"),
    ("HC_TERMINAL_ID", "t7"),
    ("HC_TERMINAL_SOCKET", "/fixture/hc/t7.sock"),
    ("HC_SESSION", "fixture-core"),
    ("HC_SCOPE", "fixture-scope"),
];

/// What a stand-in holds of [`SESSION`], `|` between: the line it writes to the file its body names.
const SAW: &str = r#"printf '%s|%s|%s|%s|%s\n' "${TASKS_SESSION-unset}" "${HC_TERMINAL_ID-unset}" "${HC_TERMINAL_SOCKET-unset}" "${HC_SESSION-unset}" "${HC_SCOPE-unset}""#;

/// The folder the helper is handed.
const DIR: &str = "SYMPHONY_TEST_SESSION_DIR";

/// Run as a process of its own by the next test, started in [`SESSION`]: what this process holds of
/// it, then a start of the board's PostgreSQL by stand-in programs, each writing what it holds
/// (`postgres --version`, `initdb`, then the server, which ends at once), and one hc call made
/// through the board's hc layer in this process's environment, which a stand-in `hc` answers the
/// same way.
#[test]
#[ignore = "a helper the next test runs as a process"]
fn helper_children_of_a_session() {
    let Some(dir) = std::env::var_os(DIR).map(PathBuf::from) else {
        return;
    };
    let held: Vec<String> = SESSION
        .iter()
        .map(|(name, _)| std::env::var(name).unwrap_or_else(|_| "unset".into()))
        .collect();
    std::fs::write(dir.join("helper-saw"), format!("{}\n", held.join("|"))).unwrap();
    let ended = Home::at(&dir.join("board"))
        .telling(|_| {})
        .patience(Duration::from_secs(30))
        .postgresql(&dir.join("bin"))
        .ensure();
    assert!(matches!(ended, Err(ServerError::NotUp(_))), "{ended:?}");
    Binary::of(&Env::of_process()).run(&["ls"]).unwrap();
}

#[test]
fn the_boards_server_and_its_hc_calls_run_without_their_starters_session() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("server-{}-sessionless", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    let saw = |name: &str| format!("{SAW} > '{}'", dir.join(name).display());
    script(
        &dir.join("bin/postgres"),
        &format!(
            "if [ \"$1\" = --version ]; then\n{}\necho 'postgres (PostgreSQL) 18.1'\nexit 0\nfi\n{}\nexit 1",
            saw("version-saw"),
            saw("server-saw")
        ),
    );
    // The data a real `initdb` makes, as far as a start reads it.
    script(
        &dir.join("bin/initdb"),
        &format!(
            "{}\nmkdir -p \"$2\" && echo 18 > \"$2/PG_VERSION\"",
            saw("initdb-saw")
        ),
    );
    for program in ["pg_ctl", "psql"] {
        script(&dir.join("bin").join(program), "exit 1");
    }
    script(&dir.join("hc"), &format!("{}\necho '{{}}'", saw("hc-saw")));
    let done = sys::output(
        Command::of(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "sessionless::helper_children_of_a_session",
                "--test-threads=1",
            ])
            .env(DIR, &dir)
            .env("HC_BIN", dir.join("hc"))
            .envs(SESSION),
    )
    .unwrap();
    let said = String::from_utf8_lossy(&done.stdout);
    assert!(
        done.status.success() && said.contains("1 passed"),
        "{said}{}",
        String::from_utf8_lossy(&done.stderr)
    );
    let read = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap();
    assert_eq!(
        read("helper-saw"),
        "fixture-session|t7|/fixture/hc/t7.sock|fixture-core|fixture-scope\n",
        "the starter's own"
    );
    for child in ["version-saw", "initdb-saw", "server-saw", "hc-saw"] {
        assert_eq!(read(child), "unset|unset|unset|unset|unset\n", "{child}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
