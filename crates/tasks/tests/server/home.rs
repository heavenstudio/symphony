//! `Home`: the board's server found by its address file, started when it is not running,
//! replaced when it is an older version and used as it is when newer, and never
//! confused with another process or with a server on other data.

use crate::children::Child as _;
use crate::common::{
    Repository, Scratch, Told, command_of, exists, gone, healthy_occupier, parent_of, script,
    surreal,
};
use crate::home_fixture::{Unresponsive, healthy_server};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use symphony_board::NewTask;
use symphony_board::cli::connect;
use symphony_board::port::{ATTEMPTS, Start, in_use, listens, start_on_free_port};
use symphony_board::rpc;
use symphony_board::server::{
    Engine, FOLDER, Home, Running, SURREAL_VERSION, ServerError, scratch,
};
use symphony_board::sys::{self, Command, pid_alive};

/// How long a case's `Home` waits for a server to answer, where that wait is no part of the case:
/// the backstop of the 20 s these cases once gave it.
const PATIENCE: Duration = test_time::backstop(Duration::from_secs(20));

/// How long a case's `Home` gives a server it stops to end: the backstop of the 30 s it once gave.
const CLOSING: Duration = test_time::backstop(Duration::from_secs(30));

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
    let told = Told::default();
    let home = s.home().telling(told.sink());
    assert_eq!(home.recorded(), None);
    assert!(!home.has_store());
    let first = home.ensure().unwrap();
    assert!(first.addr.starts_with("127.0.0.1:"), "{}", first.addr);
    assert!(rpc::healthy(&first.addr));
    assert!(pid_alive(first.pid));
    assert_eq!(first.version, SURREAL_VERSION);
    assert!(command_of(first.pid).unwrap().contains(&s.data()));
    assert_eq!(home.recorded(), Some(first.clone()));
    // The start said so, once: it takes seconds, which would otherwise pass in silence.
    assert_eq!(
        told.lines(),
        [format!(
            "· the board's server is not running: it starts now, on its store at {}, which takes \
             some seconds",
            s.dir.join("data").display()
        )]
    );
    // The server makes the store, never the board.
    assert!(home.has_store());
    assert_eq!(home.schema("tasks").unwrap(), None);
    // A server found running is used, saying nothing.
    assert_eq!(home.ensure().unwrap(), first);
    assert_eq!(s.home().telling(told.sink()).ensure().unwrap(), first);
    assert_eq!(told.lines().len(), 1, "{:?}", told.lines());
}

