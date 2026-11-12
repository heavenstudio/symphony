//! A run ends within its time, and no run starts while a check of another runs: a killed run's
//! check still running holds the next run off and the next run records it, what a check leaves
//! outside its group holds nothing, a lock held with no run to say so is stuck, a process left in
//! the checkout or the copies' folder stops the run before it checks anything and one a check
//! leaves there stops it after that check, a checkout changed under a check is never released, a
//! run told to stop (HUP, INT, TERM) stops its check and lets the next go, a check past the run's
//! time is stopped and red, a remote that fails or hangs stops the run, a git lock an interrupted
//! run left is cleared and one held stops the run. Each run that stops short is the board's red
//! `tests/nightly-red-run`.

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use symphony_board::sys::{Child, Command, Stdio, pid_alive};

use super::{CHECK, Fixture, NOW, git};

/// A process the test started or outlived, killed when the test ends, however it ends, unless the
/// test saw it end first.
pub(crate) struct Process(pub(crate) libc::pid_t);

impl Process {
    /// It ended, as the test saw: its id is not signalled after, when another process may have it.
    pub(crate) fn ended(self) {
        std::mem::forget(self);
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        // SAFETY: kill signals the one process the test left running; a pid gone already is ESRCH.
        unsafe {
            libc::kill(self.0, libc::SIGKILL);
        }
    }
}

/// `child`'s exit code once it has ended, within `seconds`.
fn ended_within(child: &mut Child, seconds: u64) -> Option<i32> {
    let until = Instant::now() + Duration::from_secs(seconds);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status.code();
        }
        assert!(Instant::now() < until, "the nightly did not end");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The pid `file` holds once a process wrote it, within a minute.
pub(crate) fn written_pid(file: &Path) -> i64 {
    let until = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(pid) =
            (fs::read_to_string(file).ok()).and_then(|text| text.trim().parse::<i64>().ok())
        {
            return pid;
        }
        assert!(Instant::now() < until, "{} never written", file.display());
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// A process group the test outlived its nightly with, killed when the test ends, however it ends.
struct Group(libc::pid_t, bool);

impl Group {
    /// Kill it, once: its id is not signalled again after, when another group may have it.
    fn kill(&mut self) {
        if std::mem::replace(&mut self.1, true) {
            return;
        }
        // SAFETY: killpg signals the group the check leads, which the test started through the
        // nightly and which outlived it; nothing else is in it.
        unsafe {
            libc::killpg(self.0, libc::SIGKILL);
        }
    }
}

impl Drop for Group {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Whether the process `pid` has ended within `seconds`.
pub(crate) fn ends_within(pid: i64, seconds: u64) -> bool {
    let until = Instant::now() + Duration::from_secs(seconds);
    while pid_alive(pid) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(50));
    }
    !pid_alive(pid)
}

