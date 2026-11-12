//! `Home`: the board's server found by its address file, started when it is not running, restarted
//! when it was killed, replaced when it is an older version and used as it is when newer, and never
//! confused with another process or with a server on other data.

use crate::common::{Scratch, command_of, exists, gone, healthy_occupier, script, signal, surreal};
use crate::home_fixture::{Unresponsive, healthy_server};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use symphony_board::port::{ATTEMPTS, Start, in_use, listens, start_on_free_port};
use symphony_board::rpc::{self, Client};
use symphony_board::server::{FOLDER, Home, Running, SURREAL_VERSION, ServerError};
use symphony_board::sys::{self, Command, pid_alive};
use symphony_board::{NewTask, Store};

fn board(running: &Running) -> Store {
    Store::open(Client::new(&running.addr, "sf", "tasks")).unwrap()
}

unsafe extern "C" {
    fn getsid(pid: i32) -> i32;
}

/// The session `pid` is in: its terminal's, unless it made one of its own.
fn session_of(pid: i64) -> i32 {
    // SAFETY: getsid only reads the process table.
    unsafe { getsid(i32::try_from(pid).unwrap()) }
}

#[test]
fn nothing_running_starts_a_server_that_is_recorded_and_found_again() {
    let s = Scratch::new("start");
    let home = s.home();
    assert_eq!(home.recorded(), None);
    let first = home.ensure().unwrap();
    assert!(first.addr.starts_with("127.0.0.1:"), "{}", first.addr);
    assert!(rpc::healthy(&first.addr));
    assert!(pid_alive(first.pid));
    assert_eq!(first.version, SURREAL_VERSION);
    assert!(command_of(first.pid).unwrap().contains(&s.data()));
    assert_eq!(home.recorded(), Some(first.clone()));
    assert_eq!(home.ensure().unwrap(), first);
    assert_eq!(s.home().ensure().unwrap(), first);
}

#[test]
fn a_killed_server_is_started_again_on_the_same_board() {
    let s = Scratch::new("restart");
    let home = s.home();
    let first = home.ensure().unwrap();
    board(&first)
        .create(&NewTask::new("sim/jump", "Jump departure", "owner"))
        .unwrap();
    signal(first.pid, "KILL");
    assert!(gone(first.pid));
    let again = home.ensure().unwrap();
    assert_ne!(again.pid, first.pid);
    assert!(rpc::healthy(&again.addr));
    assert_eq!(
        board(&again).get("sim/jump").unwrap().title,
        "Jump departure"
    );
}

#[test]
fn callers_at_once_start_one_server() {
    let s = Scratch::new("race");
    let found: Vec<Running> = std::thread::scope(|scope| {
        let callers: Vec<_> = (0..6).map(|_| scope.spawn(|| s.home().ensure())).collect();
        callers
            .into_iter()
            .map(|caller| caller.join().unwrap().unwrap())
            .collect()
    });
    assert!(
        found.iter().all(|running| *running == found[0]),
        "{found:?}"
    );
}

#[test]
fn a_server_of_another_version_is_stopped_and_replaced() {
    let s = Scratch::new("upgrade");
    let home = s.home();
    let old = home.ensure().unwrap();
    s.record(&Running {
        version: "0.0.1".into(),
        ..old.clone()
    });
    let new = home.ensure().unwrap();
    assert_eq!(new.version, SURREAL_VERSION);
    assert_ne!(new.pid, old.pid);
    assert!(gone(old.pid));
    assert_eq!(home.recorded(), Some(new));
}

/// A tree that pins an older SurrealDB than the one running (a branch cut before main moved the pin)
/// uses the newer server rather than ending it: two trees with two pins would otherwise end each
/// other's server on every call.
#[test]
fn a_server_of_a_newer_version_is_used_and_never_stopped() {
    let s = Scratch::new("newer");
    let home = s.home();
    let running = home.ensure().unwrap();
    let newer = Running {
        version: "99.0.0".into(),
        ..running.clone()
    };
    s.record(&newer);
    assert_eq!(home.ensure().unwrap(), newer);
    assert!(pid_alive(running.pid));
    assert_eq!(home.recorded(), Some(newer));
}