#[test]
fn callers_at_once_start_one_server() {
    let s = Scratch::new("race");
    let home = Home::new(&s.dir, &healthy_server(&s.dir));
    let found: Vec<Running> = std::thread::scope(|scope| {
        let callers: Vec<_> = (0..6).map(|_| scope.spawn(|| home.ensure())).collect();
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
    let home = Home::new(&s.dir, &healthy_server(&s.dir));
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
    let home = Home::new(&s.dir, &healthy_server(&s.dir));
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
    let until = Instant::now() + test_time::backstop(Duration::from_secs(60));
    let (mut orphan, addr) = start_on_free_port(ATTEMPTS, until, |reserved| {
        let addr = reserved.to_string();
        let out = std::fs::File::create(&log).unwrap();
        let mut orphan = Command::of(s.surreal())
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
        // Up once it answers on its port; ended once it exits first.
        let up = test_time::until(
            format!("the server on {addr} answers or ends"),
            until.saturating_duration_since(Instant::now()),
            || {
                if orphan.try_wait().unwrap().is_some() {
                    return Some(false);
                }
                (rpc::healthy(&addr) && listens(orphan.id(), reserved, sys::output).unwrap())
                    .then_some(true)
            },
        )
        .unwrap();
        if up {
            return Ok::<_, Infallible>(Start::Up(orphan));
        }
        Ok(Start::Ended(std::fs::read_to_string(&log).unwrap()))
    })
    .unwrap_or_else(|why| panic!("the server ended: {why}"));
    let addr = addr.to_string();
    let found = s.home().patience(PATIENCE).ensure();
    let _ = orphan.kill();
    let _ = orphan.wait();
    let found = found.unwrap();
    assert_eq!((found.addr, found.pid), (addr, i64::from(orphan.id())));
}

/// A scratch board's server runs under a supervisor shell of the board's, whose command line holds
/// the server's words (`start`, `--bind`, the data) among its own arguments. With its record gone
/// while it runs, as when its starter is killed between its answer and its record, the next command
/// finds the server itself and uses it at once. A supervisor taken for it would be waited on for the
/// whole patience at its script's `"$1"`, then ended in its place, and a second server refused the
/// data's lock.
#[test]
fn a_scratch_boards_unrecorded_server_is_found_and_used_at_once_never_its_supervisor() {
    let repo = Repository::in_scratch("scratch-unrecorded");
    // A board on SurrealDB, whose server this case is about, named before its first command.
    Home::in_common(&repo.common(), Some(repo.surreal()))
        .initialize_backend(Engine::SurrealDb)
        .unwrap();
    let store = connect(&repo.env()).unwrap();
    store
        .create(&NewTask::new("sim/found", "Found again", "owner"))
        .unwrap();
    let home = Home::in_common(&repo.common(), Some(repo.surreal()));
    assert!(scratch(home.dir()), "{}", home.dir().display());
    let running = home.recorded().expect("the server this process started");
    let supervisor = parent_of(running.pid).expect("the server's parent");
    assert_ne!(supervisor, 1, "a scratch board's server is supervised");
    let held = command_of(supervisor).unwrap();
    assert!(
        held.contains(" start ")
            && held.contains(" --bind ")
            && held.contains(&running_data(&home)),
        "the supervisor holds the server's words: {held}"
    );
    std::fs::remove_file(home.dir().join("server.json")).unwrap();

    let listed = sys::output(
        Command::of(env!("CARGO_BIN_EXE_tasks"))
            .arg("list")
            .current_dir(&repo.dir)
            .cleared()
            .envs(repo.env().vars)
            .env("TASKS_SWEEP", "off")
            .env("PATH", std::env::var_os("PATH").unwrap_or_default()),
    )
    .unwrap();
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&listed.stdout),
        String::from_utf8_lossy(&listed.stderr)
    );
    assert!(listed.status.success(), "{said}");
    assert!(said.contains("sim/found"), "{said}");
    // Found, not started again: the same server recorded, and its supervisor left alone, which a
    // start that waited out its patience on the lost record and replaced the server would not leave.
    assert_eq!(home.recorded(), Some(running.clone()));
    assert!(pid_alive(running.pid), "the server runs");
    assert!(pid_alive(supervisor), "its supervisor was left alone");
}

/// The data a server of `home` is started on, as its command line names it.
fn running_data(home: &Home) -> String {
    format!("surrealkv://{}", home.dir().join("data").display())
}

/// An address file naming a server that answers but is not this board's (another SurrealDB, on other
/// data) is not trusted: this board's own server is started and recorded, and the other left alone.
#[test]
fn a_recorded_server_that_serves_other_data_is_not_this_board() {
    let s = Scratch::new("foreign");
    let other = Scratch::new("foreign-other");
    let other_home = Home::new(&other.dir, &healthy_server(&other.dir));
    let foreign = other_home.ensure().unwrap();
    s.record(&foreign);
    let ours = Home::new(&s.dir, &healthy_server(&s.dir)).ensure().unwrap();
    assert_ne!(ours.addr, foreign.addr);
    assert!(command_of(ours.pid).unwrap().contains(&s.data()));
    assert_eq!(s.home().recorded(), Some(ours));
    assert!(pid_alive(foreign.pid), "the other server was left alone");
    other_home.stop().unwrap();
}