#[test]
fn a_killed_night_keeps_the_lock_until_its_check_ends() {
    let fixture = Fixture::new();
    let pid_file = fixture.root.join("check.pid");
    let check = format!("verify=echo $$ > '{}'; sleep 120", pid_file.display());
    let mut first = fixture
        .command(NOW, &["--check", &check, "--timeout", "600"], &[])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let check_pid = written_pid(&pid_file);
    let mut group = Group(libc::pid_t::try_from(check_pid).unwrap(), false);
    first.kill().unwrap();
    first.wait().unwrap();
    assert!(pid_alive(check_pid), "the check runs on");
    let said: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(fixture.records().join("running.json")).unwrap())
            .unwrap();
    let killed = said["id"].as_str().unwrap().to_string();

    // While it does, the next run is refused, naming the run and the check's group.
    let (code, out, err) = fixture.nightly_at(NOW + 60.0, &["--check", CHECK]);
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        err.contains("another nightly or release of this repository is running (run "),
        "{err}"
    );
    assert!(
        err.contains(&format!(
            "it ended, but its check `verify` runs on in process group {check_pid}"
        )),
        "{err}"
    );
    assert!(fixture.remote_tags().is_empty());
    assert!(
        fixture.reds().is_empty(),
        "a run within its time is not stuck"
    );

    // Long past the killed run's time, a run is still refused, and the board hears of the group.
    let late = NOW + 600.0 + 16.0 * 60.0;
    let (code, out, err) = fixture.nightly_at(late, &["--check", CHECK]);
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        err.contains(&format!(
            "still runs in process group {check_pid}, though that run ended: nothing runs until \
             it ends"
        )),
        "{err}"
    );
    assert!(
        out.contains("nightly: filed tests/nightly-red-run"),
        "{out}"
    );
    assert!(pid_alive(check_pid), "nothing of the run's was killed");

    // The check ends: the next run records the killed one, goes, and closes that red.
    group.kill();
    assert!(group_ends_within(check_pid, 20), "the check's group ended");
    let out = fixture.night(late + 60.0, 0);
    assert!(
        out.contains(&format!(
            "nightly: recorded the run {killed} (started 2026-11-12 10:00) as interrupted: it \
             ended without recording (killed, or the machine stopped) while `verify` ran; its \
             process {} was gone when the run ",
            first.id()
        )),
        "{out}"
    );
    assert!(
        out.contains("nightly: closed tests/nightly-red-run"),
        "{out}"
    );
    assert_eq!(fixture.remote_tags(), ["release-2026-11-12"]);
    let runs = fixture.runs();
    let found = runs
        .iter()
        .find(|run| run["id"] == killed.as_str())
        .unwrap();
    assert_eq!(found["result"], "interrupted", "{found}");
    assert_eq!(found["started_at"], NOW, "{found}");
    assert_eq!(found["commit"], runs.last().unwrap()["commit"], "{found}");
    let log = fixture
        .records()
        .join("logs")
        .join(&killed)
        .join("verify.log");
    assert!(log.exists(), "its log is kept: {}", log.display());
    let status = fixture.run(late + 120.0, &["--status"], 0);
    assert!(status.contains("last night: green, "), "{status}");
    assert!(
        status.contains("last interrupted run: interrupted, 2026-11-12 10:00 (28 min ago), on "),
        "{status}"
    );
}

/// Whether every process of the group `group` has ended within `seconds`.
fn group_ends_within(group: i64, seconds: u64) -> bool {
    let group = libc::pid_t::try_from(group).unwrap();
    // SAFETY: signal 0 sends nothing: killpg only says whether the group has a process.
    let lives = || unsafe { libc::killpg(group, 0) } == 0;
    let until = Instant::now() + Duration::from_secs(seconds);
    while lives() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(50));
    }
    !lives()
}

#[test]
fn a_check_past_the_runs_time_is_stopped_and_red_and_nothing_after_it_starts() {
    let fixture = Fixture::new();
    let pid_file = fixture.root.join("helper.pid");
    let slow = format!("slow=sleep 120 & echo $! > '{}'; wait", pid_file.display());
    let started = Instant::now();
    let (code, out, err) = fixture.nightly_at(
        NOW,
        &["--check", &slow, "--check", "after=true", "--timeout", "12"],
    );
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        started.elapsed() < Duration::from_secs(60),
        "{:?}",
        started.elapsed()
    );
    assert!(
        out.contains("nightly: slow red (timed out: stopped after "),
        "{out}"
    );
    // What the time left out is red, never passed over.
    assert!(
        out.contains("nightly: after red (did not run: the run's time was up before it started"),
        "{out}"
    );
    assert!(
        ends_within(written_pid(&pid_file), 20),
        "what the check started went with it"
    );
    assert!(fixture.remote_tags().is_empty());
    let reds = fixture.reds();
    assert_eq!(
        reds.keys().collect::<Vec<_>>(),
        ["tests/nightly-red-after", "tests/nightly-red-slow"]
    );
    assert!(
        reds["tests/nightly-red-slow"]
            .body
            .contains("(stopped after "),
        "the log says so: {}",
        reds["tests/nightly-red-slow"].body
    );
}

