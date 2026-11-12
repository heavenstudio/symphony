//! A run ends within its time and never hands the lock on while a check of it runs: a killed run's
//! check keeps the lock, a check past the run's time is stopped and red, a remote that fails or
//! hangs stops the run, a git lock an interrupted run left is cleared and one held stops the run.
//! Each run that stops short is the board's red `tests/nightly-red-run`.

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use symphony_board::sys::{Command, Stdio, pid_alive};

use super::{CHECK, Fixture, NOW, git};

/// The pid `file` holds once a process wrote it, within a minute.
fn written_pid(file: &Path) -> i64 {
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
fn ends_within(pid: i64, seconds: u64) -> bool {
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

    // While it does, the next run is refused, naming the run that holds the lock.
    let (code, out, err) = fixture.nightly_at(NOW + 60.0, &["--check", CHECK]);
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        err.contains("another nightly or release of this repository is running (run "),
        "{err}"
    );
    assert!(fixture.remote_tags().is_empty());
    assert!(
        fixture.reds().is_empty(),
        "a run within its time is not stuck"
    );

    // Long past the killed run's time, a run is still refused, and the board hears who holds it.
    let late = NOW + 600.0 + 16.0 * 60.0;
    let (code, out, err) = fixture.nightly_at(late, &["--check", CHECK]);
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        err.contains("is still held past the time of the run"),
        "{err}"
    );
    let holders = (err.split_once(", by process ").map(|(_, rest)| rest))
        .and_then(|rest| rest.split_once(':'))
        .map(|(pids, _)| pids.split(", ").map(str::to_string).collect::<Vec<_>>())
        .unwrap_or_default();
    assert!(holders.contains(&check_pid.to_string()), "{err}");
    assert!(
        out.contains("nightly: filed tests/nightly-red-run"),
        "{out}"
    );
    assert!(pid_alive(check_pid), "nothing of the run's was killed");

    // The check ends: the next run goes, and closes that red.
    group.kill();
    assert!(ends_within(check_pid, 20));
    let out = fixture.night(late + 60.0, 0);
    assert!(
        out.contains("nightly: closed tests/nightly-red-run"),
        "{out}"
    );
    assert_eq!(fixture.remote_tags(), ["release-2026-11-12"]);
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
    let (code, out, err) = fixture.nightly_at(NOW, &["--check", CHECK, "--remote", "nowhere"]);
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
        &["--check", CHECK, "--remote", "slow", "--timeout", "8"],
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