/// fleet-vole: a server on this board's data that the address file does not name (its starter was killed
/// between its answer and its record) holds the data's lock, so every later start would fail; the board
/// finds it by its data and records it.
#[test]
fn a_server_on_this_boards_data_that_was_never_recorded_is_found() {
    let s = Scratch::new("unrecorded");
    let log = s.dir.join("orphan.log");
    // Started again on another port only if its port was taken before it bound it, every start
    // within the production patience; up once the server itself listens there.
    let until = Instant::now() + Duration::from_secs(60);
    let (mut orphan, addr) = start_on_free_port(ATTEMPTS, until, |reserved| {
        let addr = reserved.to_string();
        let out = std::fs::File::create(&log).unwrap();
        let mut orphan = Command::new(s.surreal())
            .args([
                "start",
                "--unauthenticated",
                "--log",
                "error",
                "--bind",
                &addr,
                &s.data(),
            ])
            .stdout(out.try_clone().unwrap())
            .stderr(out)
            .spawn()
            .unwrap();
        loop {
            if orphan.try_wait().unwrap().is_some() {
                let said = std::fs::read_to_string(&log).unwrap();
                return Ok::<_, Infallible>(Start::Ended(said));
            }
            if rpc::healthy(&addr) && listens(orphan.id(), reserved, sys::output).unwrap() {
                return Ok(Start::Up(orphan));
            }
            assert!(
                Instant::now() < until,
                "the server on {addr} neither answered nor ended"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    })
    .unwrap_or_else(|why| panic!("the server ended: {why}"));
    let addr = addr.to_string();
    let found = s.home().patience(Duration::from_secs(20)).ensure();
    let _ = orphan.kill();
    let _ = orphan.wait();
    let found = found.unwrap();
    assert_eq!((found.addr, found.pid), (addr, i64::from(orphan.id())));
}

/// An address file naming a server that answers but is not this board's (another SurrealDB, on other
/// data) is not trusted: this board's own server is started and recorded, and the other left alone.
#[test]
fn a_recorded_server_that_serves_other_data_is_not_this_board() {
    let s = Scratch::new("foreign");
    let other = Scratch::new("foreign-other");
    let foreign = other.home().ensure().unwrap();
    s.record(&foreign);
    let ours = s.home().ensure().unwrap();
    assert_ne!(ours.addr, foreign.addr);
    assert!(command_of(ours.pid).unwrap().contains(&s.data()));
    assert_eq!(s.home().recorded(), Some(ours));
    assert!(pid_alive(foreign.pid), "the other server was left alone");
    other.home().stop().unwrap();
}

#[test]
fn a_recorded_process_that_is_not_the_boards_server_is_never_signalled() {
    let s = Scratch::new("stranger");
    let mut stranger = Command::new("sleep").arg("60").spawn().unwrap();
    let pid = i64::from(stranger.id());
    for version in [SURREAL_VERSION, "0.0.1"] {
        s.record(&Running {
            addr: "127.0.0.1:1".into(),
            pid,
            version: version.into(),
        });
        let started = s.home().ensure().unwrap();
        assert_ne!(started.pid, pid);
        assert!(stranger.try_wait().unwrap().is_none(), "the stranger runs");
        s.home().stop().unwrap();
    }
    stranger.kill().unwrap();
    stranger.wait().unwrap();
}

#[test]
fn a_server_that_runs_and_does_not_answer_is_waited_for_and_left_alone() {
    let s = Scratch::new("unresponsive");
    let mut server = Unresponsive::start(&s.data());
    let running = Running {
        addr: server.addr.clone(),
        pid: server.pid(),
        version: SURREAL_VERSION.into(),
    };
    assert!(command_of(running.pid).unwrap().contains(&s.data()));
    s.record(&running);

    let patience = Duration::from_millis(250);
    let home = s.home().patience(patience);
    let began = Instant::now();
    let result = home.ensure();
    let elapsed = began.elapsed();
    assert!(elapsed >= patience, "returned after {elapsed:?}");
    assert!(
        server.probes() >= 2,
        "the initial health check and a retry must reach the live server"
    );
    let error = result.unwrap_err();
    assert!(
        matches!(&error, ServerError::NotUp(why) if
            why.contains("runs and does not answer") &&
            why.contains(&running.pid.to_string()) &&
            why.contains(&running.addr) &&
            why.contains(&s.dir.join("server.log").display().to_string())),
        "{error}"
    );
    assert!(server.is_running(), "the unresponsive server was stopped");
    assert_eq!(home.recorded(), Some(running));
    assert!(server.shutdown().success(), "the helper must exit normally");
}

#[test]
fn a_server_that_cannot_start_says_why_from_its_log() {
    // A held data lock without a recoverable server must fail visibly.
    let s = Scratch::new("cannot");
    let failing = stand_in(
        &s,
        "echo 'There was a problem with the datastore: the disk is full' >&2\nexit 1",
    );
    let error = Home::new(&s.dir, &failing)
        .patience(Duration::from_secs(20))
        .ensure()
        .unwrap_err();
    assert!(
        matches!(&error, ServerError::NotUp(why) if why.contains("the disk is full")),
        "{error}"
    );
    assert_eq!(starts(&s), 1, "a server that failed is not started again");
    assert_eq!(s.home().recorded(), None);
}

/// A `surreal` that counts its starts in the folder's `starts` and then runs `body`; run once
/// already, uncounted ([`script`]).
fn stand_in(s: &Scratch, body: &str) -> PathBuf {
    script(
        &s.dir.join("surreal-stand-in"),
        &format!(
            concat!(
                "starts=$(cat '{starts}' 2>/dev/null || echo 0)\n",
                "starts=$((starts + 1))\n",
                "echo \"$starts\" > '{starts}'\n",
                "{body}",
            ),
            starts = s.dir.join("starts").display(),
            body = body
        ),
    )
}

fn starts(s: &Scratch) -> usize {
    std::fs::read_to_string(s.dir.join("starts"))
        .unwrap()
        .trim()
        .parse()
        .unwrap()
}

/// What SurrealDB says as it ends when another process bound its port first.
const TAKEN: &str = "ERROR surrealdb_server::cli: Address already in use (os error 48)";

/// A `surreal` whose first `taken` starts end as SurrealDB does when its port was taken before it
/// bound it, and whose later starts run the fixture's guarded SurrealDB.
fn taken_then_real(s: &Scratch, taken: usize) -> PathBuf {
    stand_in(
        s,
        &format!(
            "if [ \"$starts\" -le {taken} ]; then echo '{TAKEN}'; exit 1; fi\nexec '{}' \"$@\"",
            s.surreal().display()
        ),
    )
}

#[test]
fn a_server_whose_port_was_taken_before_it_bound_it_is_started_on_another() {
    let s = Scratch::new("port-taken");
    let home = Home::new(&s.dir, &taken_then_real(&s, 1));
    let running = home.ensure().unwrap();
    assert_eq!(starts(&s), 2);
    assert!(rpc::healthy(&running.addr));
    assert!(command_of(running.pid).unwrap().contains(&s.data()));
    assert_eq!(home.recorded(), Some(running));
    let log = std::fs::read_to_string(s.dir.join("server.log")).unwrap();
    assert_eq!(log.matches(TAKEN).count(), 1, "{log}");
}

#[test]
fn a_server_whose_port_is_taken_on_every_start_is_given_up_on_saying_so() {
    let s = Scratch::new("port-always-taken");
    let error = Home::new(&s.dir, &taken_then_real(&s, ATTEMPTS))
        .patience(Duration::from_secs(20))
        .ensure()
        .unwrap_err();
    assert_eq!(starts(&s), ATTEMPTS);
    let log = s.dir.join("server.log").display().to_string();
    assert_eq!(
        error,
        ServerError::NotUp(format!(
            "the board's server did not start: its port was taken on each of {ATTEMPTS} starts, \
             the last saying {TAKEN}; its log is {log}"
        ))
    );
    assert_eq!(s.home().recorded(), None);
}

/// What took the port before the server bound it answers as a healthy server does: the answer is
/// not the server's, so it is started again on another port, and only that one is recorded. The
/// first start says which port it was given and ends as SurrealDB does once the occupier has
/// answered; the case takes that port with the occupier, and fails if the port is not said within
/// 20 s. The second start is a server that is up as soon as it listens ([`healthy_server`]), so every
/// wait is on an event: the port said, the occupier's answer, the first start's end, the second's
/// own listener.
#[test]
fn a_server_whose_taken_port_answers_is_still_started_on_another() {
    let s = Scratch::new("port-taken-answers");
    let bind = s.dir.join("bind");
    let probed = s.dir.join("probed");
    let surreal = stand_in(
        &s,
        &format!(
            concat!(
                "if [ \"$starts\" -le 1 ]; then\n",
                "    for word do\n",
                "        [ \"$last\" = --bind ] && echo \"$word\" > '{bind}.new'\n",
                "        last=$word\n",
                "    done\n",
                "    mv '{bind}.new' '{bind}'\n",
                "    while [ ! -f '{probed}' ]; do sleep 0.01; done\n",
                "    echo '{taken}'; exit 1\n",
                "fi\n",
                "exec '{server}' \"$@\"",
            ),
            bind = bind.display(),
            probed = probed.display(),
            taken = TAKEN,
            server = healthy_server(&s.dir).display()
        ),
    );
    let home = Home::new(&s.dir, &surreal).patience(Duration::from_secs(20));
    let (running, taken) = std::thread::scope(|scope| {
        // Every script the case runs has run once already, so this clock times the start alone.
        let started = scope.spawn(|| home.ensure());
        let began = Instant::now();
        while !exists(&bind) {
            assert!(
                began.elapsed() < Duration::from_secs(20) && !started.is_finished(),
                "the first start never said its port"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let taken: SocketAddr = std::fs::read_to_string(&bind)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert_eq!(healthy_occupier(taken, &probed), taken);
        (started.join().unwrap().unwrap(), taken)
    });
    assert!(exists(&probed), "the occupier was asked whether it was up");
    assert_eq!(starts(&s), 2);
    assert_ne!(running.addr, taken.to_string());
    assert!(rpc::healthy(&running.addr));
    let pid = u32::try_from(running.pid).unwrap();
    assert!(listens(pid, running.addr.parse().unwrap(), sys::output).unwrap());
    assert!(command_of(running.pid).unwrap().contains(&s.data()));
    assert_eq!(home.recorded(), Some(running));
    let log = std::fs::read_to_string(s.dir.join("server.log")).unwrap();
    assert_eq!(log.matches(TAKEN).count(), 1, "{log}");
}

/// A process on this board's data told to bind an address a healthy occupier holds, and held up
/// before its bind until the occupier has answered `probes` health checks: then it ends as SurrealDB
/// does finding its port taken. It is reaped as it ends, so its pid is gone once it has. Its
/// `surreal` is the board's too: the next start is a server that is up as soon as it listens
/// ([`healthy_server`]). So the case waits on events alone: the occupier's answers, this process's
/// end, the next server's own listener.
struct Held {
    pid: i64,
    addr: SocketAddr,
    probes: usize,
    probed: PathBuf,
    surreal: PathBuf,
    log: PathBuf,
    ended: std::thread::JoinHandle<std::process::ExitStatus>,
}

impl Held {
    fn start(s: &Scratch, probes: usize) -> Held {
        let probed = s.dir.join("probed");
        let addr = healthy_occupier("127.0.0.1:0".parse().unwrap(), &probed);
        let surreal = stand_in(
            s,
            &format!(
                concat!(
                    "if [ \"$starts\" -le 1 ]; then\n",
                    "    until [ \"$(cat '{probed}' 2>/dev/null)\" -ge {probes} ] 2>/dev/null; do\n",
                    "        sleep 0.01\n",
                    "    done\n",
                    "    echo '{taken}'; exit 1\n",
                    "fi\n",
                    "exec '{server}' \"$@\"",
                ),
                probed = probed.display(),
                probes = probes,
                taken = TAKEN,
                server = healthy_server(&s.dir).display()
            ),
        );
        let log = s.dir.join("held.log");
        let out = std::fs::File::create(&log).unwrap();
        let mut child = Command::new(&surreal)
            .args(["start", "--unauthenticated", "--log", "error", "--bind"])
            .arg(addr.to_string())
            .arg(s.data())
            .stdout(out.try_clone().unwrap())
            .stderr(out)
            .spawn()
            .unwrap();
        let pid = i64::from(child.id());
        let ended = std::thread::spawn(move || child.wait().unwrap());
        Held {
            pid,
            addr,
            probes,
            probed,
            surreal,
            log,
            ended,
        }
    }

    /// The board's home, started by the held process's `surreal`.
    fn home(&self, s: &Scratch) -> Home {
        Home::new(&s.dir, &self.surreal).patience(Duration::from_secs(20))
    }

    /// It ended finding its port taken once the occupier had answered as often as it waited for,
    /// and `running`, the board's server now, is the one other start: another process, on another
    /// address, answering there from a listener of its own, on this board's data, and recorded.
    fn ended_and_gave_way_to(self, s: &Scratch, running: &Running) {
        let status = self.ended.join().unwrap();
        let said = std::fs::read_to_string(&self.log).unwrap();
        assert_eq!(status.code(), Some(1), "{said}");
        assert!(in_use(&said), "{said}");
        let answered: usize = std::fs::read_to_string(&self.probed)
            .unwrap()
            .parse()
            .unwrap();
        assert!(answered >= self.probes, "the occupier answered {answered}");
        assert_eq!(starts(s), 2);
        assert_ne!(running.pid, self.pid);
        assert_ne!(running.addr, self.addr.to_string());
        assert!(rpc::healthy(&running.addr));
        let pid = u32::try_from(running.pid).unwrap();
        assert!(listens(pid, running.addr.parse().unwrap(), sys::output).unwrap());
        assert!(command_of(running.pid).unwrap().contains(&s.data()));
        assert_eq!(s.home().recorded().as_ref(), Some(running));
    }
}

/// A process on this board's data that no address file names (its starter ended before it bound its
/// port) is found by its data while another process answers on its port as a healthy server does:
/// that answer is not its own, so it is never recorded, and once it ends, finding its port taken,
/// the board's own server is started in its place.
#[test]
fn an_unrecorded_server_whose_port_another_answers_on_is_never_recorded() {
    let s = Scratch::new("unrecorded-taken");
    let held = Held::start(&s, 1);
    let running = held.home(&s).ensure().unwrap();
    held.ended_and_gave_way_to(&s, &running);
}

/// The address file names a process on this board's data while another process answers on its
/// address as a healthy server does. It is held up past two answers, so neither the quick check nor
/// the wait under the lock can take an answer for its own while it runs; once it ends, finding its
/// port taken, the board's own server is started in its place.
#[test]
fn a_recorded_server_whose_port_another_answers_on_is_never_taken_as_up() {
    let s = Scratch::new("recorded-taken");
    let held = Held::start(&s, 2);
    s.record(&Running {
        addr: held.addr.to_string(),
        pid: held.pid,
        version: SURREAL_VERSION.into(),
    });
    let running = held.home(&s).ensure().unwrap();
    held.ended_and_gave_way_to(&s, &running);
}

#[test]
fn stop_ends_the_recorded_server_and_forgets_it() {
    let s = Scratch::new("stop");
    let home = s.home();
    let running = home.ensure().unwrap();
    assert_eq!(home.stop().unwrap(), Some(running.clone()));
    assert!(!pid_alive(running.pid));
    assert_eq!(home.recorded(), None);
    assert_eq!(home.stop().unwrap(), None);
    let again = home.ensure().unwrap();
    assert!(rpc::healthy(&again.addr));
}

#[test]
fn no_pinned_surreal_is_refused_with_how_to_fetch_it() {
    let s = Scratch::new("nosurreal");
    let missing = s.dir.join("no-such").join("surreal");
    let error = Home::new(&s.dir, &missing).ensure().unwrap_err();
    assert_eq!(error, ServerError::NoSurreal(missing.clone()));
    assert_eq!(
        error.to_string(),
        format!(
            "SurrealDB {SURREAL_VERSION} is not at {}, where `SYMPHONY_SURREAL` names it",
            missing.display()
        )
    );
    assert!(!exists(&s.dir.join("server.json")));
    // The pinned binary's place says the command that fetches it into that repository.
    let common = s.dir.join("repo").join(".git");
    std::fs::create_dir_all(&common).unwrap();
    let error = Home::in_common(&common, None).ensure().unwrap_err();
    let pinned = common
        .join("sf-surreal")
        .join(SURREAL_VERSION)
        .join("surreal");
    assert_eq!(error, ServerError::NoSurreal(pinned.clone()));
    assert_eq!(
        error.to_string(),
        format!(
            "SurrealDB {SURREAL_VERSION} is not at {}: `tools/surreal.sh` fetches it into this \
             repository's common dir (`SYMPHONY_GIT_COMMON_DIR={} bash tools/surreal.sh` in a \
             Symphony checkout), or `SYMPHONY_SURREAL` names another",
            pinned.display(),
            common.display()
        )
    );
}

#[test]
fn a_garbled_address_file_is_no_server() {
    let s = Scratch::new("garbled");
    std::fs::write(s.dir.join("server.json"), "{not json").unwrap();
    assert_eq!(s.home().recorded(), None);
    let running = s.home().ensure().unwrap();
    assert!(rpc::healthy(&running.addr));
}

#[test]
fn the_board_lives_in_the_common_dir_served_by_the_pinned_surreal() {
    let common = Path::new("/repo/.git");
    let home = Home::in_common(common, None);
    assert_eq!(home.dir(), common.join(FOLDER));
    assert_eq!(
        home.surreal(),
        common
            .join("sf-surreal")
            .join(SURREAL_VERSION)
            .join("surreal")
    );
    let named = Path::new("/opt/surreal");
    assert_eq!(Home::in_common(common, Some(named)).surreal(), named);
}

#[test]
fn the_version_is_the_one_tools_surreal_sh_fetches() {
    let script = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tools/surreal.sh"
    ))
    .unwrap();
    assert!(
        script.contains(&format!("\nVERSION={SURREAL_VERSION}\n")),
        "tools/surreal.sh pins another VERSION"
    );
}

#[test]
fn the_scripts_common_dir_is_its_variable_else_the_workspaces() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/surreal.sh");
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("server-{}-common-dir", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let init = sys::output(Command::new("git").args(["init", "-q"]).current_dir(&dir)).unwrap();
    assert!(init.status.success(), "{init:?}");
    let common = dir.join(".git").canonicalize().unwrap();
    let common_dir = |vars: &[(&str, &Path)]| {
        let mut bash = Command::new("bash");
        bash.args(["-c", "source \"$1\"; common_dir \"$2\"", "common_dir"])
            .arg(&script)
            .arg(&dir);
        for name in ["SYMPHONY_GIT_COMMON_DIR", "GIT_DIR", "GIT_COMMON_DIR"] {
            bash.env_remove(name);
        }
        for (name, value) in vars {
            bash.env(name, value);
        }
        let done = sys::output(&mut bash).unwrap();
        (
            done.status.code(),
            String::from_utf8(done.stdout).unwrap(),
            String::from_utf8(done.stderr).unwrap(),
        )
    };
    let found = format!("{}\n", common.display());
    assert_eq!(
        common_dir(&[("SYMPHONY_GIT_COMMON_DIR", &common)]),
        (Some(0), found.clone(), String::new())
    );
    assert_eq!(common_dir(&[]), (Some(0), found.clone(), String::new()));
    assert_eq!(
        common_dir(&[("SYMPHONY_GIT_COMMON_DIR", Path::new(""))]),
        (Some(0), found, String::new()),
        "an empty value is unset"
    );
    // A value that is not a git dir is refused.
    let (code, out, err) = common_dir(&[("SYMPHONY_GIT_COMMON_DIR", Path::new("relative"))]);
    assert_eq!((code, out.as_str()), (Some(1), ""), "{err}");
    assert!(
        err.ends_with(
            "❌ surreal: SYMPHONY_GIT_COMMON_DIR=relative is not a git dir (an absolute path git \
             takes as --git-dir)\n"
        ),
        "{err}"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Run as a process of its own by the next test: start the board's server and exit.
#[test]
#[ignore = "a helper the next test runs as a process"]
fn helper_start_and_exit() {
    if let Some(dir) = std::env::var_os("SYMPHONY_TEST_SERVER_DIR") {
        Home::new(Path::new(&dir), &surreal()).ensure().unwrap();
    }
}

#[test]
fn a_server_outlives_the_process_that_started_it_and_holds_none_of_its_pipes() {
    let s = Scratch::new("outlive");
    // `output` returns only once nothing holds the child's stdout and stderr open.
    let out = symphony_board::sys::output(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "home::helper_start_and_exit",
                "--test-threads=1",
            ])
            .env("SYMPHONY_TEST_SERVER_DIR", &s.dir)
            .env("SYMPHONY_SURREAL", s.surreal()),
    )
    .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let running = s.home().recorded().unwrap();
    assert!(rpc::healthy(&running.addr));
    // A session of its own, with no terminal: a terminal's hangup (a pane closed) cannot reach it, and
    // SurrealDB ends on one, whatever its starter ignores.
    assert_ne!(session_of(running.pid), -1);
    assert_ne!(
        session_of(running.pid),
        session_of(i64::from(std::process::id()))
    );
}