#[test]
fn a_remote_that_fails_or_hangs_stops_the_run_and_the_board_hears_of_it() {
    let fixture = Fixture::new();
    // The tests' clock pushes only to a folder here: these remotes are not, and push nothing.
    let (code, out, err) =
        fixture.nightly_at(NOW, &["--check", CHECK, "--remote", "nowhere", "--no-push"]);
    assert_eq!(code, 1, "{out}{err}");
    assert!(err.contains("git fetch --quiet --no-tags nowhere"), "{err}");
    let run = fixture.runs().pop().unwrap();
    assert_eq!(run["result"], "error", "{run}");
    assert_eq!(run["commit"], "", "{run}");
    let reds = fixture.reds();
    let red = &reds["tests/nightly-red-run"];
    assert_eq!(
        red.title,
        "Nightly red: the run stopped before it had a candidate"
    );
    assert!(
        red.body.contains("git fetch --quiet --no-tags nowhere"),
        "{}",
        red.body
    );

    // A remote whose transport never answers: stopped at the run's time, with its helper.
    let bin = fixture.root.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let helper = bin.join("git-remote-hang");
    let pid_file = fixture.root.join("hang.pid");
    fs::write(
        &helper,
        format!(
            "#!/bin/sh\necho $$ > '{}'\nexec sleep 120\n",
            pid_file.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&helper, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    git(&fixture.main, &["remote", "add", "slow", "hang::nowhere"]);
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let started = Instant::now();
    let (code, out, err) = fixture.nightly_with(
        NOW + 60.0,
        &[
            "--check",
            CHECK,
            "--remote",
            "slow",
            "--timeout",
            "8",
            "--no-push",
        ],
        &[("PATH", path.as_str())],
    );
    assert_eq!(code, 1, "{out}{err}");
    assert!(started.elapsed() < Duration::from_secs(60));
    assert!(
        err.contains("git fetch --quiet --no-tags slow") && err.contains("and was stopped"),
        "{err}"
    );
    assert!(
        ends_within(written_pid(&pid_file), 20),
        "the transport went with the fetch"
    );
    assert!(
        out.contains("nightly: updated tests/nightly-red-run"),
        "{out}"
    );

    // The remote back, the next run gets through and closes it.
    let out = fixture.night(NOW + 120.0, 0);
    assert!(
        out.contains("nightly: closed tests/nightly-red-run"),
        "{out}"
    );
}

#[test]
fn a_git_lock_an_interrupted_run_left_is_cleared_and_one_held_stops_the_run() {
    let fixture = Fixture::new();
    fixture.night(NOW, 0);
    let admin = git(&fixture.worktree(), &["rev-parse", "--absolute-git-dir"]);
    let lock = Path::new(&admin).join("index.lock");
    fs::write(&lock, "").unwrap();
    fixture.land("notes.txt", Some("more"), "more");
    let out = fixture.night(NOW + 60.0, 0);
    assert!(
        out.contains(&format!(
            "nightly: removed {}, which an interrupted git left and no process holds",
            lock.display()
        )),
        "{out}"
    );
    assert_eq!(fixture.remote_tags().len(), 2);

    fs::write(&lock, "").unwrap();
    let mut holder = Command::new("sleep")
        .arg("120")
        .stdin(fs::File::open(&lock).unwrap())
        .spawn()
        .unwrap();
    fixture.land("notes.txt", Some("again"), "again");
    let (code, out, err) = fixture.nightly_at(NOW + 120.0, &["--check", CHECK]);
    holder.kill().unwrap();
    holder.wait().unwrap();
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        err.contains(&format!(
            "{} is held by process {}: a git runs in the nightly's checkout",
            lock.display(),
            holder.id()
        )),
        "{err}"
    );
    assert!(lock.exists(), "a held lock stays");
    assert!(fixture.reds().contains_key("tests/nightly-red-run"));
    assert_eq!(fixture.remote_tags().len(), 2);
}

#[test]
fn what_a_check_leaves_running_outside_its_group_holds_nothing_of_the_runs() {
    let fixture = Fixture::new();
    let pid_file = fixture.root.join("detached.pid");
    // A helper that leaves the check's group and session, as a detached `git gc --auto` can, and
    // runs on after the check, keeping every descriptor it was given but working outside the
    // checkout (one working there holds the next run off, below): the check waits only until it
    // has left.
    let check = format!(
        "verify=perl -MPOSIX -e 'POSIX::setsid() or die; chdir \"/\" or die; \
         open(my $f, \">\", $ARGV[0]) or die; print $f \"$$\\n\"; close $f; sleep 120' '{pid}' & \
         while [ ! -s '{pid}' ]; do sleep 0.1; done",
        pid = pid_file.display()
    );
    fixture.run(NOW, &["--check", &check], 0);
    let detached = written_pid(&pid_file);
    let _detached = Process(libc::pid_t::try_from(detached).unwrap());
    assert!(pid_alive(detached), "it runs on after the run");
    assert!(
        !fixture.records().join("running.json").exists(),
        "no run says it holds the lock"
    );

    // The next run goes: the helper holds no lock of the run's.
    fixture.land("notes.txt", Some("more"), "more");
    fixture.night(NOW + 60.0, 0);
    assert_eq!(
        fixture.remote_tags(),
        ["release-2026-11-12", "release-2026-11-12.2"]
    );
    assert!(pid_alive(detached), "and the run left it alone");
}

#[test]
fn a_lock_held_by_no_run_or_past_its_time_is_stuck_and_the_board_hears_who_holds_it() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.records()).unwrap();
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(fixture.records().join("run.lock"))
        .unwrap();
    lock.lock().unwrap();
    // Held by a detached process alone, as a helper that kept an older build's lock would hold it.
    let mut holder = Command::new("sleep")
        .arg("120")
        .stdin(lock)
        .new_session()
        .spawn()
        .unwrap();
    let held = Process(libc::pid_t::try_from(holder.id()).unwrap());
    let (code, out, err) = fixture.nightly_at(NOW, &["--check", CHECK]);
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        err.contains(&format!(
            "run.lock is held, but no run says it holds it: it is held by process {}, which no \
             run of this repository is",
            holder.id()
        )),
        "{err}"
    );
    assert!(
        out.contains("nightly: filed tests/nightly-red-run"),
        "{out}"
    );
    assert_eq!(fixture.runs().pop().unwrap()["result"], "error");
    assert!(fixture.remote_tags().is_empty());

    // Said to be a run's, the same holder is that run's while its time lasts: a plain refusal.
    let running = serde_json::json!({
        "id": "another",
        "pid": holder.id(),
        "started_at": NOW,
        "until": NOW + 600.0,
    });
    fs::write(fixture.records().join("running.json"), running.to_string()).unwrap();
    let (code, out, err) = fixture.nightly_at(NOW + 60.0, &["--check", CHECK]);
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        err.contains(&format!(
            "another nightly or release of this repository is running (run another, process {}, \
             since 2026-11-12 10:00)",
            holder.id()
        )),
        "{err}"
    );
    assert!(out.is_empty(), "a run within its time is not stuck: {out}");
    assert_eq!(fixture.runs().len(), 1, "a plain refusal records nothing");

    // Held more than 15 minutes past that run's time, it is stuck.
    let (code, out, err) = fixture.nightly_at(NOW + 1560.0, &["--check", CHECK]);
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        err.contains(&format!(
            "run.lock is still held past the time of the run another (process {pid}, started \
             2026-11-12 10:00, its time up at 2026-11-12 10:10), by process {pid}: that run may \
             be stuck",
            pid = holder.id()
        )),
        "{err}"
    );
    assert!(
        out.contains("nightly: updated tests/nightly-red-run"),
        "{out}"
    );
    assert_eq!(fixture.runs().len(), 2);
    assert!(fixture.remote_tags().is_empty());

    // Once it ends, the next run goes, and closes the red.
    holder.kill().unwrap();
    holder.wait().unwrap();
    held.ended();
    let out = fixture.night(NOW + 1620.0, 0);
    assert!(
        out.contains("nightly: closed tests/nightly-red-run"),
        "{out}"
    );
}

