//! Where no `lsof` runs, whether the board's server holds its port's listener itself cannot be told:
//! unknown, never "not listening". The server that answers is kept, started or found, and one line
//! says the check could not run; a read of the recorded server after says nothing. Each case runs
//! the helper below as a process of its own on a `PATH` of the case's: the `ps` the board tells its
//! servers by, and the `lsof` the case gives or none.

use crate::common::{Scratch, Told, surreal};
use std::path::{Path, PathBuf};
use std::time::Duration;
use symphony_board::port::LSOF_AT;
use symphony_board::rpc;
use symphony_board::server::{Home, Running};
use symphony_board::sys::{self, Command, pid_alive};

/// The board folder the helper is handed.
const DIR: &str = "SYMPHONY_TEST_NO_LSOF_DIR";

/// What the helper's board was told, a line each, in the board folder.
const TOLD: &str = "told";

/// How long the helper's `Home` waits for its server to answer, no part of the case: the backstop of
/// the 20 s the server cases give it.
const PATIENCE: Duration = test_time::backstop(Duration::from_secs(20));

/// Run as a process of its own by the cases below: the board's server started, read again while
/// recorded, and found again once its address file is gone, then the lines told kept in the folder.
#[test]
#[ignore = "a helper the cases below run as a process"]
fn helper_a_board_on_a_path_of_its_own() {
    let Some(dir) = std::env::var_os(DIR).map(PathBuf::from) else {
        return;
    };
    let told = Told::default();
    let home = Home::new(&dir, &surreal())
        .patience(PATIENCE)
        .telling(told.sink());
    let started = home.ensure().unwrap();
    assert_eq!(home.ensure().unwrap(), started, "the recorded server, read");
    std::fs::remove_file(dir.join("server.json")).unwrap();
    assert_eq!(home.ensure().unwrap(), started, "the same server, found");
    assert_eq!(home.recorded(), Some(started.clone()), "and recorded again");
    assert!(pid_alive(started.pid), "the server was kept");
    assert!(rpc::healthy(&started.addr), "and answers");
    std::fs::write(dir.join(TOLD), told.lines().join("\n")).unwrap();
}

/// The helper run on a board of `s` with `PATH` the folder `bin`: the server it recorded, and each
/// line its board told.
fn on_path(s: &Scratch, bin: &Path) -> (Running, Vec<String>) {
    let done = sys::output(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "no_lsof::helper_a_board_on_a_path_of_its_own",
                "--test-threads=1",
            ])
            .env(DIR, &s.dir)
            .env("SYMPHONY_SURREAL", s.surreal())
            .env("PATH", bin),
    )
    .unwrap();
    let said = String::from_utf8_lossy(&done.stdout);
    assert!(
        done.status.success() && said.contains("1 passed"),
        "{said}{}",
        String::from_utf8_lossy(&done.stderr)
    );
    let running = s.home().recorded().expect("the helper recorded its server");
    let told = std::fs::read_to_string(s.dir.join(TOLD)).unwrap();
    (running, told.lines().map(str::to_string).collect())
}

/// A folder of `s`'s to be a `PATH` alone, holding `ps` and nothing else yet.
fn bin(s: &Scratch) -> PathBuf {
    let bin = s.dir.join("bin");
    std::fs::create_dir(&bin).unwrap();
    std::os::unix::fs::symlink("/bin/ps", bin.join("ps")).unwrap();
    bin
}

/// What a start of the board of `s` says first.
fn starts(s: &Scratch) -> String {
    format!(
        "· the board's server is not running: it starts now, on its store at {}, which takes some \
         seconds",
        s.dir.join("data").display()
    )
}

/// The line said of `running` where its listener cannot be told, `why`.
fn unsure(running: &Running, why: &str) -> String {
    format!(
        "· the board's server (pid {}, {}) answers, and whether it holds that port's listener \
         itself cannot be told, so it is kept as up: {why}",
        running.pid, running.addr
    )
}

/// An `lsof` the board may not run, the only one on its `PATH`: the server it starts answers and is
/// kept, never ended, and so is the one it finds unrecorded; each says the check could not run, and
/// the read between says nothing.
#[test]
fn a_server_whose_listener_no_lsof_can_tell_is_kept_and_said_in_one_line() {
    let s = Scratch::new("lsof-denied");
    let bin = bin(&s);
    let lsof = bin.join("lsof");
    std::fs::write(&lsof, "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&lsof, std::os::unix::fs::PermissionsExt::from_mode(0o644)).unwrap();
    let (running, told) = on_path(&s, &bin);
    let unsure = unsure(
        &running,
        "lsof could not run: Permission denied (os error 13)",
    );
    assert_eq!(told, [starts(&s), unsure.clone(), unsure]);
}

/// No `lsof` on `PATH`, as a launchd job's or a cron line's may hold none: the one at `/usr/sbin`
/// tells, and nothing more is said; on a machine with none there either, the check could not run.
#[test]
fn where_no_lsof_is_on_path_the_one_in_usr_sbin_tells() {
    let s = Scratch::new("lsof-off-path");
    let (running, told) = on_path(&s, &bin(&s));
    let mut said = vec![starts(&s)];
    if !Path::new(LSOF_AT).exists() {
        let unsure = unsure(
            &running,
            "lsof could not run: none is found on PATH, and /usr/sbin/lsof could not run either \
             (No such file or directory (os error 2))",
        );
        said.extend([unsure.clone(), unsure]);
    }
    assert_eq!(told, said);
}