#[test]
fn a_recorded_process_that_is_not_the_boards_server_is_never_signalled() {
    let s = Scratch::new("stranger");
    let home = Home::new(&s.dir, &healthy_server(&s.dir));
    let mut stranger = Command::of("sleep").arg("60").spawn().unwrap();
    let pid = i64::from(stranger.id());
    for version in [SURREAL_VERSION, "0.0.1"] {
        s.record(&Running {
            addr: "127.0.0.1:1".into(),
            pid,
            version: version.into(),
        });
        let started = home.ensure().unwrap();
        assert_ne!(started.pid, pid);
        assert!(stranger.try_wait().unwrap().is_none(), "the stranger runs");
        home.stop().unwrap();
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
        .patience(PATIENCE)
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
        .patience(PATIENCE)
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
    let home = Home::new(&s.dir, &surreal).patience(PATIENCE);
    let (running, taken) = std::thread::scope(|scope| {
        let started = scope.spawn(|| home.ensure());
        test_time::until_true(
            "the first start said its port, or the start ended",
            PATIENCE,
            || exists(&bind) || started.is_finished(),
        )
        .unwrap();
        assert!(exists(&bind), "the first start never said its port");
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
        let mut child = Command::of(&surreal)
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
        Home::new(&s.dir, &self.surreal).patience(PATIENCE)
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
    // The pinned binary's place says the command that fetches it there, which any project can run.
    let common = s.dir.join("repo").join(".git");
    std::fs::create_dir_all(&common).unwrap();
    let error = Home::in_common(&common, None).ensure().unwrap_err();
    let pinned = common
        .join("symphony/surreal")
        .join(SURREAL_VERSION)
        .join("surreal");
    assert_eq!(error, ServerError::NoSurreal(pinned.clone()));
    let fetch = symphony_board::server::surreal_fetch(
        &pinned,
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
    .expect("a machine the tests run on has a pinned build");
    assert_eq!(
        error.to_string(),
        format!(
            "SurrealDB {SURREAL_VERSION} is not at {}: fetch it there (`{fetch}`), or name another \
             with `SYMPHONY_SURREAL`",
            pinned.display()
        )
    );
    assert!(!error.to_string().contains("tools/surreal.sh"));
}

#[test]
fn a_store_with_no_surreal_to_serve_it_is_refused_as_unserved_not_as_no_board() {
    let s = Scratch::new("unserved");
    let data = s.dir.join("data");
    std::fs::create_dir_all(&data).unwrap();
    let missing = s.dir.join("no-such").join("surreal");
    let error = Home::new(&s.dir, &missing).ensure().unwrap_err();
    assert_eq!(
        error,
        ServerError::Unserved {
            surreal: missing.clone(),
            data: data.clone()
        }
    );
    assert_eq!(
        error.to_string(),
        format!(
            "the board's store at {} cannot be served: SurrealDB {SURREAL_VERSION}, which serves \
             it, is not at {}, where `SYMPHONY_SURREAL` names it",
            data.display(),
            missing.display()
        )
    );
    assert!(!symphony_board::board_health::absent(&error.to_string()));
    // A store that is a link to nothing is still the board's, never no board.
    std::fs::remove_dir(&data).unwrap();
    std::os::unix::fs::symlink(s.dir.join("gone"), &data).unwrap();
    assert!(matches!(
        Home::new(&s.dir, &missing).ensure().unwrap_err(),
        ServerError::Unserved { .. }
    ));
    // The pinned binary's place says how to fetch it, as with no store.
    let common = s.dir.join("repo").join(".git");
    std::fs::create_dir_all(common.join(FOLDER).join("data")).unwrap();
    let error = Home::in_common(&common, None).ensure().unwrap_err();
    let pinned = common
        .join(symphony_board::folders::SURREAL)
        .join(SURREAL_VERSION)
        .join("surreal");
    let fetch = symphony_board::server::surreal_fetch(
        &pinned,
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
    .expect("a machine the tests run on has a pinned build");
    assert!(
        error.to_string().ends_with(&format!(
            ": fetch it there (`{fetch}`), or name another with `SYMPHONY_SURREAL`"
        )),
        "{error}"
    );
    assert!(!exists(&s.dir.join("server.json")));
}

#[test]
fn a_reclaim_with_no_surreal_names_the_store_it_cannot_serve() {
    let s = Scratch::new("unserved-reclaim");
    let missing = s.dir.join("no-such").join("surreal");
    assert_eq!(
        Home::new(&s.dir, &missing).reclaim().unwrap_err(),
        ServerError::NoSurreal(missing.clone())
    );
    std::fs::create_dir_all(s.dir.join("data")).unwrap();
    assert_eq!(
        Home::new(&s.dir, &missing).reclaim().unwrap_err(),
        ServerError::Unserved {
            surreal: missing,
            data: s.dir.join("data")
        }
    );
}

#[test]
fn a_garbled_address_file_is_no_server() {
    let s = Scratch::new("garbled");
    std::fs::write(s.dir.join("server.json"), "{not json").unwrap();
    assert_eq!(s.home().recorded(), None);
    let running = Home::new(&s.dir, &healthy_server(&s.dir)).ensure().unwrap();
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
            .join("symphony/surreal")
            .join(SURREAL_VERSION)
            .join("surreal")
    );
    let named = Path::new("/opt/surreal");
    assert_eq!(Home::in_common(common, Some(named)).surreal(), named);
}

/// A board in a repository's common dir makes its folders there, `symphony/` and its own, and
/// never the common dir or a folder above it: a project's folder removed under a start, a stop or a
/// hold on its way is not made again, and each is refused as a project with no board.
#[test]
fn a_board_whose_common_dir_is_gone_makes_none_of_its_folders_again() {
    let s = Scratch::new("common-gone");
    let project = s.dir.join("project");
    let common = project.join(".git");
    std::fs::create_dir_all(&common).unwrap();
    let home = Home::in_common(&common, Some(s.surreal()));
    assert_eq!(home.stop().unwrap(), None);
    assert!(home.dir().is_dir(), "its folders made in the common dir");

    std::fs::remove_dir_all(&project).unwrap();
    for (what, refused) in [
        ("a start", home.ensure().err()),
        ("a stop", home.stop().err()),
        ("a hold", home.hold("x", "its project went").err()),
    ] {
        let why = refused
            .unwrap_or_else(|| panic!("{what} went on"))
            .to_string();
        assert!(symphony_board::board_health::absent(&why), "{what}: {why}");
        assert!(!exists(&project), "{what} made the project's folder again");
    }
}

/// SurrealDB makes every missing folder of its store's path, so a board's folder removed while its
/// server starts comes back: here a stand-in removes it as SurrealDB begins, before that opens its
/// store. The start gives up on that folder: its server is ended before it is recorded, no server
/// runs on the data, and the start is refused as a project with no board.
#[test]
fn a_start_whose_folder_goes_while_its_server_starts_ends_that_server_and_records_none() {
    let s = Scratch::new("folder-goes");
    let board = s.dir.join("board");
    let removing = stand_in(
        &s,
        &format!(
            "rm -rf '{}'\nexec '{}' \"$@\"",
            board.display(),
            s.surreal().display()
        ),
    );
    let home = Home::new(&board, &removing).patience(PATIENCE);
    let why = home.ensure().unwrap_err().to_string();
    assert_eq!(starts(&s), 1);
    assert!(symphony_board::board_health::absent(&why), "{why}");
    assert!(
        exists(&board.join("data")),
        "the stand-in's SurrealDB made its store again"
    );
    assert_eq!(home.recorded(), None);
    assert_eq!(home.serving(), None, "a server runs on the data");
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
    let init = sys::output(Command::of("git").args(["init", "-q"]).current_dir(&dir)).unwrap();
    assert!(init.status.success(), "{init:?}");
    let common = dir.join(".git").canonicalize().unwrap();
    let common_dir = |vars: &[(&str, &Path)]| {
        let mut bash = Command::of("bash");
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
        let dir = Path::new(&dir);
        let home = Home::new(dir, &surreal());
        // A board outside a scratch place starts its server detached. A checkout kept under the
        // temporary folder has its target folder, and so this board, in one: there the detached
        // start is asked for.
        let home = if scratch(dir) {
            home.supervised(false)
        } else {
            home
        };
        // A starter that blocks SIGTERM, as a signal-masking parent may start one: its server is
        // started with nothing blocked, else a stop's SIGTERM would wait in its mask for ever.
        if std::env::var_os("SYMPHONY_TEST_SERVER_MASKED").is_some() {
            // SAFETY: the set is initialised by sigemptyset before it is read; the call changes
            // only this thread's mask, the one a spawn from it hands its child.
            unsafe {
                let mut blocked: libc::sigset_t = std::mem::zeroed();
                libc::sigemptyset(&raw mut blocked);
                libc::sigaddset(&raw mut blocked, libc::SIGTERM);
                assert_eq!(
                    libc::pthread_sigmask(
                        libc::SIG_BLOCK,
                        &raw const blocked,
                        std::ptr::null_mut()
                    ),
                    0
                );
            }
        }
        home.ensure().unwrap();
    }
}

/// The signals `pid` blocks, as `ps` says them: a set in hex, signal N its bit N - 1.
fn blocked_by(pid: i64) -> u64 {
    let out =
        sys::output(Command::of("ps").args(["-o", "blocked=", "-p", &pid.to_string()])).unwrap();
    let said = String::from_utf8_lossy(&out.stdout);
    u64::from_str_radix(said.trim().trim_start_matches("0x"), 16).unwrap()
}

/// A server started by a starter that blocks SIGTERM blocks nothing: it ends when it is asked to,
/// cleanly. It has no terminal and its input is `/dev/null`.
#[test]
fn a_server_started_by_a_masked_starter_is_asked_to_end_and_ends() {
    let s = Scratch::new("masked");
    let out = symphony_board::sys::output(
        Command::of(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "home::helper_start_and_exit",
                "--test-threads=1",
            ])
            .env("SYMPHONY_TEST_SERVER_DIR", &s.dir)
            .env("SYMPHONY_TEST_SERVER_MASKED", "1")
            .env("SYMPHONY_SURREAL", s.surreal()),
    )
    .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let running = s.home().recorded().unwrap();
    let blocked = blocked_by(running.pid);
    assert_eq!(
        blocked & (1 << (libc::SIGTERM - 1)),
        0,
        "SIGTERM blocked: {blocked:#x}"
    );
    let tty = sys::output(Command::of("ps").args(["-o", "tty=", "-p", &running.pid.to_string()]))
        .unwrap();
    let tty = String::from_utf8_lossy(&tty.stdout);
    assert!(matches!(tty.trim(), "??" | "?"), "a terminal: {tty}");
    let input = sys::output(Command::of("lsof").args([
        "-a",
        "-p",
        &running.pid.to_string(),
        "-d",
        "0",
        "-Fn",
    ]))
    .unwrap();
    assert!(
        String::from_utf8_lossy(&input.stdout).contains("n/dev/null"),
        "{}",
        String::from_utf8_lossy(&input.stdout)
    );
    // Stopped is ended within its closing: a server that did not end in that time is an error.
    assert_eq!(
        s.home().closing(CLOSING).stop().unwrap(),
        Some(running.clone())
    );
    assert!(!pid_alive(running.pid));
}

#[test]
fn a_server_outlives_the_process_that_started_it_and_holds_none_of_its_pipes() {
    let s = Scratch::new("outlive");
    // `output` returns only once nothing holds the child's stdout and stderr open.
    let out = symphony_board::sys::output(
        Command::of(std::env::current_exe().unwrap())
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