#[test]
fn a_run_that_has_just_taken_the_lock_is_waited_for_over_what_a_dead_run_left() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.records()).unwrap();
    let lock = fs::File::create(fixture.records().join("run.lock")).unwrap();
    lock.lock().unwrap();
    // What a run that ended without releasing it left: its process is gone.
    let mut gone = Command::new("true").spawn().unwrap();
    gone.wait().unwrap();
    let stale = serde_json::json!({
        "id": "dead", "pid": gone.id(), "started_at": NOW - 7200.0, "until": NOW - 3600.0,
    });
    fs::write(fixture.records().join("running.json"), stale.to_string()).unwrap();
    // The run that holds the lock says so a moment later, as one that has just taken it does.
    let said = fixture.records().join("running.json");
    let saying = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(700));
        let fresh = serde_json::json!({
            "id": "fresh", "pid": std::process::id(), "started_at": NOW, "until": NOW + 3600.0,
        });
        fs::write(said, fresh.to_string()).unwrap();
    });
    let (code, out, err) = fixture.nightly_at(NOW + 60.0, &["--check", CHECK]);
    saying.join().unwrap();
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        err.contains(&format!(
            "another nightly or release of this repository is running (run fresh, process {}, \
             since 2026-11-12 10:00)",
            std::process::id()
        )),
        "{err}"
    );
    assert!(
        out.is_empty(),
        "a run that has just taken the lock is not stuck: {out}"
    );
    assert!(fixture.reds().is_empty());

    // Held while what a killed run left says its check runs on: the run holding the lock is the
    // one reporting that check, and this one is refused as that check's run is.
    let mut check = Command::new("sleep")
        .arg("120")
        .process_group(0)
        .spawn()
        .unwrap();
    let guard = Process(libc::pid_t::try_from(check.id()).unwrap());
    let killed = serde_json::json!({
        "id": "killed", "pid": gone.id(), "started_at": NOW - 7200.0, "until": NOW - 3600.0,
        "check": {"name": "verify", "group": check.id()},
    });
    fs::write(fixture.records().join("running.json"), killed.to_string()).unwrap();
    let (code, out, err) = fixture.nightly_at(NOW + 120.0, &["--check", CHECK]);
    check.kill().unwrap();
    check.wait().unwrap();
    guard.ended();
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        err.contains(&format!(
            "another nightly or release of this repository is running (run killed, process {}, \
             since 2026-11-12 08:00): it ended, but its check `verify` runs on in process group {}",
            gone.id(),
            check.id()
        )),
        "{err}"
    );
    assert!(out.is_empty(), "{out}");
    assert!(fixture.reds().is_empty());
    drop(lock);
}

#[test]
fn a_process_left_in_the_checkout_stops_the_run_before_it_checks_anything() {
    let fixture = Fixture::new();
    fixture.night(NOW, 0);
    fixture.land("notes.txt", Some("more"), "more");
    let checkout = fixture.worktree();

    // One working there, as a shell left in it or a helper a check started there would.
    let mut working = Command::new("sleep")
        .arg("120")
        .current_dir(&checkout)
        .spawn()
        .unwrap();
    let guard = Process(libc::pid_t::try_from(working.id()).unwrap());
    let (code, out, err) = fixture.nightly_at(NOW + 60.0, &["--check", CHECK]);
    working.kill().unwrap();
    working.wait().unwrap();
    guard.ended();
    assert_eq!(code, 1, "{out}{err}");
    let way_out = "end it, or move it out of that folder (a shell, a terminal tab, an editor or a \
                   Finder window there counts), and the next run goes";
    let said = format!(
        "{} is in use by process {} (sleep), working there: nothing is checked while a process is \
         in the run's folders; {way_out}",
        checkout.display(),
        working.id()
    );
    assert!(err.contains(&said), "{err}");
    assert!(
        !out.contains("nightly: verify"),
        "nothing was checked: {out}"
    );
    assert!(
        out.contains("nightly: filed tests/nightly-red-run"),
        "{out}"
    );
    let filed = &fixture.reds()["tests/nightly-red-run"].body;
    assert!(filed.contains(&said), "{filed}");
    assert!(
        filed.contains(
            "Who fixes: the owner, or an orchestrator on the owner's word; not an \
                        executor."
        ),
        "the process may be anyone's, so the red is the owner's: {filed}"
    );
    assert!(!filed.contains("any free executor"), "{filed}");
    assert!(
        !checkout.join("notes.txt").exists(),
        "the checkout was left as it was"
    );

    // Gone, the next run goes and closes the red.
    let out = fixture.night(NOW + 120.0, 0);
    assert!(
        out.contains("nightly: closed tests/nightly-red-run"),
        "{out}"
    );
    assert_eq!(
        fixture.remote_tags(),
        ["release-2026-11-12", "release-2026-11-12.2"]
    );
}

#[test]
fn a_process_holding_a_file_inside_the_checkout_open_stops_the_run_too() {
    let fixture = Fixture::new();
    fixture.night(NOW, 0);
    fixture.land("notes.txt", Some("more"), "more");
    let checkout = fixture.worktree();
    // From outside it, as an editor or a `tail -f` started elsewhere would.
    let mut holding = Command::new("sleep")
        .arg("120")
        .stdin(fs::File::open(checkout.join("symphony.toml")).unwrap())
        .current_dir(&fixture.root)
        .spawn()
        .unwrap();
    let guard = Process(libc::pid_t::try_from(holding.id()).unwrap());
    let (code, out, err) = fixture.nightly_at(NOW + 60.0, &["--check", CHECK]);
    holding.kill().unwrap();
    holding.wait().unwrap();
    guard.ended();
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        err.contains(&format!(
            "{} is in use by process {} (sleep), holding symphony.toml open: nothing is checked",
            checkout.display(),
            holding.id()
        )),
        "{err}"
    );
    assert!(
        out.contains("nightly: filed tests/nightly-red-run"),
        "{out}"
    );
    assert_eq!(fixture.remote_tags(), ["release-2026-11-12"]);
}

#[test]
fn a_process_in_the_conciliation_copies_folder_stops_the_run_too() {
    let fixture = Fixture::new();
    // The folder the run makes each project's copy ready in, in its turn.
    let copies = fixture.root.join("wt/nightly-conciliate");
    fs::create_dir_all(copies.join("consumer")).unwrap();
    let copies = copies.canonicalize().unwrap();
    let mut copying = Command::new("sleep")
        .arg("120")
        .current_dir(copies.join("consumer"))
        .spawn()
        .unwrap();
    let guard = Process(libc::pid_t::try_from(copying.id()).unwrap());
    let (code, out, err) = fixture.nightly_at(NOW, &["--check", CHECK]);
    copying.kill().unwrap();
    copying.wait().unwrap();
    guard.ended();
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        err.contains(&format!(
            "{} is in use by process {} (sleep), working in consumer: nothing is checked while a \
             process is in the run's folders; end it, or move it out of that folder (a shell, a \
             terminal tab, an editor or a Finder window there counts), and the next run goes",
            copies.display(),
            copying.id()
        )),
        "{err}"
    );
    assert!(
        !out.contains("nightly: verify"),
        "nothing was checked: {out}"
    );
    assert!(
        out.contains("nightly: filed tests/nightly-red-run"),
        "{out}"
    );
    assert!(fixture.remote_tags().is_empty());
}

#[test]
fn a_process_a_check_leaves_in_the_checkout_stops_the_run_after_that_check() {
    let fixture = Fixture::new();
    let pid_file = fixture.root.join("survivor.pid");
    // The check leaves a survivor outside its group and session, working in the checkout and
    // writing only in its build folder, which the look at what git names passes over.
    let check = format!(
        "verify=mkdir -p .target && perl -MPOSIX -e 'POSIX::setsid() or die; \
         open(my $f, \">\", $ARGV[0]) or die; print $f \"$$\\n\"; close $f; \
         open(STDOUT, \">\", \"/dev/null\"); open(STDERR, \">\", \"/dev/null\"); \
         open(my $o, \">\", \".target/out\") or die; print $o \"one\\n\"; close $o; \
         exec \"sleep\", \"120\"' '{pid}' & \
         while [ ! -s '{pid}' ]; do sleep 0.1; done",
        pid = pid_file.display()
    );
    let (code, out, err) = fixture.nightly_at(NOW, &["--check", &check]);
    let survivor = written_pid(&pid_file);
    let guard = Process(libc::pid_t::try_from(survivor).unwrap());
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        out.contains("nightly: verify green (exit 0, "),
        "the check passed: {out}"
    );
    assert!(
        err.contains(&format!(
            "after `verify` ended green (exit 0), {} is in use by process {survivor} (sleep), \
             working there: what was checked may have changed, so nothing checked is released; \
             end it, or move it out of that folder (a shell, a terminal tab, an editor or a \
             Finder window there counts), and the next run goes",
            fixture.worktree().display()
        )),
        "{err}"
    );
    assert!(fixture.remote_tags().is_empty(), "nothing is released");
    let run = fixture.runs().pop().unwrap();
    assert!(
        run["checks"].as_array().unwrap().is_empty(),
        "a check the survivor may have changed counts for nothing: {run}"
    );
    let filed = &fixture.reds()["tests/nightly-red-run"].body;
    assert!(
        filed.contains("Who fixes: the owner, or an orchestrator on the owner's word"),
        "{filed}"
    );

    // Once it is ended, the next run goes and closes the red.
    // SAFETY: kill signals the survivor this test's check started, which the test saw running.
    unsafe {
        libc::kill(libc::pid_t::try_from(survivor).unwrap(), libc::SIGKILL);
    }
    assert!(ends_within(survivor, 20));
    guard.ended();
    let out = fixture.night(NOW + 60.0, 0);
    assert!(
        out.contains("nightly: closed tests/nightly-red-run"),
        "{out}"
    );
    assert_eq!(fixture.remote_tags(), ["release-2026-11-12"]);
}

#[test]
fn a_checkout_changed_under_a_check_is_never_released_and_the_board_hears_what_changed() {
    let fixture = Fixture::new();
    fixture.land("a.txt", Some("one\n"), "a.txt reads one");
    let pid_file = fixture.root.join("writer.pid");
    let target = fixture.worktree().join("a.txt");
    // The first night's check leaves a writer outside its group and outside the checkout. It looks
    // at a.txt without opening it, and 2 s after a.txt reads `broken` (7 bytes) writes `one` over
    // it, as a survivor of an earlier check can.
    let check = format!(
        "verify=perl -MPOSIX -e 'POSIX::setsid() or die; chdir \"/\" or die; \
         open(my $p, \">\", $ARGV[0]) or die; print $p \"$$\\n\"; close $p; \
         for (1..1200) {{ if ((-s $ARGV[1] // 0) == 7) {{ sleep 2; \
         open(my $w, \">\", $ARGV[1]) or die; print $w \"one\\n\"; close $w; exit 0 }} \
         select(undef, undef, undef, 0.1) }}' '{pid}' '{target}' & \
         while [ ! -s '{pid}' ]; do sleep 0.1; done",
        pid = pid_file.display(),
        target = target.display()
    );
    fixture.run(NOW, &["--check", &check], 0);
    let writer = written_pid(&pid_file);
    let guard = Process(libc::pid_t::try_from(writer).unwrap());
    assert_eq!(fixture.remote_tags(), ["release-2026-11-12"]);

    // A broken commit lands; the next night's check would see it, but reads after the writer.
    let broken = fixture.land("a.txt", Some("broken\n"), "a.txt breaks");
    let (code, out, err) = fixture.nightly_at(
        NOW + 60.0,
        &["--check", "verify=sleep 5; grep -qx one a.txt"],
    );
    assert!(ends_within(writer, 20), "the writer wrote and left");
    guard.ended();
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        out.contains("nightly: verify green (exit 0, "),
        "the check passed on what the writer left: {out}"
    );
    assert!(
        err.contains(&format!(
            "after `verify` ended green (exit 0), {} no longer holds {}: M a.txt; a process may be \
             writing there, so nothing checked is released",
            fixture.worktree().display(),
            &broken[..12]
        )),
        "{err}"
    );
    assert_eq!(
        fixture.remote_tags(),
        ["release-2026-11-12"],
        "the broken commit is not released"
    );
    let run = fixture.runs().pop().unwrap();
    assert_eq!(run["commit"], broken.as_str(), "{run}");
    assert!(
        run["checks"].as_array().unwrap().is_empty(),
        "a check of other bytes counts for nothing: {run}"
    );
    assert_eq!(
        fixture.reds().keys().collect::<Vec<_>>(),
        ["tests/nightly-red-run"]
    );
    assert!(
        fixture.reds()["tests/nightly-red-run"]
            .body
            .contains("M a.txt"),
        "the board hears what changed"
    );

    // Fixed on main, the next night is green and closes the red.
    fixture.land("a.txt", Some("one\n"), "a.txt reads one again");
    let out = fixture.run(NOW + 120.0, &["--check", "verify=grep -qx one a.txt"], 0);
    assert!(
        out.contains("nightly: closed tests/nightly-red-run"),
        "{out}"
    );
    assert_eq!(
        fixture.remote_tags(),
        ["release-2026-11-12", "release-2026-11-12.2"]
    );
}

#[test]
fn a_run_told_to_stop_stops_its_check_releases_the_lock_and_says_so() {
    let fixture = Fixture::new();
    let signals = [
        (libc::SIGTERM, "TERM"),
        (libc::SIGINT, "INT"),
        (libc::SIGHUP, "HUP"),
    ];
    for (at, (signal, name)) in (0_u32..).zip(signals) {
        // As a scheduler or a terminal starts it: the signal at its default, not ignored.
        // SAFETY: signal sets this process's disposition for a valid signal; the nightly started
        // below inherits it.
        unsafe {
            libc::signal(signal, libc::SIG_DFL);
        }
        let pid_file = fixture.root.join(format!("check-{name}.pid"));
        let said_file = fixture.root.join(format!("said-{name}.txt"));
        let check = format!("verify=echo $$ > '{}'; sleep 120", pid_file.display());
        let mut night = fixture
            .command(
                NOW + 60.0 * f64::from(at),
                &["--check", &check, "--timeout", "600"],
                &[],
            )
            .stdout(fs::File::create(&said_file).unwrap())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let check_pid = written_pid(&pid_file);
        let running = Process(libc::pid_t::try_from(check_pid).unwrap());
        // SAFETY: kill sends the signal to the nightly this test started and has not reaped.
        unsafe {
            libc::kill(libc::pid_t::try_from(night.id()).unwrap(), signal);
        }
        assert_eq!(ended_within(&mut night, 60), Some(1), "{name}");
        assert!(ends_within(check_pid, 5), "{name}: its check was stopped");
        running.ended();
        assert!(
            !fixture.records().join("running.json").exists(),
            "{name}: no run says it holds the lock"
        );
        let lock = fs::File::open(fixture.records().join("run.lock")).unwrap();
        assert!(lock.try_lock().is_ok(), "{name}: the lock is free");
        drop(lock);
        let run = fixture.runs().pop().unwrap();
        assert_eq!(run["result"], "error", "{name}: {run}");
        let said = run["error"].as_str().unwrap();
        assert!(
            said.starts_with(
                "the run was told to stop (HUP, INT or TERM) while `verify` ran, which was \
                 stopped after "
            ),
            "{name}: {said}"
        );
        assert!(run["checks"].as_array().unwrap().is_empty(), "{run}");
        let out = fs::read_to_string(&said_file).unwrap();
        assert!(
            out.contains("nightly: verify stopped (interrupted: stopped after "),
            "{name}: a check stopped is said so, neither green nor red: {out}"
        );
    }
    assert_eq!(
        fixture.reds().keys().collect::<Vec<_>>(),
        ["tests/nightly-red-run"],
        "a check stopped is no check's red: the run's"
    );
    assert!(fixture.remote_tags().is_empty());

    let out = fixture.night(NOW + 600.0, 0);
    assert!(
        out.contains("nightly: closed tests/nightly-red-run"),
        "{out}"
    );
}
