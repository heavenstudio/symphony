//! The driver's own cases: its leash, cleanup, waits and protocol bounds, against stand-in browsers
//! and transports, and against the real Chrome when this machine has one (`SYMPHONY_CHROME` names
//! it).

use std::io::ErrorKind;
use std::net::TcpStream;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

use crate::profiles::{self, next_profile};
use crate::{
    BACKSTOP_FLOOR, BrowserGroup, CLEANUP_CPU, Chrome, End, LAUNCH_BACKSTOP, Leash, Owner, POLL,
    PROTOCOL_CPU, ROOTED, WaitBudget, chrome_args, cleanup_profile, devtools_endpoint, find,
    listed, load_said, process, signal_group,
};

/// This test binary as the leash: it runs [`browser_leash`] alone.
pub(crate) fn leash() -> Leash {
    Leash::new(
        std::env::current_exe().unwrap(),
        &["--exact", "tests::browser_leash", "--nocapture"],
    )
}

/// A process id as the kernel's calls take one.
fn pid(pid: u32) -> libc::pid_t {
    libc::pid_t::try_from(pid).unwrap()
}

#[test]
fn a_zero_allowance_keeps_initial_readiness_and_bounds_both_active_and_idle_replies() {
    let member = |cpu| process::Member {
        pid: 1,
        parent: 0,
        state: 1,
        cpu,
    };
    let mut budget = WaitBudget::new(Duration::ZERO, &[member(Duration::ZERO)]);
    assert!(!budget.exhausted(&[member(Duration::ZERO)]));
    assert!(!budget.exhausted(&[member(Duration::from_millis(1))]));
    assert!(budget.exhausted(&[member(Duration::from_secs(60))]));
    let mut idle = WaitBudget::new(Duration::ZERO, &[]);
    assert!(idle.poll(&[]));
}

#[test]
fn a_page_wait_counts_only_the_observer_turns_its_page_answered_a_probe_in() {
    // Four turns' allowance: 100 ms of 25 ms turns.
    let mut budget = WaitBudget::new(Duration::from_millis(100), &[]).on_page();
    // Frozen: however many observer turns pass, none counts and the allowance is untouched.
    assert!(budget.probe_due(), "a page that has not answered is probed");
    budget.probed(7);
    for _ in 0..1000 {
        assert!(!budget.turn(&[]));
        assert!(!budget.probe_due(), "one probe in flight is enough");
    }
    assert_eq!(budget.polls, 0);
    assert_eq!(budget.page.as_ref().unwrap().passed, 1000);
    // Only the page's own probe answers for it.
    assert!(!budget.answered(&json!(8)));
    assert!(!budget.answered(&json!("7")));
    assert!(!budget.answered(&Value::Null));
    assert!(budget.answered(&json!(7)));
    assert!(
        !budget.probe_due(),
        "an answered page is probed again only after a turn"
    );
    // Running: one turn per answer, and the allowance runs out on the fourth.
    assert!(!budget.turn(&[]));
    assert_eq!(budget.polls, 1);
    assert!(!budget.turn(&[]), "a second turn without an answer passes");
    assert_eq!(
        (budget.polls, budget.page.as_ref().unwrap().passed),
        (1, 1001)
    );
    for (probe, exhausted) in [(9, false), (10, false), (11, true)] {
        assert!(budget.probe_due());
        budget.probed(probe);
        assert!(budget.answered(&json!(probe)));
        assert_eq!(budget.turn(&[]), exhausted);
    }
    assert_eq!(budget.polls, 4);
    // A wait on no page counts every observer turn and never probes.
    let mut browser = WaitBudget::new(Duration::from_millis(50), &[]);
    assert!(!browser.probe_due());
    assert!(!browser.answered(&json!(1)));
    assert!(!browser.turn(&[]));
    assert!(browser.turn(&[]));
}

#[test]
fn a_probe_unanswered_for_a_second_goes_again_and_any_of_them_answers() {
    let mut budget = WaitBudget::new(Duration::from_secs(1), &[]).on_page();
    budget.probed(3);
    assert!(!budget.probe_due());
    budget.page.as_mut().unwrap().asked -= crate::PROBE_AGAIN;
    assert!(
        budget.probe_due(),
        "a probe lost to a navigation is sent again"
    );
    budget.probed(4);
    assert!(!budget.probe_due());
    // The first probe's late reply still counts, and answers for both.
    assert!(budget.answered(&json!(3)));
    assert!(!budget.answered(&json!(4)));
    assert!(budget.page.as_ref().unwrap().probes.is_empty());
}

#[test]
fn the_backstop_is_six_times_the_allowance_never_under_its_floor_and_ends_the_wait() {
    let backstop = |allowance, floor| WaitBudget::new(allowance, &[]).floor(floor).backstop();
    assert_eq!(
        backstop(Duration::from_secs(30), crate::BACKSTOP_FLOOR),
        Duration::from_secs(180)
    );
    assert_eq!(
        backstop(Duration::from_secs(10), crate::BACKSTOP_FLOOR),
        Duration::from_secs(120)
    );
    assert_eq!(
        backstop(Duration::ZERO, crate::BACKSTOP_FLOOR),
        crate::BACKSTOP_FLOOR
    );
    assert_eq!(
        backstop(Duration::from_millis(100), Duration::ZERO),
        Duration::from_millis(600)
    );
    // A page that never answered: the backstop alone ends the wait, and says so.
    let mut budget = WaitBudget::new(Duration::from_millis(100), &[])
        .floor(Duration::ZERO)
        .on_page();
    for _ in 0..3 {
        assert!(!budget.turn(&[]));
    }
    assert!(!budget.past_backstop());
    budget.started -= Duration::from_millis(600);
    assert!(budget.past_backstop());
    assert!(budget.exhausted(&[]));
    assert!(!budget.past_deadline());
    let stall = budget.stall();
    assert!(
        stall.starts_with(
            "the page under test stalled: its renderer took part in 0 of 3 observer turns"
        ),
        "{stall}"
    );
    assert!(
        stall.contains("the wait's wall-clock backstop of 600ms ran out after"),
        "{stall}"
    );
    // A wait on no page that runs out on the wall says so too.
    let mut browser = WaitBudget::new(Duration::from_millis(100), &[]).floor(Duration::ZERO);
    browser.started -= Duration::from_millis(600);
    assert!(browser.exhausted(&[]));
    let browser_stall = browser.stall();
    assert!(
        browser_stall.starts_with("the browser stalled: the wait's wall-clock backstop of 600ms"),
        "{browser_stall}"
    );
    for said in [&stall, &browser_stall] {
        assert!(said.contains("; load average "), "{said}");
        assert!(said.ends_with(" over 1, 5 and 15 minutes"), "{said}");
    }
    assert!(
        browser_stall
            .split_once("; ")
            .unwrap()
            .0
            .ends_with("0 completed observer polls and 0ns CPU"),
        "{browser_stall}"
    );
}

/// The tests' own waits on a condition (`test_time::backstop`, RULES.md
/// `tests/no-test-fails-on-load`) run to the same backstop as a driver's wait on the same
/// allowance: a change to either rule fails here until the other follows.
#[test]
fn the_tests_waits_and_the_driver_share_one_backstop_rule() {
    for allowance in [
        Duration::ZERO,
        POLL,
        Duration::from_secs(10),
        Duration::from_secs(20),
        Duration::from_secs(21),
        crate::BACKSTOP_FLOOR,
        PROTOCOL_CPU,
        Duration::from_secs(3600),
    ] {
        assert_eq!(
            test_time::backstop(allowance),
            WaitBudget::new(allowance, &[]).backstop(),
            "the backstop of a wait of {allowance:?}"
        );
    }
    assert_eq!(test_time::FLOOR, BACKSTOP_FLOOR);
    // The launch's wait for DevTools keeps the rule on its CPU allowance.
    assert_eq!(LAUNCH_BACKSTOP, test_time::backstop(PROTOCOL_CPU));
    assert_eq!(
        LAUNCH_BACKSTOP,
        WaitBudget::new(PROTOCOL_CPU, &[]).backstop()
    );
    assert_eq!(LAUNCH_BACKSTOP, Duration::from_secs(360));
    // And a stall says the load in the tests' own words.
    for load in [Some([0.5, 12.25, 250.0]), None] {
        assert_eq!(load_said(load), test_time::Load(load).to_string());
    }
}

#[test]
fn a_wait_says_the_check_that_ended_it_however_late_its_error_is_made() {
    let hour = Duration::from_secs(3600);
    // Its allowance spent first (one turn of a 25 ms allowance): a deadline and a backstop that
    // pass before its error is made turn it into neither.
    let mut spent = WaitBudget::new(POLL, &[]).until(Some(Instant::now() + hour));
    assert!(spent.poll(&[]));
    assert_eq!(spent.end, Some(End::Allowance));
    spent.deadline = Some(Instant::now());
    spent.started -= Duration::from_secs(600);
    assert!(spent.past_deadline() && spent.past_backstop());
    let error = spent.ended("a wait", "a wait: its allowance spent".to_string());
    assert!(!error.past_deadline() && !error.stalled(), "{error}");
    assert_eq!(error.to_string(), "a wait: its allowance spent");
    // The deadline passed first: a deadline, whatever else ran out after it.
    let mut late = WaitBudget::new(PROTOCOL_CPU, &[]).until(Some(Instant::now()));
    assert!(late.exhausted(&[]));
    late.started -= Duration::from_secs(600);
    late.polls = late.max_polls;
    let error = late.ended("a wait", "spent".to_string());
    assert!(error.past_deadline() && !error.stalled(), "{error}");
    assert_eq!(error.to_string(), "a wait: no reply before the deadline");
    // The backstop ran out first: a stall, though the deadline has passed by the time its error
    // is made.
    let mut stalled = WaitBudget::new(PROTOCOL_CPU, &[]).until(Some(Instant::now() + hour));
    stalled.started -= Duration::from_secs(600);
    assert!(stalled.exhausted(&[]));
    stalled.deadline = Some(Instant::now());
    let error = stalled.ended("a wait", "spent".to_string());
    assert!(error.stalled() && !error.past_deadline(), "{error}");
    assert!(
        error.to_string().starts_with("a wait: the browser stalled"),
        "{error}"
    );
    // A wait still running has no end kept, and one found stays: the first check found true.
    let mut running = WaitBudget::new(PROTOCOL_CPU, &[]).until(Some(Instant::now() + hour));
    assert!(!running.exhausted(&[]));
    assert_eq!(running.end, None);
    running.polls = running.max_polls;
    assert!(running.exhausted(&[]));
    running.deadline = Some(Instant::now());
    assert!(running.exhausted(&[]));
    assert_eq!(running.end, Some(End::Allowance));
}

/// Reexecuted in a separate process, with no shared mutable environment in the suite.
#[test]
fn browser_leash() {
    let Some(code) = crate::leash_main() else {
        let args = chrome_args(Path::new("/private/profile"));
        assert!(args.contains(&"--mute-audio".into()));
        assert!(args.contains(&"--user-data-dir=/private/profile".into()));
        // The page under test runs at foreground priority however Chrome counts its tab.
        assert!(args.contains(&"--disable-renderer-backgrounding".into()));
        assert!(args.contains(&"--disable-backgrounding-occluded-windows".into()));
        // No updater, whose wake leaves the browser's group and outlives the leash.
        assert!(args.contains(&"--disable-updater-scheduler".into()));
        // Motion as the page asks, not as the machine's settings reduce it.
        assert!(args.contains(&"--force-prefers-no-reduced-motion".into()));
        // A same-site visit keeps its frame, in the one list of features turned off Chrome reads.
        let disabled: Vec<_> = args
            .iter()
            .filter_map(|arg| arg.strip_prefix("--disable-features="))
            .collect();
        assert_eq!(disabled.len(), 1, "{args:?}");
        assert!(
            disabled[0]
                .split(',')
                .any(|feature| feature == "RenderDocument")
        );
        return;
    };
    std::process::exit(code);
}

#[test]
fn profile_cleanup_rechecks_after_helpers_and_reports_other_errors() {
    use std::io::BufRead;

    let profile = next_profile().unwrap();
    std::fs::create_dir_all(profile.join("nested")).unwrap();
    std::fs::write(profile.join("nested/data"), "profile").unwrap();
    // Announce readiness before cleanup starts. Recreate the profile only after its first
    // removal, so returning on that removal instead of waiting for helpers always leaks it.
    let mut browser = BrowserGroup {
        child: Command::new("/bin/sh")
            .args([
                "-c",
                r#"printf 'ready\n'
while test -d "$1"; do /bin/sleep 0.01; done
mkdir "$1"
"#,
                "profile-writer",
            ])
            .arg(&profile)
            .stdout(Stdio::piped())
            .process_group(0)
            .spawn()
            .unwrap(),
        done: false,
    };
    let mut ready = String::new();
    std::io::BufReader::new(browser.child.stdout.take().unwrap())
        .read_line(&mut ready)
        .unwrap();
    assert_eq!(ready, "ready\n");
    let group = browser.child.id();
    let reaper = std::thread::spawn(move || browser.reap().unwrap());
    let cleanup = cleanup_profile(&profile, group);
    if cleanup.is_err() {
        // A regression must also stop this owned helper instead of stranding its waiter.
        let _ = signal_group(group, 9);
    }
    let status = reaper.join().unwrap();
    cleanup.unwrap();
    assert!(status.success());
    assert!(!profile.exists(), "cleanup must remove the whole profile");
    cleanup_profile(&profile, group).unwrap();

    // A path that is not a directory is a real fixture error, not a retryable writer race.
    std::fs::write(&profile, "not a directory").unwrap();
    let error = cleanup_profile(&profile, group).unwrap_err();
    std::fs::remove_file(&profile).unwrap();
    assert_eq!(error.kind(), ErrorKind::NotADirectory);
}

/// The browser ending on its own is left unreaped, its pid and group id its own, while its group
/// is killed, helpers and all; it is reaped after, and a second end sends nothing.
#[test]
fn a_browser_that_ended_is_reaped_only_after_the_kill_of_its_group() {
    let child = Command::new("/bin/sh")
        .args(["-c", "/bin/sleep 30 & exit 0"])
        .process_group(0)
        .spawn()
        .unwrap();
    let group = child.id();
    let mut browser = BrowserGroup { child, done: false };
    test_time::until_true(
        "the browser ends on its own",
        test_time::backstop(Duration::from_secs(30)),
        || crate::exited(group).unwrap(),
    )
    .unwrap();
    // SAFETY: signal 0 to this test's own unreaped child sends nothing.
    assert_eq!(
        unsafe { libc::kill(pid(group), 0) },
        0,
        "its pid is its own"
    );
    assert!(signal_group(group, 0).is_ok(), "its helper runs");
    assert!(browser.end().unwrap().success());
    test_time::until_true(
        "its helper ends with its group's kill",
        test_time::backstop(Duration::from_secs(10)),
        || signal_group(group, 0).is_err(),
    )
    .unwrap();
    assert!(
        browser.end().unwrap().success(),
        "a second end answers the same"
    );
}

/// A browser stopped (SIGSTOP, as a test stalls one) has not ended: the leash leaves it and its
/// group alone, and it is taken for ended only once it exits.
#[test]
fn a_stopped_browser_has_not_ended() {
    let mut child = Command::new("/bin/sleep").arg("30").spawn().unwrap();
    let id = child.id();
    // SAFETY: stops this test's own unreaped child.
    let stop = unsafe { libc::kill(pid(id), libc::SIGSTOP) };
    // SAFETY: siginfo_t is plain data, for which all zeroes is a value.
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    // SAFETY: waits for this test's own child to stop, reaping none.
    let waited = unsafe {
        libc::waitid(
            libc::P_PID,
            libc::id_t::from(id),
            &raw mut info,
            libc::WSTOPPED | libc::WNOWAIT,
        )
    };
    let stopped = crate::exited(id).unwrap();
    // SAFETY: continues this test's own unreaped child.
    let resume = unsafe { libc::kill(pid(id), libc::SIGCONT) };
    let resumed = crate::exited(id).unwrap();
    // Killed before any assertion, so a failing one leaves no stopped process behind.
    child.kill().unwrap();
    // Asserted below, after the reap, so a stall leaves no child behind.
    let killed = test_time::until_true(
        "the kill is seen before the reap",
        test_time::backstop(Duration::from_secs(10)),
        || crate::exited(id).unwrap(),
    );
    let status = child.wait().unwrap();
    assert_eq!((stop, waited, resume), (0, 0, 0));
    assert_eq!(info.si_code, libc::CLD_STOPPED);
    assert!(!stopped, "a stopped child was taken for ended");
    assert!(!resumed, "a continued child was taken for ended");
    killed.unwrap();
    assert_eq!(
        std::os::unix::process::ExitStatusExt::signal(&status),
        Some(libc::SIGKILL)
    );
}

/// Keep a removed CPU guard from hanging its regression: end the owned group
/// after more CPU work, so the caller can assert on the wrong completion result.
fn cpu_watchdog(
    group: u32,
    limit: Duration,
) -> (std::sync::mpsc::Sender<()>, std::thread::JoinHandle<bool>) {
    let (done, receive) = std::sync::mpsc::channel();
    let watcher = std::thread::spawn(move || {
        let mut budget = None;
        while matches!(
            receive.recv_timeout(POLL),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ) {
            // A transient snapshot error while a churned child exits is not CPU work.
            let Ok(members) = process::members(group) else {
                continue;
            };
            let budget = budget.get_or_insert_with(|| process::Budget::new(limit, &members));
            if budget.exhausted(&members) {
                let _ = signal_group(group, 9);
                return true;
            }
        }
        false
    });
    (done, watcher)
}

/// A stand-in browser: `script`, written to a path of its own and made executable.
fn stand_in_binary(script: &str) -> PathBuf {
    let binary = next_profile().unwrap();
    std::fs::write(&binary, script).unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    binary
}

struct StandIn {
    owner: Owner,
    binary: PathBuf,
    group: Option<u32>,
}

impl StandIn {
    fn new(script: &str) -> Self {
        Self::under(script, &leash())
    }

    fn under(script: &str, leash: &Leash) -> Self {
        let binary = stand_in_binary(script);
        let owner = Owner::launch(binary.to_str().unwrap(), leash).unwrap();
        Self {
            owner,
            binary,
            group: None,
        }
    }

    fn group(&mut self) -> u32 {
        let group = self
            .owner
            .ready(
                "recorded stand-in PID",
                PROTOCOL_CPU,
                LAUNCH_BACKSTOP,
                |profile| {
                    let pid = std::fs::read_to_string(profile.join("browser.pid")).ok()?;
                    if !pid.ends_with('\n') {
                        return None;
                    }
                    pid.trim().parse::<u32>().ok().filter(|pid| *pid > 0)
                },
            )
            .unwrap();
        self.group = Some(group);
        group
    }
}

impl Drop for StandIn {
    fn drop(&mut self) {
        // A probe of BrowserGroup's kill must not strand this fixture's spinner.
        // The separate native leash also owns it if this test is killed outright.
        if let Some(group) = self.group {
            let _ = signal_group(group, 9);
        }
        let _ = std::fs::remove_file(&self.binary);
    }
}

#[test]
fn shutdown_diagnostics_keep_browser_exit_and_suite_eof_distinct_after_profile_cleanup() {
    use std::io::{Seek, Write};
    for (script, close_suite, expected) in [
        (
            "#!/bin/sh\nprintf 'browser exit detail\\n' >&2\nexit 47\n",
            false,
            "browser exited:",
        ),
        (
            "#!/bin/sh\nprintf 'browser waiting detail\\n' >&2\nexec /usr/bin/tail -f /dev/null\n",
            true,
            "suite stdin closed",
        ),
    ] {
        let mut stand_in = StandIn::new(script);
        if close_suite {
            // The launcher's PID record is written after opening the stderr log.
            stand_in.group();
            let diagnostics = stand_in.owner.diagnostics();
            assert!(diagnostics.contains("status=Ok(None)"), "{diagnostics}");
            assert!(
                diagnostics.contains("shutdown=not recorded yet"),
                "{diagnostics}"
            );
            drop(stand_in.owner.leash.stdin.take());
        }
        // Child::wait closes its stdin. Hold the suite's lifetime pipe separately so
        // observing a browser exit cannot itself request an EOF shutdown.
        let lifetime = stand_in.owner.leash.stdin.take();
        let status = stand_in.owner.leash.wait().unwrap();
        drop(lifetime);
        assert_eq!(status.code(), Some(if close_suite { 0 } else { 47 }));
        assert!(!stand_in.owner.profile.exists());
        let diagnostics = stand_in.owner.diagnostics();
        assert!(diagnostics.contains(expected), "{diagnostics}");
        if !close_suite {
            assert!(diagnostics.contains("47"), "{diagnostics}");
            assert!(diagnostics.contains("browser exit detail"), "{diagnostics}");
        }
        assert!(diagnostics.contains("members=Ok([])"), "{diagnostics}");
        let exit = stand_in.owner.profile.with_extension("exit");
        let stderr = stand_in.owner.profile.with_extension("stderr");
        assert!(!exit.exists());
        assert!(!stderr.exists());
        // An arbitrarily long Chrome log must not turn one socket failure into huge output.
        stand_in.owner.stderr.rewind().unwrap();
        stand_in
            .owner
            .stderr
            .write_all(format!("discarded{}kept", "x".repeat(9_000)).as_bytes())
            .unwrap();
        let diagnostics = stand_in.owner.diagnostics();
        assert!(!diagnostics.contains("discarded"));
        assert!(diagnostics.ends_with("kept"));
        assert!(diagnostics.len() < 9_000);
        drop(stand_in);
        assert!(!exit.exists());
        assert!(!stderr.exists());
    }
}

#[test]
fn a_browser_that_cannot_start_fails_the_launch_promptly_with_why() {
    let missing = next_profile().unwrap().join("no-such-chrome");
    let failure = Chrome::launch(missing.to_str().unwrap(), &leash())
        .err()
        .expect("a missing browser must fail its launch");
    let failure = failure.to_string();
    assert!(failure.contains("Chrome's leash exited"), "{failure}");
    assert!(failure.contains("browser did not start"), "{failure}");
    assert!(failure.contains(missing.to_str().unwrap()), "{failure}");
}

/// A browser that starts and exits at once fails its launch as soon as its leash has ended it,
/// with how it ended and what it wrote, long before the launch's backstop.
#[test]
fn a_browser_that_exits_at_once_fails_the_launch_with_its_exit() {
    let binary = stand_in_binary("#!/bin/sh\nprintf 'early exit detail\\n' >&2\nexit 3\n");
    let failure = Chrome::launch(binary.to_str().unwrap(), &leash())
        .err()
        .expect("a browser that exits must fail its launch");
    std::fs::remove_file(&binary).unwrap();
    let failure = failure.to_string();
    assert!(
        failure.starts_with(
            "Chrome DevTools readiness: Chrome's leash exited exit status: 3 before DevTools port \
             readiness; "
        ),
        "{failure}"
    );
    assert!(
        failure.contains("shutdown=browser exited: exit status: 3"),
        "{failure}"
    );
    assert!(failure.contains("early exit detail"), "{failure}");
}

/// What a browser that never opens DevTools does: it sleeps, spending nothing.
const ASLEEP: &str = "#!/bin/sh\nexec /bin/sleep 3600\n";

/// A browser alive that never opens DevTools and spends no CPU, asleep, stalls its launch: the
/// wait ends at its wall-clock backstop, not before, saying it stalled, the browser's pid, its
/// group's processes alive, the CPU they spent, how long it waited and the machine's load. The
/// backstop here is half a second; a launch's own is six minutes ([`LAUNCH_BACKSTOP`]).
#[test]
fn a_browser_that_never_opens_devtools_stalls_its_wait_at_the_backstop() {
    let mut stand_in = StandIn::new(ASLEEP);
    let group = stand_in.group();
    let backstop = Duration::from_millis(500);
    let began = Instant::now();
    let error = stand_in
        .owner
        .ready(
            "DevTools port readiness",
            PROTOCOL_CPU,
            backstop,
            devtools_endpoint,
        )
        .expect_err("a browser that never opens DevTools must fail its wait");
    let waited = began.elapsed();
    // Asleep, so alive as the wait ends, in a group of one; the fixture's drop ends it.
    let alive = process::members(group).unwrap();
    drop(stand_in);
    assert_eq!(error.kind(), ErrorKind::TimedOut);
    assert!(waited >= backstop, "it gave up after {waited:?}");
    assert_eq!(
        alive.iter().map(|member| member.pid).collect::<Vec<_>>(),
        [group]
    );
    let said = error.to_string();
    let stalled = format!(
        "Chrome stalled before DevTools port readiness: Chrome (pid {group}) has 1 process alive \
         in its group, which spent "
    );
    assert!(said.starts_with(&stalled), "{said}");
    assert!(
        said.contains(
            " CPU in the wait, and the launch's wall-clock backstop of 500.0ms ran out after "
        ),
        "{said}"
    );
    assert!(said.contains("; load average "), "{said}");
    assert!(said.ends_with(" over 1, 5 and 15 minutes"), "{said}");
    eprintln!("stalled wait group={group}: {said}");

    // A leash that stalls before it records a browser is named instead: an idle owned process.
    let profile = next_profile().unwrap();
    std::fs::create_dir(&profile).unwrap();
    let idle = Command::new("/bin/cat")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .process_group(0)
        .spawn()
        .unwrap();
    let leash = idle.id();
    let mut owner = Owner {
        leash: idle,
        profile: profile.clone(),
        stderr: std::fs::File::open("/dev/null").unwrap(),
        shutdown: std::fs::File::open("/dev/null").unwrap(),
    };
    let error = owner
        .ready(
            "DevTools port readiness",
            PROTOCOL_CPU,
            Duration::from_millis(100),
            devtools_endpoint,
        )
        .expect_err("a leash that never records a browser must fail its wait");
    drop(owner);
    assert!(!profile.exists());
    assert_eq!(error.kind(), ErrorKind::TimedOut);
    let said = error.to_string();
    let stalled = format!(
        "Chrome stalled before DevTools port readiness: its leash (pid {leash}) had not recorded \
         Chrome's pid, and spent "
    );
    assert!(said.starts_with(&stalled), "{said}");
    assert!(
        said.contains("the launch's wall-clock backstop of 100.0ms ran out after "),
        "{said}"
    );
}

/// The stall of one start, as its launch's failure says it: the pid it names, Chrome's or, on a
/// machine too loaded to have started it, its leash's, leads a group no process is left in, as the
/// launch ended it before it returned.
#[track_caller]
fn a_stall_ended(start: &str) {
    assert!(
        start.starts_with(
            "Chrome DevTools readiness: Chrome stalled before DevTools port readiness: "
        ),
        "{start}"
    );
    assert!(
        start.contains("the launch's wall-clock backstop of 1.0s ran out after "),
        "{start}"
    );
    assert!(start.contains("; load average "), "{start}");
    assert!(start.contains("; leash="), "{start}");
    let pid: u32 = start
        .split_once("(pid ")
        .and_then(|(_, rest)| rest.split_once(')'))
        .and_then(|(pid, _)| pid.parse().ok())
        .unwrap_or_else(|| panic!("no pid in {start}"));
    assert!(process::members(pid).unwrap().is_empty(), "{start}");
}

/// What a launch's failure says after a first start that stalled: the second start's failure,
/// then the stall before it, ended.
#[track_caller]
fn after_a_stall(failure: &str) -> &str {
    let (second, first) = failure
        .split_once("; the start before it: ")
        .unwrap_or_else(|| panic!("no start before it in {failure}"));
    a_stall_ended(first);
    second
}

/// The same stall through a whole launch, on each of its two starts: it fails as a stall, with the
/// second stall first and the first after, each with its diagnostics, and leaves no browser, leash
/// or profile behind.
#[test]
fn a_launch_whose_browser_never_opens_devtools_fails_as_a_stall() {
    let binary = stand_in_binary(ASLEEP);
    let asleep = (binary.to_str().unwrap(), Duration::from_secs(1));
    let failure = Chrome::launch_within(&leash(), [asleep; 2])
        .err()
        .expect("a browser that never opens DevTools must fail its launch");
    std::fs::remove_file(&binary).unwrap();
    assert!(failure.stalled(), "{failure}");
    let failure = failure.to_string();
    a_stall_ended(after_a_stall(&failure));
    eprintln!("stalled launch: {failure}");
}

/// A launch whose first start stalls starts Chrome once more: a second start that exits fails the
/// launch at once with that exit, not as a stall, and names the stall before it. The exit is
/// waited for as a launch waits, so only its own stall could end it sooner.
#[test]
fn a_launch_whose_first_start_stalls_starts_chrome_again_and_names_both_when_that_fails() {
    let asleep = stand_in_binary(ASLEEP);
    let exits = stand_in_binary("#!/bin/sh\nprintf 'second start gave up\\n' >&2\nexit 3\n");
    let failure = Chrome::launch_within(
        &leash(),
        [
            (asleep.to_str().unwrap(), Duration::from_secs(1)),
            (exits.to_str().unwrap(), LAUNCH_BACKSTOP),
        ],
    )
    .err()
    .expect("a second start that exits must fail the launch");
    std::fs::remove_file(&asleep).unwrap();
    std::fs::remove_file(&exits).unwrap();
    assert!(!failure.stalled(), "{failure}");
    let failure = failure.to_string();
    let second = after_a_stall(&failure);
    assert!(
        second.starts_with(
            "Chrome DevTools readiness: Chrome's leash exited exit status: 3 before DevTools port \
             readiness; "
        ),
        "{failure}"
    );
    assert!(second.contains("second start gave up"), "{failure}");
}

/// A launch whose first start stalls runs on the Chrome it starts next, on a profile of its own.
#[test]
fn a_launch_whose_first_start_stalls_runs_on_the_chrome_it_starts_again() {
    let Some(real) = find() else {
        eprintln!("no Chrome: real browser capability unavailable");
        return;
    };
    let asleep = stand_in_binary(ASLEEP);
    let launched = Chrome::launch_within(
        &leash(),
        [
            (asleep.to_str().unwrap(), Duration::from_secs(1)),
            (&real, LAUNCH_BACKSTOP),
        ],
    );
    std::fs::remove_file(&asleep).unwrap();
    let mut chrome = launched.unwrap();
    chrome.open("data:text/html,<p>started again</p>").unwrap();
    assert_eq!(
        chrome
            .eval("document.querySelector('p').textContent")
            .unwrap(),
        json!("started again")
    );
}

/// Set on the run of [`a_quiet_leash_keeps_its_reports_off_the_launchers_stderr`] that launches.
const QUIET: &str = "SYMPHONY_CHROME_QUIET_LEASH";

#[test]
fn a_quiet_leash_keeps_its_reports_off_the_launchers_stderr() {
    const TEST: &str = "tests::a_quiet_leash_keeps_its_reports_off_the_launchers_stderr";
    if let Ok(quiet) = std::env::var(QUIET) {
        let leash = if quiet == "1" {
            leash().quiet()
        } else {
            leash()
        };
        let mut stand_in = StandIn::under("#!/bin/sh\nexit 3\n", &leash);
        let lifetime = stand_in.owner.leash.stdin.take();
        assert_eq!(stand_in.owner.leash.wait().unwrap().code(), Some(3));
        drop(lifetime);
        return;
    }
    for (quiet, reported) in [("0", true), ("1", false)] {
        let output = suite_child(TEST)
            .env(QUIET, quiet)
            .stdout(Stdio::null())
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stderr}");
        assert_eq!(
            stderr.contains("Chrome shutdown"),
            reported,
            "quiet={quiet}: {stderr}"
        );
    }
}

#[test]
fn an_unexpected_protocol_reset_fails_with_the_browser_exit_evidence() {
    let binary = stand_in_binary("#!/bin/sh\nprintf 'reset exit detail\\n' >&2\nexit 47\n");
    let mut owner = Owner::launch(binary.to_str().unwrap(), &leash()).unwrap();
    let lifetime = owner.leash.stdin.take();
    assert_eq!(owner.leash.wait().unwrap().code(), Some(47));
    drop(lifetime);
    std::fs::remove_file(binary).unwrap();

    // A peer dropping TCP without a WebSocket close exercises the reported transport
    // boundary without requiring a real Chrome crash.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (peer, _) = listener.accept().unwrap();
    drop(peer);
    let mut chrome = Chrome {
        owner,
        socket: WebSocket::from_raw_socket(
            MaybeTlsStream::Plain(stream),
            tungstenite::protocol::Role::Client,
            None,
        ),
        next_id: 0,
        session: None,
        context: None,
        observer: None,
        events: Vec::new(),
        arrivals: Vec::new(),
        deadline: None,
        backstop: BACKSTOP_FLOOR,
        frame: None,
        following: false,
        dismissing: false,
        frame_domains: Vec::new(),
        own_pages: Vec::new(),
        fronting: false,
        gone: None,
    };
    let failure = chrome
        .read(false)
        .expect_err("an unexpected reset must still fail the case")
        .to_string();
    assert!(failure.contains("reading from Chrome:"), "{failure}");
    assert!(failure.contains("browser exited:"), "{failure}");
    assert!(failure.contains("reset exit detail"), "{failure}");
}

#[test]
fn cleanup_profile_rejects_a_spinning_helper_missed_by_the_kill() {
    if std::env::var_os("SYMPHONY_CHROME_CLEANUP_SPINNER").is_none() {
        let output =
            suite_child("tests::cleanup_profile_rejects_a_spinning_helper_missed_by_the_kill")
                .env("SYMPHONY_CHROME_CLEANUP_SPINNER", "1")
                .output()
                .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprint!("{stderr}");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        let stdout = String::from_utf8(output.stdout).unwrap();
        let report = stdout
            .lines()
            .find_map(|line| line.strip_prefix("CLEANUP_SPINNER "))
            .expect("cleanup fixture report");
        let (group, seconds): (u32, u64) = serde_json::from_str(report).unwrap();
        let lines = stderr
            .lines()
            .filter(|line| line.starts_with(&format!("Chrome cleanup group={group} ")))
            .count();
        assert!(lines > 0, "the live helper must be diagnosed");
        // Two cleanup calls: one CPU failure and one ESRCH wait after the final kill.
        assert!(
            u64::try_from(lines).unwrap() <= seconds + 2,
            "{lines} ESRCH diagnostics in {seconds} seconds"
        );
        return;
    }

    let mut stand_in = StandIn::new(
        r#"#!/bin/sh
for arg in "$@"; do
    case "$arg" in --user-data-dir=*) profile=${arg#*=};; esac
done
/usr/bin/tail -f /dev/null &
main=$!
(
    while :; do
        /bin/sleep 0.01 &
        i=0
        while test "$i" -lt 2000; do i=$((i+1)); done
        wait
    done
) &
helper=$!
printf '%s %s\n' "$main" "$helper" > "$profile/spinner-ready"
wait "$main"
wait "$helper"
"#,
    );
    let group = stand_in.group();
    let (main, helper) = stand_in
        .owner
        .ready(
            "spinning helper",
            PROTOCOL_CPU,
            LAUNCH_BACKSTOP,
            |profile| {
                let ready = std::fs::read_to_string(profile.join("spinner-ready")).ok()?;
                if !ready.ends_with('\n') {
                    return None;
                }
                let pids: Vec<u32> = ready
                    .split_whitespace()
                    .map(str::parse)
                    .collect::<Result<_, _>>()
                    .ok()?;
                (pids.len() == 2 && pids.iter().all(|pid| *pid > 0)).then(|| (pids[0], pids[1]))
            },
        )
        .unwrap();
    // SAFETY: the owned fixture announced this positive main PID; killing it leaves
    // its helper running in the recorded group, under the separate leash's ownership.
    assert_eq!(unsafe { libc::kill(pid(main), 9) }, 0);
    assert!(
        process::members(group)
            .unwrap()
            .iter()
            .any(|member| member.pid == helper)
    );
    let (done, watcher) = cpu_watchdog(group, CLEANUP_CPU * 2);
    let started = Instant::now();
    let result = cleanup_profile(&stand_in.owner.profile, group);
    let _ = done.send(());
    let forced = watcher.join().unwrap();
    drop(stand_in);
    let error = result.expect_err("a missed spinning helper must fail cleanup");
    assert_eq!(error.kind(), ErrorKind::TimedOut);
    assert!(error.to_string().contains("CPU after SIGKILL"), "{error}");
    eprintln!("spinning helper={helper} group={group}: {error}; watchdog killed={forced}");
    println!(
        "CLEANUP_SPINNER {}",
        json!([group, started.elapsed().as_secs()])
    );
}

/// Drain queued protocol messages until the peer closes the connection, with or without a
/// WebSocket close handshake. Other protocol and I/O errors are fixture failures.
fn wait_for_protocol_eof(
    socket: &mut WebSocket<impl std::io::Read + std::io::Write>,
) -> tungstenite::Result<()> {
    use tungstenite::Error;
    use tungstenite::error::ProtocolError;

    loop {
        match socket.read() {
            Ok(_) => {}
            Err(
                Error::ConnectionClosed
                | Error::Protocol(ProtocolError::ResetWithoutClosingHandshake),
            ) => {
                return Ok(());
            }
            Err(Error::Io(error))
                if matches!(
                    error.kind(),
                    ErrorKind::ConnectionReset
                        | ErrorKind::ConnectionAborted
                        | ErrorKind::BrokenPipe
                ) =>
            {
                return Ok(());
            }
            Err(error) => return Err(error),
        }
    }
}

#[test]
fn protocol_eof_drains_messages_and_rejects_nonclosure_errors() {
    use std::io::Cursor;
    use tungstenite::protocol::Role;

    for ending in [b"".as_slice(), b"\x88\x00"] {
        // Two server text frames precede either TCP EOF or a WebSocket close followed by EOF.
        let frames = [b"\x81\x01a\x81\x01b".as_slice(), ending].concat();
        let mut socket = WebSocket::from_raw_socket(Cursor::new(frames), Role::Client, None);
        wait_for_protocol_eof(&mut socket).unwrap();
        assert!(matches!(
            socket.read(),
            Err(tungstenite::Error::AlreadyClosed)
        ));
    }
    // A reserved opcode is a protocol failure, not evidence that the browser exited.
    let mut socket =
        WebSocket::from_raw_socket(Cursor::new(b"\x83\x00".to_vec()), Role::Client, None);
    assert!(matches!(
        wait_for_protocol_eof(&mut socket),
        Err(tungstenite::Error::Protocol(
            tungstenite::error::ProtocolError::InvalidOpcode(3)
        ))
    ));

    // An open but silent peer is not EOF; exercise WouldBlock without a timing deadline.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (_peer, _) = listener.accept().unwrap();
    stream.set_nonblocking(true).unwrap();
    let mut socket = WebSocket::from_raw_socket(stream, Role::Client, None);
    assert!(matches!(
        wait_for_protocol_eof(&mut socket),
        Err(tungstenite::Error::Io(error)) if error.kind() == ErrorKind::WouldBlock
    ));

    struct FailedIo(ErrorKind);
    impl std::io::Read for FailedIo {
        fn read(&mut self, _buffer: &mut [u8]) -> std::io::Result<usize> {
            Err(self.0.into())
        }
    }
    impl std::io::Write for FailedIo {
        fn write(&mut self, _buffer: &[u8]) -> std::io::Result<usize> {
            Err(self.0.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Err(self.0.into())
        }
    }
    for kind in [
        ErrorKind::ConnectionReset,
        ErrorKind::ConnectionAborted,
        ErrorKind::BrokenPipe,
        ErrorKind::TimedOut,
    ] {
        let mut socket = WebSocket::from_raw_socket(FailedIo(kind), Role::Client, None);
        let result = wait_for_protocol_eof(&mut socket);
        if kind == ErrorKind::TimedOut {
            assert!(matches!(result, Err(tungstenite::Error::Io(error)) if error.kind() == kind));
        } else {
            result.unwrap();
        }
    }
}

const SUITE_SIGNALS: [libc::c_int; 3] = [libc::SIGINT, libc::SIGHUP, libc::SIGTERM];

fn suite_signal_disposition(disposition: libc::sighandler_t) -> std::io::Result<()> {
    // SAFETY: sigemptyset and sigaction are async-signal-safe. The initialized action
    // contains no handler pointer and changes only the calling process's dispositions.
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = disposition;
        if libc::sigemptyset(&mut action.sa_mask) != 0 {
            return Err(std::io::Error::last_os_error());
        }
        for signal in SUITE_SIGNALS {
            if libc::sigaction(signal, &action, std::ptr::null_mut()) != 0 {
                return Err(std::io::Error::last_os_error());
            }
        }
    }
    Ok(())
}

/// This test binary run again on the case `test` alone.
pub(crate) fn suite_child(test: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args(["--exact", test, "--nocapture"]);
    // Background shells can pass SIG_IGN through exec. Every catchable signal in
    // the lifecycle matrix must terminate this child; SIGKILL cannot be ignored.
    // SAFETY: the hook changes only the forked child's dispositions through
    // async-signal-safe calls, before it executes the suite fixture.
    unsafe { command.pre_exec(|| suite_signal_disposition(libc::SIG_DFL)) };
    command
}

#[test]
fn suite_child_resets_inherited_ignored_signals() {
    const TEST: &str = "tests::suite_child_resets_inherited_ignored_signals";
    const MODE: &str = "SYMPHONY_CHROME_SUITE_SIGNAL_MODE";
    let mode = std::env::var(MODE).ok();
    if let Some(mode) = mode {
        let expected = match mode.as_str() {
            "ignored-parent" => libc::SIG_IGN,
            "reset-child" => libc::SIG_DFL,
            _ => panic!("unknown suite signal fixture mode: {mode}"),
        };
        for signal in SUITE_SIGNALS {
            // SAFETY: a null action queries this process's disposition into initialized storage.
            let action = unsafe {
                let mut action: libc::sigaction = std::mem::zeroed();
                assert_eq!(libc::sigaction(signal, std::ptr::null(), &mut action), 0);
                action
            };
            assert_eq!(action.sa_sigaction, expected, "signal {signal} in {mode}");
        }
        if mode == "ignored-parent" {
            assert!(
                suite_child(TEST)
                    .env(MODE, "reset-child")
                    .status()
                    .unwrap()
                    .success(),
                "suite child must reset its ignored parent's signal dispositions"
            );
        }
        return;
    }

    let mut parent = Command::new(std::env::current_exe().unwrap());
    parent
        .args(["--exact", TEST, "--nocapture"])
        .env(MODE, "ignored-parent");
    // SAFETY: only this forked parent's dispositions change, through the same
    // async-signal-safe calls. The concurrent test runner keeps its own dispositions.
    unsafe { parent.pre_exec(|| suite_signal_disposition(libc::SIG_IGN)) };
    assert!(parent.status().unwrap().success());
}

#[test]
fn chrome_dies_with_its_suite() {
    use std::io::{BufRead, Write};
    use std::os::unix::process::ExitStatusExt;
    if std::env::var_os("SYMPHONY_CHROME_SUITE_CHILD").is_some() {
        let fake_binary = std::env::var("SYMPHONY_FAKE_BROWSER").ok();
        let real = fake_binary
            .is_none()
            .then(|| Chrome::launch(&find().expect("real Chrome capability"), &leash()).unwrap());
        let mut fake = fake_binary.map(|binary| Owner::launch(&binary, &leash()).unwrap());
        let profile = real
            .as_ref()
            .map(|chrome| &chrome.owner.profile)
            .or_else(|| fake.as_ref().map(|owner| &owner.profile))
            .unwrap()
            .clone();
        let helper = fake.as_mut().map(|owner| {
            owner
                .ready(
                    "stand-in helper handshake",
                    PROTOCOL_CPU,
                    LAUNCH_BACKSTOP,
                    |profile| {
                        if let Ok(ready) = std::fs::read_to_string(profile.join("fake-ready"))
                            && ready.ends_with('\n')
                        {
                            let pids: Vec<u32> = ready
                                .split_whitespace()
                                .filter_map(|pid| pid.parse().ok())
                                .collect();
                            let recorded = std::fs::read_to_string(profile.join("browser.pid"))
                                .ok()
                                .filter(|text| text.ends_with('\n'))
                                .and_then(|text| text.trim().parse::<u32>().ok());
                            if pids.len() == 2 && recorded == Some(pids[0]) {
                                return Some(pids[1]);
                            }
                        }
                        None
                    },
                )
                .expect("stand-in helper handshake")
        });
        let endpoint = real
            .as_ref()
            .map(|_| std::fs::read_to_string(profile.join("DevToolsActivePort")).unwrap());
        let group: u32 = std::fs::read_to_string(profile.join("browser.pid"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let leash = real
            .as_ref()
            .map(|chrome| chrome.owner.leash.id())
            .or_else(|| fake.as_ref().map(|owner| owner.leash.id()))
            .unwrap();
        println!(
            "CHROME_READY {}",
            json!([profile, endpoint, group, helper, leash])
        );
        std::io::stdout().flush().unwrap();
        let _ = std::io::copy(&mut std::io::stdin().lock(), &mut std::io::sink());
        return;
    }
    struct Suite(std::process::Child);
    impl Drop for Suite {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    struct Sweep(Option<u32>, PathBuf);
    impl Drop for Sweep {
        fn drop(&mut self) {
            if let Some(group) = self.0 {
                let _ = signal_group(group, 9);
            }
            let _ = std::fs::remove_dir_all(&self.1);
        }
    }
    struct Scratch(PathBuf);
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let scratch = Scratch(next_profile().unwrap());
    std::fs::create_dir(&scratch.0).unwrap();
    let fake_browser = scratch.0.join("browser");
    std::fs::write(
        &fake_browser,
        r#"#!/bin/sh
for arg in "$@"; do
    case "$arg" in --user-data-dir=*) profile=${arg#*=};; esac
done
test -n "$profile" || exit 2
trap '' HUP TERM
(
    while :; do
        mkdir -p "$profile/writing/nested"
        printf '%s\n' writing > "$profile/writing/nested/data"
        rm -f "$profile/writing/nested/data"
    done
) 2>/dev/null &
helper=$!
trap 'exit 0' TERM
printf '%s %s\n' "$$" "$helper" > "$profile/fake-ready"
wait "$helper"
"#,
    )
    .unwrap();
    std::fs::set_permissions(&fake_browser, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut variants = vec![Some(fake_browser)];
    if find().is_some() {
        variants.push(None);
    } else {
        eprintln!("no Chrome: real browser capability unavailable; stand-in still tested");
    }
    for fake_browser in variants {
        for (signal, expected, closed_stderr) in [
            ("normal", None, false),
            ("-TERM", Some(libc::SIGTERM), false),
            ("-HUP", Some(libc::SIGHUP), false),
            ("-INT", Some(libc::SIGINT), false),
            ("-KILL", Some(libc::SIGKILL), false),
            ("-KILL", Some(libc::SIGKILL), true),
        ] {
            let mut command = suite_child("tests::chrome_dies_with_its_suite");
            if let Some(binary) = &fake_browser {
                command.env("SYMPHONY_FAKE_BROWSER", binary);
            } else {
                command.env_remove("SYMPHONY_FAKE_BROWSER");
            }
            let mut suite = Suite(
                command
                    .env("SYMPHONY_CHROME_SUITE_CHILD", "1")
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(if closed_stderr {
                        Stdio::piped()
                    } else {
                        Stdio::inherit()
                    })
                    .spawn()
                    .unwrap(),
            );
            let stdout = suite.0.stdout.take().unwrap();
            let (send, receive) = std::sync::mpsc::channel();
            let reader = std::thread::spawn(move || {
                for line in std::io::BufReader::new(stdout)
                    .lines()
                    .map_while(Result::ok)
                {
                    if let Some(ready) = line.strip_prefix("CHROME_READY ") {
                        let _ =
                            send.send(
                                serde_json::from_str::<(
                                    PathBuf,
                                    Option<String>,
                                    u32,
                                    Option<u32>,
                                    u32,
                                )>(ready)
                                .unwrap(),
                            );
                    }
                }
            });
            let (profile, endpoint, group, helper, leash) =
                receive.recv().expect("suite protocol handshake");
            let mut sweep = Sweep(Some(group), profile.clone());
            assert!(
                signal_group(group, 0).is_ok(),
                "recorded browser group must be alive before stopping its suite"
            );
            if let Some(helper) = helper {
                // SAFETY: getpgid only queries the positive helper PID the owned fixture announced.
                let actual = unsafe { libc::getpgid(pid(helper)) };
                assert_eq!(
                    actual,
                    pid(group),
                    "the never-exiting helper must be alive in the browser's group"
                );
            }
            assert_eq!(
                std::fs::metadata(&profile).unwrap().permissions().mode() & 0o777,
                0o700
            );
            let mut protocol = endpoint.map(|endpoint| {
                let mut lines = endpoint.lines();
                let port = lines.next().unwrap().to_string();
                let path = lines.next().unwrap();
                let (socket, _) =
                    tungstenite::connect(format!("ws://127.0.0.1:{port}{path}")).unwrap();
                (socket, port)
            });
            if closed_stderr {
                // The suite has announced its live browser and leash. Lose the output reader
                // before killing the suite, as when a piped cargo test loses its consumer.
                drop(suite.0.stderr.take().expect("piped suite stderr reader"));
            }
            let input = suite.0.stdin.take(); // Kept to the reap: Child::wait closes it first.
            if let Some(expected) = expected {
                assert!(
                    Command::new("/bin/kill")
                        .args([signal, &suite.0.id().to_string()])
                        .status()
                        .unwrap()
                        .success()
                );
                assert_eq!(suite.0.wait().unwrap().signal(), Some(expected));
            } else {
                drop(input);
                assert!(suite.0.wait().unwrap().success());
            }
            reader.join().unwrap();
            // Observe protocol closure, then require the whole recorded group to be gone below.
            // A closing WebSocket alone can precede the browser releasing its listening socket.
            if let Some((socket, _)) = &mut protocol {
                wait_for_protocol_eof(socket)
                    .unwrap_or_else(|error| panic!("browser protocol after {signal}: {error}"));
            }
            test_time::until_true(
                format!("the leash {leash} ends after {signal}"),
                test_time::FLOOR,
                || {
                    // SAFETY: the suite announced this exact leash PID at startup. Signal zero
                    // only queries it, including its zombie until the parent reaps it.
                    if unsafe { libc::kill(pid(leash), 0) } == 0 {
                        return false;
                    }
                    let error = std::io::Error::last_os_error();
                    if error.raw_os_error() == Some(3) {
                        return true;
                    }
                    assert_eq!(error.kind(), ErrorKind::PermissionDenied, "{error}");
                    false
                },
            )
            .unwrap();
            // The leash's exit is the boundary under test. Waiting here for a leaked
            // helper/profile would hide a premature exit instead of failing the case.
            assert_eq!(
                signal_group(group, 0)
                    .err()
                    .and_then(|error| error.raw_os_error()),
                Some(3),
                "browser group outlived its leash after {signal}"
            );
            assert!(
                !profile.exists(),
                "profile outlived its leash after {signal}"
            );
            for extension in ["stderr", "exit"] {
                assert!(
                    !profile.with_extension(extension).exists(),
                    "{extension} log outlived its leash after {signal}"
                );
            }
            // An ended suite took its folder with it; the next run's sweep takes a killed one's.
            profiles::sweep();
            let folder = profile.parent().unwrap();
            assert!(!folder.exists(), "suite's folder outlived {signal}");
            sweep.0 = None;
            if let Some((_, port)) = &protocol {
                assert!(
                    TcpStream::connect(format!("127.0.0.1:{port}")).is_err(),
                    "DevTools survived {signal}"
                );
            }
        }
    }
}

#[test]
fn startup_requires_complete_readiness_and_observes_leash_exit() {
    let profile = next_profile().unwrap();
    std::fs::create_dir(&profile).unwrap();
    let mut child = Command::new("/bin/sh")
        .args(["-c", "read -r release"])
        .stdin(Stdio::piped())
        .process_group(0)
        .spawn()
        .unwrap();
    let group = child.id();
    let release = child.stdin.take().unwrap();
    let owner = Owner {
        leash: child,
        profile: profile.clone(),
        stderr: std::fs::File::open("/dev/null").unwrap(),
        shutdown: std::fs::File::open("/dev/null").unwrap(),
    };
    // These are actual partial/invalid writes at the file boundary Chrome uses.
    for (pid, port) in [
        (
            "123",
            "1234\n/devtools/browser/12345678-1234-1234-1234-123456789abc",
        ),
        ("123\n", "1234\n"),
        (
            "123\n",
            "0\n/devtools/browser/12345678-1234-1234-1234-123456789abc",
        ),
        (
            "123\n",
            "1234\n/devtools/browser/12345678-1234-1234-1234-123456789ab",
        ),
    ] {
        std::fs::write(profile.join("browser.pid"), pid).unwrap();
        std::fs::write(profile.join("DevToolsActivePort"), port).unwrap();
        assert!(devtools_endpoint(&profile).is_none());
    }
    // The owned child is idle on its pipe; its wall wait cannot spend CPU credit.
    std::fs::write(profile.join("browser.pid"), format!("{group}\n")).unwrap();
    let waiter = std::thread::spawn(move || {
        let mut owner = owner;
        let endpoint = owner
            .ready(
                "complete test port",
                PROTOCOL_CPU,
                LAUNCH_BACKSTOP,
                devtools_endpoint,
            )
            .unwrap();
        (owner, endpoint)
    });
    std::fs::write(
        profile.join("DevToolsActivePort"),
        "1234\n/devtools/browser/12345678-1234-1234-1234-123456789abc",
    )
    .unwrap();
    let (mut owner, endpoint) = waiter.join().unwrap();
    assert_eq!(
        endpoint,
        (
            "1234".into(),
            "/devtools/browser/12345678-1234-1234-1234-123456789abc".into()
        )
    );
    std::fs::remove_file(profile.join("DevToolsActivePort")).unwrap();
    drop(release);
    assert!(!owner.leash.wait().unwrap().success());
    let error = owner
        .ready(
            "missing port after exit",
            PROTOCOL_CPU,
            LAUNCH_BACKSTOP,
            devtools_endpoint,
        )
        .unwrap_err();
    assert!(error.to_string().contains("exited"), "{error}");
    drop(owner);
    assert!(!profile.exists());

    // The real leash owns a stand-in that burns CPU without publishing a port.
    let mut stand_in = StandIn::new("#!/bin/sh\nwhile :; do :; done\n");
    let group = stand_in.group();
    let (done, watcher) = cpu_watchdog(group, Duration::from_secs(1));
    let result = stand_in.owner.ready(
        "missing stand-in port",
        Duration::from_millis(50),
        LAUNCH_BACKSTOP,
        devtools_endpoint,
    );
    let _ = done.send(());
    let forced = watcher.join().unwrap();
    drop(stand_in);
    let error = result.expect_err("a spinning browser without a port must fail startup");
    assert_eq!(error.kind(), ErrorKind::TimedOut);
    assert!(
        error
            .to_string()
            .contains("CPU without missing stand-in port"),
        "{error}"
    );
    eprintln!("spinning startup group={group}: {error}; watchdog killed={forced}");
}

/// The stand-in browser's end of a driver's socket: each command as the driver sent it, and the
/// replies and events it sends back, through a writer of their own so that a burst of them never
/// holds up the driver's own sends.
struct Peer {
    reader: WebSocket<TcpStream>,
    writer: Option<std::sync::mpsc::Sender<Value>>,
    /// The late messages [`Peer::late`] holds, each sent unless the stand-in ends first.
    cancels: Vec<std::sync::mpsc::Sender<()>>,
}

impl Peer {
    /// The driver's next command; `None` once the driver has gone.
    fn next(&mut self) -> Option<Value> {
        match self.reader.read() {
            Ok(Message::Text(text)) => Some(serde_json::from_str(text.as_str()).unwrap()),
            _ => None,
        }
    }

    fn send(&self, message: Value) {
        self.writer.as_ref().unwrap().send(message).unwrap();
    }

    fn reply(&self, command: &Value, result: Value) {
        self.send(json!({"id": command["id"], "result": result}));
    }

    /// `message`, `after` this long unless the stand-in ends first: a late success, so a driver
    /// that stops waiting as it should is refused by a test rather than left hanging.
    fn late(&mut self, after: Duration, message: Value) {
        let (cancel, cancelled) = std::sync::mpsc::channel::<()>();
        let send = self.writer.clone().unwrap();
        std::thread::spawn(move || {
            if let Err(std::sync::mpsc::RecvTimeoutError::Timeout) = cancelled.recv_timeout(after) {
                let _ = send.send(message);
            }
        });
        self.cancels.push(cancel);
    }
}

/// An observation turn: the observer page's timer, or a browser target census before there is one.
fn marker(command: &Value) -> bool {
    command["sessionId"] == "observer"
        || (command["method"] == "Target.getTargets" && command.get("sessionId").is_none())
}

/// A probe of the page under test.
fn probe(command: &Value) -> bool {
    command["sessionId"] == "page"
        && command["method"] == "Runtime.evaluate"
        && command["params"]["expression"] == "0"
}

/// The driver on a stand-in browser whose commands `serve` answers, with a page session `page`,
/// an observer session `observer` when `observer`, and `backstop` the least a wait's wall-clock
/// backstop allows. An idle owned process stands for the browser's group, so its CPU never
/// advances: Chrome's background startup work is legitimate CPU and cannot serve as an idle
/// fixture for a deliberately small allowance. What `serve` returns once the driver has gone, by
/// the handle.
fn with_peer<T: Send + 'static>(
    observer: bool,
    backstop: Duration,
    serve: impl FnOnce(&mut Peer) -> T + Send + 'static,
) -> (Chrome, std::thread::JoinHandle<T>) {
    use tungstenite::protocol::Role;

    let profile = next_profile().unwrap();
    std::fs::create_dir(&profile).unwrap();
    let idle = Command::new("/bin/cat")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (far, _) = listener.accept().unwrap();
    let served = std::thread::spawn(move || {
        let (send, receive) = std::sync::mpsc::channel::<Value>();
        let mut socket = WebSocket::from_raw_socket(far.try_clone().unwrap(), Role::Server, None);
        let writer = std::thread::spawn(move || {
            for value in receive {
                if socket.send(Message::text(value.to_string())).is_err() {
                    break;
                }
            }
        });
        let mut far_end = Peer {
            reader: WebSocket::from_raw_socket(far, Role::Server, None),
            writer: Some(send),
            cancels: Vec::new(),
        };
        let result = serve(&mut far_end);
        far_end.writer.take();
        far_end.cancels.clear();
        drop(far_end);
        writer.join().unwrap();
        result
    });
    let chrome = Chrome {
        owner: Owner {
            leash: idle,
            profile,
            stderr: std::fs::File::open("/dev/null").unwrap(),
            shutdown: std::fs::File::open("/dev/null").unwrap(),
        },
        socket: WebSocket::from_raw_socket(MaybeTlsStream::Plain(stream), Role::Client, None),
        next_id: 0,
        session: Some("page".into()),
        context: None,
        observer: observer.then(|| "observer".into()),
        events: Vec::new(),
        arrivals: Vec::new(),
        deadline: None,
        backstop,
        frame: None,
        following: false,
        dismissing: false,
        frame_domains: Vec::new(),
        // The targets of the observer's page and the page under test, as a launch has them.
        own_pages: vec!["observer-target".into(), "page-target".into()],
        fronting: false,
        gone: None,
    };
    (chrome, served)
}

#[test]
fn browser_waits_keep_events_and_bound_idle_polls_without_charging_observer_delay() {
    // Exercise the actual transport and kernel meter with an idle owned process.
    let (mut controlled, peer) = with_peer(true, BACKSTOP_FLOOR, |peer| {
        let mut evaluations = 0;
        let mut idle_evaluations = 0;
        let mut pending_markers = 0;
        let mut probes = 0;
        // The evaluation held open, and the observer turns after which it turns true.
        let mut pending: Option<(Value, u32)> = None;
        while let Some(command) = peer.next() {
            assert_eq!(command["method"], "Runtime.evaluate");
            if marker(&command) {
                if let Some((held, left)) = &mut pending {
                    pending_markers += 1;
                    *left -= 1;
                    if *left == 0 {
                        // Only the driver's allowance may reject the pending expression before
                        // this late reply.
                        peer.reply(held, json!({"result": {"value": true}}));
                        pending = None;
                    }
                }
                // A pump's or an observation turn's timer.
                peer.reply(&command, json!({"result": {"value": null}}));
                continue;
            }
            if probe(&command) {
                probes += 1;
                peer.reply(&command, json!({"result": {"type": "number", "value": 0}}));
                continue;
            }
            // A new evaluation: the driver gave up the one held open.
            pending = None;
            let expression = command["params"]["expression"].as_str().unwrap().to_owned();
            if matches!(expression.as_str(), "pending-wait" | "pending-eval") {
                peer.send(json!({"method":"Fixture.observed", "params":command["params"]}));
                // One turn more than the driver's allowance takes: two for a 26 ms wait, 2400
                // for an evaluation, and the first, before the page has answered a probe.
                let turns = if expression == "pending-wait" {
                    4
                } else {
                    2402
                };
                // A late success after two minutes also fails the case if the driver stops
                // observing altogether, rather than leaving it hanging.
                peer.late(
                    Duration::from_secs(120),
                    json!({"id": command["id"], "result": {"result": {"value": true}}}),
                );
                pending = Some((command, turns));
                continue;
            }
            if expression == "protocol-error" {
                peer.send(json!({"id":command["id"],"error":{"message":"fixture protocol error"}}));
                continue;
            }
            if expression == "evaluation-error" {
                peer.reply(
                    &command,
                    json!({"exceptionDetails":{"text":"fixture evaluation error"}}),
                );
                continue;
            }
            let value = match expression.as_str() {
                "ready" => {
                    evaluations += 1;
                    if evaluations == 1 {
                        // Reply only after this owned timer process exits. The first
                        // result is always false after >100 ms wall time, with no
                        // work charged to the idle process's 100 ms CPU allowance.
                        assert!(
                            Command::new("/bin/sleep")
                                .arg("0.25")
                                .status()
                                .unwrap()
                                .success()
                        );
                    }
                    json!(evaluations > 1)
                }
                "never-ready" => {
                    idle_evaluations += 1;
                    // Close the transport on excess retries so a regression fails promptly
                    // even though the owned process stays idle and its CPU never advances.
                    assert!(
                        idle_evaluations <= 3,
                        "idle wait exceeded its poll allowance"
                    );
                    json!(false)
                }
                "after-timeout" => json!("usable"),
                // Asked of a condition that threw: this page's document has its element.
                ROOTED => json!(1.0),
                other => panic!("unexpected evaluation {other}"),
            };
            peer.reply(&command, json!({"result": {"value": value}}));
        }
        (evaluations, idle_evaluations, pending_markers, probes)
    });
    assert!(
        controlled
            .wait_for("ready", Duration::from_millis(100))
            .unwrap()
    );
    assert!(!controlled.wait_for("never-ready", Duration::ZERO).unwrap());
    assert!(
        !controlled
            .wait_for("never-ready", Duration::from_millis(26))
            .unwrap()
    );
    assert!(
        !controlled
            .wait_for("pending-wait", Duration::from_millis(26))
            .unwrap()
    );
    let why = controlled
        .eval("pending-eval")
        .expect_err("plain eval must bound a promise that outlives all observer turns")
        .to_string();
    assert!(why.contains("2400 completed observer polls"), "{why}");
    assert_eq!(controlled.eval("after-timeout").unwrap(), json!("usable"));
    for (expression, expected) in [
        ("protocol-error", "fixture protocol error"),
        ("evaluation-error", "fixture evaluation error"),
    ] {
        let why = controlled
            .wait_for(expression, Duration::ZERO)
            .expect_err("an evaluation failure must not become a false condition")
            .to_string();
        assert!(why.contains(expected), "{why}");
    }
    assert!(
        controlled
            .events
            .iter()
            .any(|event| event["method"] == "Fixture.observed"
                && event["params"]["expression"] == "pending-eval")
    );
    drop(controlled);
    // Each pending wait's turns: the first, before the page answered its probe, then its
    // allowance's, each after a probe the page answered: 1 + 2 for the 26 ms wait and 1 + 2400
    // for the evaluation.
    assert_eq!(peer.join().unwrap(), (2, 3, 3 + 2401, 2 + 2400));

    let Some(binary) = find() else {
        eprintln!("no Chrome: real browser capability unavailable");
        return;
    };
    let mut chrome = Chrome::launch(&binary, &leash()).unwrap();
    chrome.open("data:text/html,<p>ready</p>").unwrap();
    // open consumed the event from the wire but kept it for later observers. Even
    // a zero CPU allowance must accept an event/condition that already succeeded.
    chrome
        .wait_event("Page.domContentEventFired", Duration::ZERO)
        .unwrap();
    assert!(
        chrome
            .wait_for("document.querySelector('p')", Duration::ZERO)
            .unwrap()
    );
    assert!(
        !chrome
            .wait_for("new Promise(() => {})", Duration::from_millis(26))
            .unwrap()
    );
    assert!(
        !chrome
            .wait_for("Promise.resolve(false)", Duration::ZERO)
            .unwrap()
    );
    assert!(
        chrome
            .wait_for("Promise.resolve(true)", Duration::ZERO)
            .unwrap()
    );
    chrome.eval("window.idlePolls=0").unwrap();
    assert!(
        !chrome
            .wait_for(
                "(window.idlePolls++, document.querySelector('#missing'))",
                Duration::from_millis(26)
            )
            .unwrap()
    );
    let polls = chrome.eval("window.idlePolls").unwrap().as_u64().unwrap();
    assert!(
        (1..=2).contains(&polls),
        "idle page evaluated {polls} times"
    );
    // A false condition that itself does renderer work must also exhaust its allowance.
    assert!(!chrome
        .wait_for(
            "(() => { const until=performance.now()+50; while(performance.now()<until) {} return false; })()",
            Duration::from_millis(1)
        )
        .unwrap());
    chrome
        .eval("window.observed=false; setTimeout(()=>{window.observed=true}, 5)")
        .unwrap();
    chrome.pump(Duration::from_millis(10)).unwrap();
    assert!(
        chrome
            .wait_for("window.observed === true", PROTOCOL_CPU)
            .unwrap()
    );
    chrome
        .eval("setTimeout(() => location.href='about:blank#next', 10)")
        .unwrap();
    // Page navigation may discard its own promises. The observation marker's
    // session belongs to the stable blank target and must still answer.
    chrome.pump(Duration::from_millis(50)).unwrap();
    assert!(
        chrome
            .wait_for("location.href === 'about:blank#next'", PROTOCOL_CPU)
            .unwrap()
    );
    chrome.open("data:text/html,<p>after</p>").unwrap();
    assert_eq!(
        chrome
            .eval("document.querySelector('p').textContent")
            .unwrap(),
        json!("after")
    );
}

#[test]
fn navigation_and_protocol_replies_end_on_completed_observer_turns() {
    for (navigation, observer) in [(true, true), (false, true), (false, false)] {
        let (mut chrome, peer) = with_peer(observer, BACKSTOP_FLOOR, move |peer| {
            let mut held = false;
            let mut polls = 0;
            let mut probes = 0;
            while let Some(command) = peer.next() {
                if !held {
                    assert_eq!(
                        command["method"],
                        if navigation {
                            "Page.navigate"
                        } else {
                            "Fixture.held"
                        }
                    );
                    held = true;
                    if navigation {
                        // Navigation is accepted, but its document never finishes parsing
                        // within the allowance. open must fail on the missing event.
                        peer.reply(&command, json!({}));
                    }
                    peer.send(json!({"method":"Fixture.unrelated"}));
                    // The late success, after two minutes: a driver that stops observing
                    // returns successfully instead of hanging the test, and fails the
                    // expect_err below.
                    peer.late(
                        Duration::from_secs(120),
                        if navigation {
                            json!({"method":"Page.domContentEventFired"})
                        } else {
                            json!({"id":command["id"],"result":{}})
                        },
                    );
                } else if probe(&command) {
                    // The page answers every probe: each observer turn after the first counts.
                    probes += 1;
                    peer.reply(&command, json!({"result": {"value": 0}}));
                } else {
                    assert_eq!(
                        command["method"],
                        if observer {
                            "Runtime.evaluate"
                        } else {
                            "Target.getTargets"
                        }
                    );
                    if observer {
                        assert_eq!(command["sessionId"], "observer");
                    } else {
                        assert!(command.get("sessionId").is_none());
                    }
                    polls += 1;
                    peer.reply(&command, json!({}));
                }
            }
            (held, polls, probes)
        });
        let started = Instant::now();
        let why = if navigation {
            chrome
                .open("https://fixture.invalid/held")
                .map(|()| Value::Null)
        } else {
            chrome.call("Fixture.held", json!({}))
        }
        .expect_err("a late event or reply must exhaust completed observer turns")
        .to_string();
        assert!(
            why.contains(if navigation {
                "Page.domContentEventFired"
            } else {
                "Fixture.held"
            }),
            "{why}"
        );
        assert!(why.contains("2400 completed observer polls"), "{why}");
        assert!(
            chrome
                .events
                .iter()
                .any(|event| event["method"] == "Fixture.unrelated")
        );
        eprintln!(
            "navigation={navigation} observer={observer} bounded in {:?}: {why}",
            started.elapsed()
        );
        drop(chrome);
        // The wait's turns: the first, before the page answered a probe, then the allowance's
        // 2400, each after one; a navigation's call took one turn more, its reply coming first.
        assert_eq!(
            peer.join().unwrap(),
            (true, if navigation { 2402 } else { 2401 }, 2400)
        );
    }
}

/// A stand-in page frozen until the browser has taken `frozen` observer turns: the replies to
/// what the driver asked of the page meanwhile are held, and go, probes first, as that turn ends,
/// the held evaluations' as `meet` makes them. Every later probe is answered at once, every later
/// evaluation as `meet` makes it, `None` holding it for good. What the page saw: its evaluations,
/// probes and the observer turns.
fn frozen_page(
    frozen: u32,
    meet: impl Fn(u32) -> Option<Value> + Send + 'static,
) -> (Chrome, std::thread::JoinHandle<(u32, u32, u32)>) {
    with_peer(true, BACKSTOP_FLOOR, move |peer| {
        let (mut evaluations, mut probes, mut markers) = (0, 0, 0);
        let mut held: Vec<(Value, Option<Value>)> = Vec::new();
        let answer = |peer: &Peer, command: &Value, value: Option<Value>| {
            if let Some(value) = value {
                peer.reply(command, json!({"result": {"value": value}}));
            }
        };
        while let Some(command) = peer.next() {
            if marker(&command) {
                markers += 1;
                if markers == frozen {
                    held.sort_by_key(|(command, _)| !probe(command));
                    for (command, value) in held.drain(..) {
                        answer(peer, &command, value);
                    }
                }
                peer.reply(&command, json!({"result": {"value": null}}));
                continue;
            }
            let value = if probe(&command) {
                probes += 1;
                Some(json!(0))
            } else {
                assert_eq!(command["method"], "Runtime.evaluate", "{command}");
                evaluations += 1;
                meet(evaluations)
            };
            if markers < frozen {
                held.push((command, value));
            } else {
                answer(peer, &command, value);
            }
        }
        (evaluations, probes, markers)
    })
}

#[test]
fn a_page_frozen_mid_wait_spends_none_of_its_allowance_and_meets_it_once_it_runs_again() {
    // Frozen for a hundred times the wait's four turns (100 ms) from its first evaluation on, which
    // the page answers false once it runs again, and the second true.
    let (mut chrome, page) = frozen_page(400, |evaluation| Some(json!(evaluation > 1)));
    let started = Instant::now();
    assert!(
        chrome
            .wait_for("window.met", Duration::from_millis(100))
            .unwrap(),
        "a frozen page spent its wait in {:?}",
        started.elapsed()
    );
    drop(chrome);
    let (evaluations, probes, markers) = page.join().unwrap();
    assert_eq!(evaluations, 2);
    assert!(markers > 400, "the wait outlived {markers} observer turns");
    assert!(probes >= 1, "the page was never probed");
}

#[test]
fn a_page_that_runs_again_but_never_meets_its_condition_fails_within_its_allowance() {
    // Frozen for 299 observer turns, which count for nothing, then running: the wait's four
    // turns (100 ms) are the four after it, the first as the page's held probes answer.
    let (mut chrome, page) = frozen_page(300, |_| None);
    assert!(
        !chrome
            .wait_for("window.never", Duration::from_millis(100))
            .unwrap()
    );
    drop(chrome);
    let (evaluations, probes, markers) = page.join().unwrap();
    assert_eq!((evaluations, markers), (1, 303));
    assert!(probes >= 4, "{probes} probes");
}

#[test]
fn a_page_that_never_runs_ends_each_wait_at_its_backstop_with_an_error_naming_the_stall() {
    // Nothing on the page's session is ever answered but one navigation; the observer answers
    // every turn at once. A browser command is not the page's: it waits out its own turns.
    let (mut chrome, peer) = with_peer(true, Duration::from_millis(300), |peer| {
        let (mut markers, mut probes, mut browser_probes) = (0u32, 0u32, None::<u32>);
        while let Some(command) = peer.next() {
            if marker(&command) {
                markers += 1;
                peer.reply(&command, json!({"result": {"value": null}}));
            } else if probe(&command) {
                probes += 1;
                if let Some(count) = &mut browser_probes {
                    *count += 1;
                }
            } else if command["method"] == "Fixture.browser" {
                assert!(command.get("sessionId").is_none(), "{command}");
                browser_probes = Some(0);
            } else if command["params"]["url"] == "https://fixture.invalid/loads" {
                peer.reply(&command, json!({"frameId": "page"}));
            }
        }
        (markers, probes, browser_probes)
    });
    type Wait = fn(&mut Chrome) -> crate::Result<()>;
    let waits: [(&str, Wait); 4] = [
        ("stalled: an evaluation", |chrome| {
            chrome
                .wait_for("stalled: an evaluation", Duration::from_millis(26))
                .map(drop)
        }),
        ("waiting for Fixture.never", |chrome| {
            chrome
                .wait_for_protocol_events("Fixture.never", 1, Duration::from_millis(26))
                .map(drop)
        }),
        ("Page.navigate", |chrome| {
            chrome.navigate("https://fixture.invalid/held", Duration::from_millis(26))
        }),
        ("waiting for Page.domContentEventFired", |chrome| {
            chrome.navigate("https://fixture.invalid/loads", Duration::from_millis(26))
        }),
    ];
    for (what, wait) in waits {
        let started = Instant::now();
        let error = wait(&mut chrome).unwrap_err();
        let took = started.elapsed();
        let message = error.to_string();
        assert!(
            error.stalled() && !error.past_deadline(),
            "{what}: {message}"
        );
        assert!(
            message.starts_with(&format!(
                "{what}: the page under test stalled: its renderer took part in 0 of "
            )),
            "{message}"
        );
        assert!(
            message.contains("the wait's wall-clock backstop of 300ms ran out"),
            "{message}"
        );
        // A wait is a stall only when the backstop is the check that ended it (`End::Backstop`),
        // whenever its error is made: had the allowance ended it, the error would name the polls
        // and CPU spent instead. How much later than the backstop a loaded machine let it end is
        // no part of the promise (RULES.md `tests/no-test-fails-on-load`).
        assert!(
            took >= Duration::from_millis(300),
            "{what} ended {took:?} after its start, its backstop 300 ms"
        );
    }
    let why = chrome
        .browser_call("Fixture.browser", json!({}))
        .unwrap_err();
    assert!(!why.stalled(), "{why}");
    assert!(
        why.to_string()
            .contains("Fixture.browser: no reply after 2400 completed observer polls"),
        "{why}"
    );
    drop(chrome);
    let (markers, probes, browser_probes) = peer.join().unwrap();
    assert!(
        markers > 2400 && probes >= 4,
        "{markers} turns, {probes} probes"
    );
    assert_eq!(browser_probes, Some(0), "a browser command probed the page");
}

#[test]
fn an_error_reply_or_a_failed_observer_turn_fails_the_wait_naming_it() {
    let (mut chrome, peer) = with_peer(true, BACKSTOP_FLOOR, |peer| {
        let mut broken = false;
        while let Some(command) = peer.next() {
            if command["method"] == "Fixture.refused" {
                peer.send(json!({"id": command["id"], "error": {"message": "refused fixture"}}));
            } else if command["params"]["expression"] == "observer-broken" {
                broken = true;
            } else if marker(&command) && broken {
                peer.reply(
                    &command,
                    json!({"exceptionDetails": {"text": "observer gone"}}),
                );
            } else if marker(&command) {
                peer.reply(&command, json!({"result": {"value": null}}));
            }
        }
        broken
    });
    let refused = chrome
        .call("Fixture.refused", json!({}))
        .unwrap_err()
        .to_string();
    assert!(
        refused.starts_with("Fixture.refused: ") && refused.contains("refused fixture"),
        "{refused}"
    );
    let broken = chrome
        .wait_for("observer-broken", Duration::from_millis(100))
        .unwrap_err()
        .to_string();
    assert!(
        broken.starts_with("observer poll: ") && broken.contains("observer gone"),
        "{broken}"
    );
    // A wait before a page is attached is the browser's: every observer turn counts.
    assert!(chrome.page_budget(POLL).unwrap().page.is_some());
    chrome.session = None;
    assert!(chrome.page_budget(POLL).unwrap().page.is_none());
    drop(chrome);
    assert!(peer.join().unwrap());
}

#[test]
fn an_evaluation_lost_to_a_navigation_is_told_by_chromes_message_alone() {
    for (error, lost) in [
        (
            json!({"code": -32000, "message": "Execution context was destroyed."}),
            true,
        ),
        (
            json!({"code": -32000, "message": "Inspected target navigated or closed"}),
            true,
        ),
        (
            json!({"code": -32000, "message": "Cannot find default execution context"}),
            true,
        ),
        // The message alone, whole: another message, one that only begins as a lost one does,
        // or none, is a failure of the evaluation itself.
        (
            json!({"code": -32001, "message": "Session with given id not found."}),
            false,
        ),
        (
            json!({"code": -32000, "message": "Execution context was destroyed"}),
            false,
        ),
        (
            json!({"code": -32000, "message": "Inspected target navigated or closed!"}),
            false,
        ),
        (json!({"code": -32000}), false),
        (json!("Execution context was destroyed."), false),
    ] {
        let told = crate::Error::evaluation("window.x", &error);
        assert_eq!(told.navigated, lost, "{error}");
        assert_eq!(told.to_string(), format!("window.x: {error}"));
        assert!(!told.stalled() && !told.past_deadline(), "{error}");
    }
}

/// Chrome's error reply of `message` to `command`.
fn refuse(peer: &Peer, command: &Value, message: &str) {
    peer.send(json!({"id": command["id"], "error": {"code": -32000, "message": message}}));
}

#[test]
fn a_wait_whose_evaluation_a_navigation_takes_asks_the_page_again_within_its_allowance() {
    const DESTROYED: &str = "Execution context was destroyed.";
    const NAVIGATED: &str = "Inspected target navigated or closed";
    const BETWEEN: &str = "Cannot find default execution context";
    // The stand-in page answers each evaluation of an expression in turn: with Chrome's error
    // reply (`Err`) or a value, at once, or once the browser has taken one more observer turn
    // after the one its watch began with (`held`), the evaluation in flight as the page leaves.
    // Probes and observer turns are answered at once. What it saw: each expression asked.
    let (mut chrome, peer) = with_peer(true, BACKSTOP_FLOOR, |peer| {
        let answer = |peer: &Peer, command: &Value, answer: Result<Value, &str>| match answer {
            Ok(value) => peer.reply(command, json!({"result": {"value": value}})),
            Err(message) => refuse(peer, command, message),
        };
        let mut asked: Vec<String> = Vec::new();
        let mut markers = 0u32;
        let mut held: Vec<(Value, Result<Value, &str>, u32)> = Vec::new();
        while let Some(command) = peer.next() {
            if marker(&command) {
                markers += 1;
                for (command, value, _) in held.extract_if(.., |(_, _, at)| markers > *at + 1) {
                    answer(peer, &command, value);
                }
                peer.reply(&command, json!({"result": {"value": null}}));
                continue;
            }
            if probe(&command) {
                peer.reply(&command, json!({"result": {"value": 0}}));
                continue;
            }
            assert_eq!(command["method"], "Runtime.evaluate", "{command}");
            let expression = command["params"]["expression"].as_str().unwrap().to_owned();
            asked.push(expression.clone());
            let count = asked.iter().filter(|asked| **asked == expression).count();
            let (value, hold) = match (expression.as_str(), count) {
                // The page goes on to another process under the first look, is asked between two
                // documents, goes on in its own frame host under the third, and meets the
                // condition on the second look at the document it stays on.
                ("window.arrived", 1) => (Err(NAVIGATED), true),
                ("window.arrived", 2) => (Err(BETWEEN), false),
                ("window.arrived", 3) => (Err(DESTROYED), true),
                ("window.arrived", 4) => (Ok(json!(false)), false),
                ("window.arrived", 5) => (Ok(json!(true)), false),
                // A page that goes on to another document whenever it is asked.
                ("window.leaving", _) => (Err(DESTROYED), false),
                // A page whose next ask, after a lost one, is refused otherwise (here as Chrome
                // refuses a session it no longer has): any error reply but a lost evaluation's
                // ends the wait. A page closed under an evaluation leaves it unanswered
                // (a_wait_on_a_page_that_closes_or_crashes_fails_at_once_saying_which).
                ("window.refused", 1) => (Err(NAVIGATED), false),
                ("window.refused", 2) => (Err("Session with given id not found."), false),
                ("window.asked", 1) => (Err(NAVIGATED), false),
                (other, count) => panic!("evaluation {count} of {other}"),
            };
            if hold {
                held.push((command, value, markers));
            } else {
                answer(peer, &command, value);
            }
        }
        asked
    });
    assert!(chrome.wait_for("window.arrived", PROTOCOL_CPU).unwrap());
    // Each lost evaluation is one of the page's turns: a 26 ms wait's two (two asks), and the one
    // a zero allowance still takes. The condition was never met.
    assert!(
        !chrome
            .wait_for("window.leaving", Duration::from_millis(26))
            .unwrap()
    );
    assert!(!chrome.wait_for("window.leaving", Duration::ZERO).unwrap());
    let refused = chrome.wait_for("window.refused", PROTOCOL_CPU).unwrap_err();
    assert!(
        refused.to_string().starts_with(
            "window.refused: {\"code\":-32000,\"message\":\"Session with given id not found.\""
        ),
        "{refused}"
    );
    assert!(!refused.stalled() && !refused.past_deadline(), "{refused}");
    // An evaluation on its own is asked once: it may act.
    let lost = chrome.eval("window.asked").unwrap_err().to_string();
    assert!(
        lost.starts_with("window.asked: ") && lost.contains(NAVIGATED),
        "{lost}"
    );
    drop(chrome);
    let asked = peer.join().unwrap();
    let count = |expression: &str| asked.iter().filter(|asked| *asked == expression).count();
    assert_eq!(
        [
            "window.arrived",
            "window.leaving",
            "window.refused",
            "window.asked"
        ]
        .map(count),
        [5, 2 + 1, 2, 1],
        "{asked:?}"
    );
}

#[test]
fn a_wait_on_a_page_that_closes_or_crashes_fails_at_once_saying_which() {
    const CRASHED: &str = "the page under test's renderer crashed";
    const CLOSED: &str = "the page under test closed: its session detached";
    // Chrome's messages as it tells of the page under test going away, as the real Chrome sends
    // them (a_real_page_closed_or_crashed_under_a_wait_fails_it_at_once_saying_which), and those
    // of a followed frame, which detaches and crashes on its own session.
    let crashed = |session: &str| {
        json!({"method": "Inspector.targetCrashed",
            "params": {}, "sessionId": session})
    };
    let detached = |session: &str, target: &str, on: Option<&str>| {
        let mut event = json!({"method": "Target.detachedFromTarget",
            "params": {"sessionId": session, "targetId": target}});
        if let Some(on) = on {
            event["sessionId"] = json!(on);
        }
        event
    };
    let page_closes = vec![
        json!({"method": "Inspector.detached", "params": {"reason": "Render process gone."},
            "sessionId": "page"}),
        detached("page", "page-target", None),
    ];
    let a_frame_goes = vec![
        detached("frame", "frame-target", Some("page")),
        crashed("frame"),
    ];
    let page_crashes = vec![crashed("page")];
    // What the stand-in page's renderer does with what is asked of it: answers it, answers nothing
    // (crashed) or refuses it for want of a session (closed), as Chrome does.
    #[derive(Clone, Copy, PartialEq)]
    enum Renderer {
        Running,
        Crashed,
        Closed,
    }
    // Each evaluation of a wait's expression is held until the browser's next observer turn,
    // which sends what it names first, then its answer (none: never answered), and leaves the
    // renderer as it names. Probes and observer turns are answered at once while it runs. What it
    // saw: each expression asked, refused or not.
    let (mut chrome, peer) = with_peer(true, BACKSTOP_FLOOR, move |peer| {
        let mut asked: Vec<String> = Vec::new();
        let mut renderer = Renderer::Running;
        let mut held: Option<(Value, Vec<Value>, Option<Value>, Renderer)> = None;
        while let Some(command) = peer.next() {
            if marker(&command) {
                if let Some((evaluation, events, answer, then)) = held.take() {
                    for event in events {
                        peer.send(event);
                    }
                    if let Some(value) = answer {
                        peer.reply(&evaluation, json!({"result": {"value": value}}));
                    }
                    renderer = then;
                }
                peer.reply(&command, json!({"result": {"value": null}}));
                continue;
            }
            assert_eq!(command["sessionId"], "page", "{command}");
            if !probe(&command) && command["method"] == "Runtime.evaluate" {
                asked.push(command["params"]["expression"].as_str().unwrap().to_owned());
            }
            match renderer {
                Renderer::Closed => {
                    peer.send(json!({"id": command["id"],
                        "error": {"code": -32001, "message": "Session with given id not found."}}));
                    continue;
                }
                // A reload starts another renderer, which Chrome tells of before its reply.
                Renderer::Crashed if command["method"] == "Page.reload" => {
                    peer.send(json!({"method": "Inspector.targetReloadedAfterCrash",
                        "params": {}, "sessionId": "page"}));
                    peer.reply(&command, json!({}));
                    renderer = Renderer::Running;
                    continue;
                }
                Renderer::Crashed => continue,
                Renderer::Running => {}
            }
            if probe(&command) {
                peer.reply(&command, json!({"result": {"value": 0}}));
                continue;
            }
            let expression = command["params"]["expression"].as_str().unwrap();
            let (events, answer, then) = match expression {
                "window.framed" => (a_frame_goes.clone(), Some(json!(true)), Renderer::Running),
                "window.crashing" => (page_crashes.clone(), None, Renderer::Crashed),
                "window.back" => (Vec::new(), Some(json!(true)), Renderer::Running),
                "window.closing" => (page_closes.clone(), None, Renderer::Closed),
                other => panic!("evaluation of {other}"),
            };
            held = Some((command, events, answer, then));
        }
        asked
    });
    // A followed frame's session detaching, or its renderer crashing, is not the page's: the wait
    // goes on and meets its condition.
    assert!(chrome.wait_for("window.framed", PROTOCOL_CPU).unwrap());
    // The page's renderer crashes under the wait, which Chrome answers nothing of after: no turn
    // of the page's counts, so only the backstop would end it, two minutes on. It fails at once,
    // and so does a wait begun on the crashed page, which asks nothing.
    let gone = |chrome: &mut Chrome, expression: &str| {
        let error = chrome
            .wait_for(expression, Duration::from_millis(100))
            .unwrap_err();
        assert!(!error.stalled() && !error.past_deadline(), "{error}");
        error.to_string()
    };
    assert_eq!(
        gone(&mut chrome, "window.crashing"),
        format!("window.crashing: {CRASHED}")
    );
    assert_eq!(
        gone(&mut chrome, "window.after"),
        format!("window.after: {CRASHED}")
    );
    // A reload brings the renderer back, and the page is waited on again.
    chrome.call("Page.reload", json!({})).unwrap();
    assert!(chrome.wait_for("window.back", PROTOCOL_CPU).unwrap());
    // The page closes under the wait. Chrome refuses every probe after it, each a turn of the
    // page's, which would spend the wait's allowance and end it with its condition unmet: it fails
    // at once, and so does a wait begun after, which asks nothing.
    assert_eq!(
        gone(&mut chrome, "window.closing"),
        format!("window.closing: {CLOSED}")
    );
    assert_eq!(
        gone(&mut chrome, "window.gone"),
        format!("window.gone: {CLOSED}")
    );
    // An evaluation on its own is asked as before, and refused.
    assert_eq!(
        chrome.eval("window.asked").unwrap_err().to_string(),
        "window.asked: {\"code\":-32001,\"message\":\"Session with given id not found.\"}"
    );
    // Each message that told of the page's end is kept, the one that ended a wait among them.
    let told: Vec<(Value, Value)> = chrome
        .events()
        .iter()
        .map(|event| (event["method"].clone(), event["sessionId"].clone()))
        .collect();
    assert_eq!(
        told,
        [
            ("Target.detachedFromTarget", json!("page")),
            ("Inspector.targetCrashed", json!("frame")),
            ("Inspector.targetCrashed", json!("page")),
            ("Inspector.targetReloadedAfterCrash", json!("page")),
            ("Inspector.detached", json!("page")),
            ("Target.detachedFromTarget", Value::Null),
        ]
        .map(|(method, session)| (json!(method), session))
    );
    drop(chrome);
    assert_eq!(
        peer.join().unwrap(),
        [
            "window.framed",
            "window.crashing",
            "window.back",
            "window.closing",
            "window.asked"
        ]
    );
}

#[test]
fn a_chrome_that_refuses_to_tell_of_shared_workers_still_has_frames_followed() {
    let (mut chrome, peer) = with_peer(true, BACKSTOP_FLOOR, |peer| {
        let mut refused = false;
        while let Some(command) = peer.next() {
            if marker(&command) || probe(&command) {
                peer.reply(&command, json!({"result": {"value": 0}}));
            } else if command["method"] == "Target.setDiscoverTargets" {
                // Asked of the browser's own session, and refused there.
                refused = command["sessionId"].is_null();
                peer.send(json!({"id": command["id"], "error": {"message": "refused fixture"}}));
            } else {
                peer.reply(&command, json!({}));
            }
        }
        refused
    });
    chrome.follow_frames().unwrap();
    assert!(chrome.following);
    drop(chrome);
    assert!(peer.join().unwrap());
}

/// `Target.attachedToTarget` for a target of `kind` on `session`, held there (`waiting`) or not.
fn attached(session: &str, kind: &str, waiting: bool) -> Value {
    json!({"sessionId": "page", "method": "Target.attachedToTarget", "params": {
        "sessionId": session, "waitingForDebugger": waiting,
        "targetInfo": {"targetId": session, "type": kind, "url": "http://b/"}}})
}

#[test]
fn a_followed_frame_or_worker_has_its_events_on_and_what_it_starts_followed_before_it_runs() {
    let (mut chrome, peer) = with_peer(true, BACKSTOP_FLOOR, |peer| {
        // Every command but the observer's turns and the page's probes: its session, its method and
        // its parameters.
        let mut seen = Vec::new();
        while let Some(command) = peer.next() {
            if marker(&command) || probe(&command) {
                peer.reply(&command, json!({"result": {"value": 0}}));
                continue;
            }
            seen.push((
                command["sessionId"].as_str().unwrap_or("").to_string(),
                command["method"].as_str().unwrap_or("").to_string(),
                command["params"].clone(),
            ));
            // The targets the page starts, told of before the reply, as Chrome does.
            match (command["method"].as_str(), command["sessionId"].as_str()) {
                (Some("Fixture.before"), _) => {
                    peer.send(attached("unfollowed", "iframe", true));
                    peer.reply(&command, json!({}));
                }
                (Some("Target.setAutoAttach"), Some("page")) => {
                    peer.send(attached("frame", "iframe", true));
                    peer.send(attached("worker", "worker", true));
                    peer.send(attached("running", "iframe", false));
                    peer.send(attached("service", "service_worker", true));
                    peer.reply(&command, json!({}));
                }
                (Some("Target.setDiscoverTargets"), None) => peer.reply(&command, json!({})),
                _ => {}
            }
        }
        seen
    });
    // Until frames are followed, a target the page starts is kept and left as it is.
    chrome.call("Fixture.before", json!({})).unwrap();
    chrome.enable_in_frames(&["Runtime.enable", "Log.enable"]);
    chrome.follow_frames().unwrap();
    let kept: Vec<&Value> = chrome
        .events
        .iter()
        .filter(|event| event["method"] == "Target.attachedToTarget")
        .map(|event| &event["params"]["sessionId"])
        .collect();
    assert_eq!(
        kept,
        ["unfollowed", "frame", "worker", "running", "service"]
    );
    drop(chrome);
    let follow = json!({"autoAttach": true, "waitForDebuggerOnStart": true, "flatten": true});
    let command = |session: &str, method: &str, params: &Value| {
        (session.to_string(), method.to_string(), params.clone())
    };
    let none = json!({});
    assert_eq!(
        peer.join().unwrap(),
        [
            command("page", "Fixture.before", &none),
            command("page", "Target.setAutoAttach", &follow),
            // A held frame: its network and page events on, and what was asked for frames, and its
            // own frames followed, then let run.
            command("frame", "Network.enable", &none),
            command("frame", "Page.enable", &none),
            command("frame", "Runtime.enable", &none),
            command("frame", "Log.enable", &none),
            command("frame", "Target.setAutoAttach", &follow),
            command("frame", "Runtime.runIfWaitingForDebugger", &none),
            // A held worker: its network events on and the workers it starts followed.
            command("worker", "Network.enable", &none),
            command("worker", "Target.setAutoAttach", &follow),
            command("worker", "Runtime.runIfWaitingForDebugger", &none),
            // A frame already running is followed and has nothing to be let do.
            command("running", "Network.enable", &none),
            command("running", "Page.enable", &none),
            command("running", "Runtime.enable", &none),
            command("running", "Log.enable", &none),
            command("running", "Target.setAutoAttach", &follow),
            // Any other target is only let run.
            command("service", "Runtime.runIfWaitingForDebugger", &none),
            // Of the browser's own session, only to tell of shared workers: nothing attached.
            command(
                "",
                "Target.setDiscoverTargets",
                &json!({"discover": true, "filter": [{"type": "shared_worker"}]}),
            ),
        ]
    );
}

#[test]
fn a_dialog_is_answered_on_the_session_it_opened_on_once_dialogs_are_dismissed_and_kept() {
    let (mut chrome, peer) = with_peer(true, BACKSTOP_FLOOR, |peer| {
        let mut answered = Vec::new();
        while let Some(command) = peer.next() {
            if marker(&command) || probe(&command) {
                peer.reply(&command, json!({"result": {"value": 0}}));
                continue;
            }
            if command["method"] == "Fixture.open" {
                // Each kind of dialog, on the page and in a followed frame, told of before the
                // reply.
                for (session, kind) in [
                    ("page", "alert"),
                    ("frame", "confirm"),
                    ("page", "prompt"),
                    ("page", "beforeunload"),
                ] {
                    peer.send(
                        json!({"sessionId": session, "method": "Page.javascriptDialogOpening",
                        "params": {"type": kind, "message": kind, "url": "http://a/"}}),
                    );
                }
                peer.reply(&command, json!({}));
                continue;
            }
            answered.push((
                command["sessionId"].as_str().unwrap_or("").to_string(),
                command["method"].as_str().unwrap_or("").to_string(),
                command["params"].clone(),
            ));
        }
        answered
    });
    // Not dismissing, a dialog is only kept.
    chrome.call("Fixture.open", json!({})).unwrap();
    chrome.dismiss_dialogs();
    chrome.call("Fixture.open", json!({})).unwrap();
    let kept = chrome
        .events()
        .iter()
        .filter(|event| event["method"] == "Page.javascriptDialogOpening")
        .count();
    assert_eq!(kept, 8);
    drop(chrome);
    let answer = |session: &str, accept: bool| {
        (
            session.to_string(),
            "Page.handleJavaScriptDialog".to_string(),
            json!({"accept": accept}),
        )
    };
    // Dismissed, but a beforeunload, which lets the page leave.
    assert_eq!(
        peer.join().unwrap(),
        [
            answer("page", false),
            answer("frame", false),
            answer("page", false),
            answer("page", true),
        ]
    );
}

#[test]
fn a_page_the_page_opens_brings_it_back_in_front_unwaited_as_it_is_told_of_once_kept_in_front() {
    let (mut chrome, peer) = with_peer(true, BACKSTOP_FLOOR, |peer| {
        // Every command but the observer's turns and the page's probes: its session, its method and
        // its parameters.
        let mut seen = Vec::new();
        while let Some(command) = peer.next() {
            if marker(&command) || probe(&command) {
                peer.reply(&command, json!({"result": {"value": 0}}));
                continue;
            }
            seen.push((
                command["sessionId"].as_str().unwrap_or("").to_string(),
                command["method"].as_str().unwrap_or("").to_string(),
                command["params"].clone(),
            ));
            if command["method"] == "Fixture.open" {
                // Told of before the reply, as Chrome tells of what a call started: a page the
                // page opened, appearing and committing its document; the page itself and the
                // observer's page changing; a shared worker appearing; the opened page closing.
                for (method, target, kind) in [
                    ("Target.targetCreated", "opened", "page"),
                    ("Target.targetInfoChanged", "opened", "page"),
                    ("Target.targetInfoChanged", "page-target", "page"),
                    ("Target.targetInfoChanged", "observer-target", "page"),
                    ("Target.targetCreated", "shared", "shared_worker"),
                ] {
                    peer.send(json!({"method": method, "params": {
                        "targetInfo": {"targetId": target, "type": kind, "url": "http://a/"}}}));
                }
                peer.send(json!({"method": "Target.targetDestroyed",
                    "params": {"targetId": "opened"}}));
            }
            // Bringing the page in front is never waited for: it gets no reply here.
            if command["method"] != "Page.bringToFront" {
                peer.reply(&command, json!({}));
            }
        }
        seen
    });
    // Not kept in front, a page opened is only kept among the events.
    chrome.call("Fixture.open", json!({})).unwrap();
    chrome.keep_in_front().unwrap();
    chrome.follow_frames().unwrap();
    chrome.call("Fixture.open", json!({})).unwrap();
    // A wait goes on, the page never answering its bringing in front.
    chrome.call("Fixture.after", json!({})).unwrap();
    let told = chrome
        .events()
        .iter()
        .filter(|event| {
            event["method"]
                .as_str()
                .unwrap()
                .starts_with("Target.target")
        })
        .count();
    assert_eq!(told, 12);
    drop(chrome);
    let command = |session: &str, method: &str, params: Value| {
        (session.to_string(), method.to_string(), params)
    };
    let discover = |kinds: &[&str]| {
        let filter: Vec<Value> = kinds.iter().map(|kind| json!({ "type": kind })).collect();
        json!({"discover": true, "filter": filter})
    };
    assert_eq!(
        peer.join().unwrap(),
        [
            command("page", "Fixture.open", json!({})),
            // Of the browser's own session: pages told of, then shared workers too, as frames
            // are followed.
            command("", "Target.setDiscoverTargets", discover(&["page"])),
            command(
                "page",
                "Target.setAutoAttach",
                json!({"autoAttach": true, "waitForDebuggerOnStart": true, "flatten": true}),
            ),
            command(
                "",
                "Target.setDiscoverTargets",
                discover(&["shared_worker", "page"]),
            ),
            command("page", "Fixture.open", json!({})),
            // The opened page appearing, then committing: the page brought in front each time,
            // on its own session; nothing for the page itself, the observer's, a worker or a page
            // closing.
            command("page", "Page.bringToFront", json!({})),
            command("page", "Page.bringToFront", json!({})),
            command("page", "Fixture.after", json!({})),
        ]
    );
}

#[test]
fn a_page_opened_while_the_page_is_asked_is_put_behind_it_so_the_page_answers() {
    // As Chrome holds the page when a page of its own site, opened later (from a timer) while the
    // page is being asked something, shows a dialog in front: the page answers nothing, its probes
    // included, until it is brought back in front, which closes that dialog. Told of that page as
    // the evaluation waits, the driver brings the page in front, and the evaluation is answered.
    let (mut chrome, peer) = with_peer(true, BACKSTOP_FLOOR, |peer| {
        let mut held = Vec::new();
        let mut fronted = false;
        while let Some(command) = peer.next() {
            let asked = command["sessionId"] == "page" && command["method"] == "Runtime.evaluate";
            if command["method"] == "Page.bringToFront" {
                fronted = true;
                for command in held.drain(..) {
                    answer(peer, &command);
                }
            } else if asked && !fronted {
                if command["params"]["expression"] == "document.title" {
                    peer.send(json!({"method": "Target.targetCreated", "params": {
                        "targetInfo": {"targetId": "opened", "type": "page", "url": ""}}}));
                }
                held.push(command);
            } else {
                answer(peer, &command);
            }
        }
        fronted
    });
    fn answer(peer: &Peer, command: &Value) {
        let value = if command["params"]["expression"] == "document.title" {
            json!("answered")
        } else {
            json!(0)
        };
        peer.reply(command, json!({"result": {"value": value}}));
    }
    chrome.keep_in_front().unwrap();
    assert_eq!(chrome.eval("document.title").unwrap(), json!("answered"));
    drop(chrome);
    assert!(peer.join().unwrap());
}

/// A loopback server ([`framing_server`]) and how a test drives it: its address and port, the
/// paths of the held requests as each arrives, and a release of one held document's rest.
struct Framing {
    base: String,
    port: u16,
    arrived: std::sync::mpsc::Receiver<String>,
    release: std::sync::mpsc::Sender<()>,
}

/// A loopback server of a page with two frames Chrome runs in processes of their own, a sandboxed
/// one of its own and a cross-site one (the server again, as `localhost`), each with a picture; of
/// `/swapping.html`, a frame of `localhost` and one of the page's own site; of `/holding.html`, a
/// frame whose picture `/held.png` is never answered; and of `/slow.html` (any query), a document
/// whose head and first bytes come at once and the rest only once the test sends on its `release`,
/// one send each. It tells of each `/held.png` and `/slow.html` request, by its path and query, as
/// it arrives.
fn framing_server() -> Framing {
    use std::io::{BufRead, Write};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let base = format!("http://127.0.0.1:{port}/");
    let (arrive, arrived) = std::sync::mpsc::channel::<String>();
    let (release, released) = std::sync::mpsc::channel::<()>();
    let released = std::sync::Arc::new(std::sync::Mutex::new(released));
    std::thread::spawn(move || {
        for connection in listener.incoming() {
            let Ok(connection) = connection else {
                return;
            };
            let (arrive, released) = (arrive.clone(), released.clone());
            std::thread::spawn(move || {
                let mut reader = std::io::BufReader::new(connection.try_clone().unwrap());
                let mut head = String::new();
                while reader.read_line(&mut head).is_ok_and(|read| read > 2) {}
                let path = head.split(' ').nth(1).unwrap_or("/").to_string();
                let answer = |kind: &str, body: &str| {
                    let _ = write!(
                        &connection,
                        "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nCache-Control: no-store\r\n\
                         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                };
                match path.split('?').next().unwrap_or("/") {
                    "/" => answer(
                        "text/html",
                        &format!(
                            "<!doctype html><iframe id=sandboxed sandbox=allow-scripts \
                             src=/frame.html></iframe><iframe id=cross \
                             src=http://localhost:{port}/frame.html></iframe>"
                        ),
                    ),
                    "/swapping.html" => answer(
                        "text/html",
                        &format!(
                            "<!doctype html><iframe id=back \
                             src=http://localhost:{port}/frame.html></iframe><iframe id=out \
                             src=/frame.html></iframe>"
                        ),
                    ),
                    "/frame.html" => answer("text/html", "<!doctype html><img src=/p.png>"),
                    "/holding.html" => answer("text/html", "<!doctype html><img src=/held.png>"),
                    "/held.png" => {
                        // Never answered: held until the browser lets the connection go.
                        let _ = arrive.send(path);
                        let mut rest = String::new();
                        while reader.read_line(&mut rest).is_ok_and(|read| read > 0) {}
                    }
                    "/slow.html" => {
                        let (first, rest) = ("<!doctype html><p>slow", "ly</p>");
                        let _ = write!(
                            &connection,
                            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\
                             Cache-Control: no-store\r\nContent-Length: {}\r\n\
                             Connection: close\r\n\r\n{first}",
                            first.len() + rest.len()
                        );
                        let _ = (&connection).flush();
                        let _ = arrive.send(path);
                        // The rest once released; never, once the test has ended.
                        if released.lock().unwrap().recv().is_ok() {
                            let _ = write!(&connection, "{rest}");
                        }
                    }
                    _ => answer("image/png", ""),
                }
            });
        }
    });
    Framing {
        base,
        port,
        arrived,
        release,
    }
}

/// Wait, reading the browser meanwhile (a followed frame starting is held until the driver reads
/// of it and lets it run), until the server has seen a request for each of `paths`.
fn arrive(chrome: &mut Chrome, framing: &Framing, paths: &[&str]) {
    let mut seen: Vec<String> = Vec::new();
    test_time::until_true(
        format!("the server asked for {paths:?}"),
        test_time::backstop(PROTOCOL_CPU),
        || {
            chrome.pump(POLL).unwrap();
            seen.extend(framing.arrived.try_iter());
            paths
                .iter()
                .all(|path| seen.iter().any(|seen| seen == path))
        },
    )
    .unwrap_or_else(|stall| panic!("{stall}"));
}

/// Wait until the requests [`Chrome::requests`] reads meet `condition`; them.
fn requests_until(
    chrome: &mut Chrome,
    what: &str,
    condition: impl Fn(&[crate::Request]) -> bool,
) -> Vec<crate::Request> {
    test_time::until(what, test_time::backstop(PROTOCOL_CPU), || {
        let requests = chrome.requests().unwrap();
        condition(&requests).then_some(requests)
    })
    .unwrap_or_else(|stall| panic!("{stall}"))
}

/// The request to `url` among `requests`, if any.
fn request<'a>(requests: &'a [crate::Request], url: &str) -> Option<&'a crate::Request> {
    requests.iter().find(|request| request.url == url)
}

/// Whether the request to `url` among `requests` is still open: seen, neither finished nor failed.
fn still_open(requests: &[crate::Request], url: &str) -> bool {
    request(requests, url).is_some_and(|request| !request.finished && request.failed.is_none())
}

#[test]
fn a_followed_frame_in_a_process_of_its_own_ends_its_requests_among_the_pages_and_its_removal_too()
{
    let Some(binary) = find() else {
        eprintln!("no Chrome: real browser capability unavailable");
        return;
    };
    let framing = framing_server();
    let (base, port) = (framing.base.clone(), framing.port);
    let mut chrome = Chrome::launch(&binary, &leash()).unwrap();
    chrome.follow_frames().unwrap();
    chrome.navigate(&base, PROTOCOL_CPU).unwrap();
    let sandboxed = format!("{base}frame.html");
    let cross = format!("http://localhost:{port}/frame.html");
    let pictures = [
        format!("{base}p.png"),
        format!("http://localhost:{port}/p.png"),
    ];
    let wanted = [&sandboxed, &cross, &pictures[0], &pictures[1]];
    let requests = test_time::until(
        "both frames' documents and pictures answered",
        test_time::backstop(PROTOCOL_CPU),
        || {
            let requests = chrome.requests().unwrap();
            wanted
                .iter()
                .all(|url| {
                    requests
                        .iter()
                        .any(|request| &request.url == *url && request.finished)
                })
                .then_some(requests)
        },
    )
    .unwrap_or_else(|stall| panic!("{stall}"));
    let by = |url: &str| requests.iter().find(|request| request.url == url).unwrap();
    assert_eq!(by(&base).frame, None);
    for (document, picture) in [(&sandboxed, &pictures[0]), (&cross, &pictures[1])] {
        let document = by(document);
        assert_eq!((document.kind.as_str(), document.status), ("Document", 200));
        assert_eq!(document.frame.as_deref(), Some(document.url.as_str()));
        assert_eq!(by(picture).frame.as_deref(), Some(document.url.as_str()));
    }

    // A frame removed while its picture is in flight leaves no request open.
    let holding = format!("http://localhost:{port}/holding.html");
    let held = format!("http://localhost:{port}/held.png");
    chrome
        .eval(&format!(
            "(() => {{ const frame = document.createElement('iframe'); frame.id = 'holding'; \
             frame.src = {}; document.body.append(frame); return 0; }})()",
            json!(holding)
        ))
        .unwrap();
    arrive(&mut chrome, &framing, &["/held.png"]);
    let open = |chrome: &mut Chrome| {
        chrome
            .requests()
            .unwrap()
            .into_iter()
            .find(|request| request.url == held)
            .filter(|request| !request.finished && request.failed.is_none())
    };
    let picture = test_time::until(
        "the held picture told of",
        test_time::backstop(PROTOCOL_CPU),
        || open(&mut chrome),
    )
    .unwrap_or_else(|stall| panic!("{stall}"));
    assert_eq!(picture.frame.as_deref(), Some(holding.as_str()));
    chrome
        .eval("document.getElementById('holding').remove(), 0")
        .unwrap();
    test_time::until_true(
        "the held picture ended with its frame",
        test_time::backstop(PROTOCOL_CPU),
        || open(&mut chrome).is_none(),
    )
    .unwrap_or_else(|stall| panic!("{stall}"));
    let ended = chrome
        .requests()
        .unwrap()
        .into_iter()
        .find(|request| request.url == held)
        .unwrap();
    // Given up, by the page or with its frame, never a failure of the network's.
    assert!(ended.canceled, "{ended:?}");
}

#[test]
fn a_followed_frame_moving_between_processes_mid_load_is_waited_for_and_a_removed_one_is_not() {
    let Some(binary) = find() else {
        eprintln!("no Chrome: real browser capability unavailable");
        return;
    };
    let framing = framing_server();
    let (base, port) = (framing.base.clone(), framing.port);
    let mut chrome = Chrome::launch(&binary, &leash()).unwrap();
    chrome.follow_frames().unwrap();
    chrome
        .navigate(&format!("{base}swapping.html"), PROTOCOL_CPU)
        .unwrap();
    let first = [
        format!("http://localhost:{port}/frame.html"),
        format!("{base}frame.html"),
    ];
    requests_until(
        &mut chrome,
        "both frames' first documents answered",
        |requests| {
            first
                .iter()
                .all(|url| request(requests, url).is_some_and(|request| request.finished))
        },
    );
    // Into the page's process: the frame of `localhost` is sent to the page's own site. Out of it:
    // the frame of the page's site is sent to `localhost`. Each document's head and first bytes
    // come, so it commits in the process it goes to (the first frame's session ends), and its
    // rest is held.
    let back = format!("{base}slow.html?back");
    let out = format!("http://localhost:{port}/slow.html?out");
    chrome
        .eval(&format!(
            "document.getElementById('back').src = {}, document.getElementById('out').src = {}, 0",
            json!(back),
            json!(out)
        ))
        .unwrap();
    arrive(
        &mut chrome,
        &framing,
        &["/slow.html?back", "/slow.html?out"],
    );
    let swapped = |chrome: &Chrome| {
        chrome
            .events()
            .iter()
            .any(|event| event["method"] == "Target.detachedFromTarget")
    };
    requests_until(
        &mut chrome,
        "both documents answered in their new processes",
        |requests| {
            [&back, &out]
                .iter()
                .all(|url| request(requests, url).is_some_and(|request| request.status == 200))
        },
    );
    test_time::until_true(
        "the frame of localhost leaving its process",
        test_time::backstop(PROTOCOL_CPU),
        || {
            chrome.pump(POLL).unwrap();
            swapped(&chrome)
        },
    )
    .unwrap_or_else(|stall| panic!("{stall}"));
    // However its session ended, each frame still loads its document.
    let requests = chrome.requests().unwrap();
    for url in [&back, &out] {
        assert!(still_open(&requests, url), "{url}: {requests:#?}");
    }
    for _ in [&back, &out] {
        framing.release.send(()).unwrap();
    }
    let requests = requests_until(&mut chrome, "both documents' rest arrived", |requests| {
        [&back, &out]
            .iter()
            .all(|url| request(requests, url).is_some_and(|request| request.finished))
    });
    for url in [&back, &out] {
        let document = request(&requests, url).unwrap();
        assert_eq!(
            (document.frame.as_deref(), document.failed.as_deref()),
            (Some(url.as_str()), None),
            "{document:?}"
        );
    }

    // Removed while its document's rest is held, a frame of its own process leaves it open no more.
    let gone = format!("http://localhost:{port}/slow.html?gone");
    chrome
        .eval(&format!(
            "(() => {{ const frame = document.createElement('iframe'); frame.id = 'gone'; \
             frame.src = {}; document.body.append(frame); return 0; }})()",
            json!(gone)
        ))
        .unwrap();
    arrive(&mut chrome, &framing, &["/slow.html?gone"]);
    requests_until(
        &mut chrome,
        "the removed frame's document answered",
        |requests| request(requests, &gone).is_some_and(|request| request.status == 200),
    );
    chrome
        .eval("document.getElementById('gone').remove(), 0")
        .unwrap();
    let requests = requests_until(
        &mut chrome,
        "the removed frame's document given up",
        |requests| !still_open(requests, &gone),
    );
    let document = request(&requests, &gone).unwrap();
    assert!(document.canceled && !document.finished, "{document:?}");
}

/// A loopback server of a page whose `/held` requests are answered only once the test sends on
/// the channel, one send each; the channel it tells of each `/held` request as it arrives on.
fn holding_server() -> (
    String,
    std::sync::mpsc::Sender<()>,
    std::sync::mpsc::Receiver<()>,
) {
    use std::io::{BufRead, Write};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/", listener.local_addr().unwrap());
    let (release, released) = std::sync::mpsc::channel::<()>();
    let (arrive, arrived) = std::sync::mpsc::channel::<()>();
    let released = std::sync::Arc::new(std::sync::Mutex::new(released));
    std::thread::spawn(move || {
        for connection in listener.incoming() {
            let Ok(connection) = connection else {
                return;
            };
            let (released, arrive) = (released.clone(), arrive.clone());
            std::thread::spawn(move || {
                let mut reader = std::io::BufReader::new(connection.try_clone().unwrap());
                let mut head = String::new();
                while reader.read_line(&mut head).is_ok_and(|read| read > 2) {}
                let body = if head.starts_with("GET /held ") {
                    let _ = arrive.send(());
                    if released.lock().unwrap().recv().is_err() {
                        return;
                    }
                    "held"
                } else {
                    "<!doctype html><p>held page</p>"
                };
                let _ = write!(
                    &connection,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nCache-Control: no-store\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            });
        }
    });
    (base, release, arrived)
}

/// Block the page's renderer in a synchronous request of `/held` from its next task on, the way
/// Chrome blocks it on its own work: no task and no reply to the driver until it is answered.
/// `window.held` counts the requests answered.
fn block_on_held(chrome: &mut Chrome, arrived: &std::sync::mpsc::Receiver<()>) {
    chrome
        .eval(
            "setTimeout(() => { const request = new XMLHttpRequest(); \
             request.open('GET', '/held', false); request.send(); \
             window.held = (window.held || 0) + 1; }, 0); 0",
        )
        .unwrap();
    test_time::until(
        "the page asks for /held",
        test_time::backstop(Duration::from_secs(10)),
        || arrived.try_recv().ok(),
    )
    .unwrap_or_else(|stall| panic!("{stall}"));
}

#[test]
fn a_real_page_blocked_in_its_renderer_spends_nothing_and_a_stopped_browser_ends_at_the_backstop() {
    let Some(binary) = find() else {
        eprintln!("no Chrome: real browser capability unavailable");
        return;
    };
    let (base, release, arrived) = holding_server();
    let mut chrome = Chrome::launch(&binary, &leash()).unwrap();
    chrome.open(&base).unwrap();

    // The page's renderer blocked for 3 s while the rest of the browser runs on: two and a half
    // times the 48 turns of a 1.2 s wait, which observer turns alone would spend. Only the turns
    // the page takes part in count, so the wait sees it run again and meet its condition.
    //
    // The turns are what this wait proves, so it keeps the 48 turns `wait_for` gives 1.2 s and
    // takes the protocol's CPU allowance. Chrome's own work in its first seconds after a page (a
    // renderer made and others retired, the browser busy: more than a second of CPU on a loaded
    // machine) lands in any wait begun then, and 1.2 s of browser CPU would bound that work,
    // not the page's turns (RULES.md `tests/no-test-fails-on-load`).
    block_on_held(&mut chrome, &arrived);
    let mut budget = chrome.page_budget(Duration::from_millis(1200)).unwrap();
    assert_eq!(budget.max_polls, 48);
    budget.cpu = process::Budget::new(PROTOCOL_CPU, &chrome.owner.members().unwrap());
    let started = Instant::now();
    let late = release.clone();
    let releaser = std::thread::spawn(move || {
        #[expect(
            clippy::disallowed_methods,
            reason = "no-timed-waits baseline: a test releases what it holds after a length of time"
        )]
        std::thread::sleep(Duration::from_secs(3));
        late.send(()).unwrap();
    });
    let met = chrome
        .eval_bounded("window.held === 1", &mut budget)
        .unwrap();
    let took = started.elapsed();
    let passed = budget.page.as_ref().unwrap().passed;
    assert_eq!(
        met,
        Some(json!(true)),
        "the blocked page's wait ended in {took:?}, {} of its {} turns counted and {passed} passed \
         without the page",
        budget.polls,
        budget.max_polls
    );
    assert!(took >= Duration::from_secs(3), "met in {took:?}");
    releaser.join().unwrap();

    // The whole browser stopped: no turn completes and no CPU is spent, which no allowance
    // ends. The backstop does, naming the stall, and the page answers once it runs again.
    chrome.backstop = Duration::from_millis(1500);
    let group: u32 = std::fs::read_to_string(chrome.owner.profile.join("browser.pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    signal_group(group, libc::SIGSTOP).unwrap();
    let started = Instant::now();
    let stalled = chrome.wait_for("window.held === 1", Duration::from_millis(250));
    let took = started.elapsed();
    signal_group(group, libc::SIGCONT).unwrap();
    let stalled = stalled.unwrap_err();
    assert!(stalled.stalled(), "{stalled}");
    assert!(
        stalled.to_string().starts_with(
            "window.held === 1: the page under test stalled: its renderer took part in 0 of 0 \
             observer turns"
        ),
        "{stalled}"
    );
    // A wait is a stall only when the backstop is the check that ended it (`End::Backstop`), so
    // the frozen renderer, not the 250 ms allowance, ended this one; how much later than the
    // backstop a loaded machine let it end is no part of the promise (RULES.md
    // `tests/no-test-fails-on-load`).
    assert!(
        took >= Duration::from_millis(1500),
        "the stall ended {took:?} after the wait's start, its backstop 1.5 s"
    );
    chrome.backstop = BACKSTOP_FLOOR;
    assert_eq!(chrome.eval("window.held").unwrap(), json!(1));
}

#[test]
fn a_real_page_navigating_under_a_wait_is_asked_again_on_the_document_it_went_to() {
    let Some(binary) = find() else {
        eprintln!("no Chrome: real browser capability unavailable");
        return;
    };
    let (base, _release, _arrived) = holding_server();
    let mut chrome = Chrome::launch(&binary, &leash()).unwrap();
    // A condition false on the first document, where it navigates the page and stays pending, so
    // its evaluation is surely in flight as the page leaves: for a document of the page's own
    // site (Chrome answers `Execution context was destroyed.` where the page keeps its frame
    // host, `Inspected target navigated or closed` where it keeps the page it left for Back and
    // gives the next a new one), and for one of another site, in another process (the latter).
    for to in [
        format!("{base}second"),
        format!("{}second", base.replace("127.0.0.1", "localhost")),
    ] {
        chrome.open(&format!("{base}first")).unwrap();
        let to = json!(to);
        assert!(
            chrome
                .wait_for(
                    &format!(
                        "location.href === {to} || \
                         new Promise(() => setTimeout(() => {{ location.href = {to}; }}))"
                    ),
                    PROTOCOL_CPU
                )
                .unwrap(),
            "{to}"
        );
        assert_eq!(chrome.eval("location.href").unwrap(), to);
    }
    // An evaluation on its own is asked once: the navigation it started is its error.
    chrome.open(&format!("{base}first")).unwrap();
    let lost = chrome
        .eval("new Promise(() => setTimeout(() => { location.href = '/second'; }))")
        .unwrap_err();
    assert!(lost.navigated, "{lost}");
}

/// A DevTools connection of its own to `chrome`'s browser, and a thread that, once the page under
/// test has asked for `/held` (`arrived`, as [`holding_server`] tells of it), so that a wait's
/// evaluation asking for it is surely in flight, ends that page by `how`: `Target.closeTarget`, or
/// `Page.crash` on a session of its own. The connection and that session are made here, so that
/// their failure is the test's at once, before any wait. The thread ends the page even when the
/// arrival stalls, and then fails with the stall: left running, the page would hold the wait under
/// it to its allowance, and the test, which joins the thread ([`ended`]) before it judges the wait,
/// would report the wait. The end's reply is not waited for: a crashed renderer answers none. The
/// connection, to keep until the wait is over, and the arrivals back.
fn end_the_page_once_held(
    chrome: &Chrome,
    arrived: std::sync::mpsc::Receiver<()>,
    how: &'static str,
) -> std::thread::JoinHandle<(
    std::sync::mpsc::Receiver<()>,
    WebSocket<MaybeTlsStream<TcpStream>>,
)> {
    let (port, path) = devtools_endpoint(chrome.profile()).unwrap();
    let target = chrome.own_pages[1].clone();
    let backstop = test_time::backstop(Duration::from_secs(10));
    let (mut socket, _) = tungstenite::connect(format!("ws://127.0.0.1:{port}{path}")).unwrap();
    if let MaybeTlsStream::Plain(stream) = socket.get_mut() {
        stream.set_read_timeout(Some(backstop)).unwrap();
    }
    let send = |socket: &mut WebSocket<_>, command: Value| {
        socket.send(Message::text(command.to_string())).unwrap();
    };
    let end = if how == "Page.crash" {
        send(
            &mut socket,
            json!({"id": 1, "method": "Target.attachToTarget",
                "params": {"targetId": target, "flatten": true}}),
        );
        let session = loop {
            let Message::Text(text) = socket.read().unwrap() else {
                continue;
            };
            let reply: Value = serde_json::from_str(text.as_str()).unwrap();
            if reply["id"] == 1 {
                break reply["result"]["sessionId"].as_str().unwrap().to_owned();
            }
        };
        json!({"id": 2, "method": how, "params": {}, "sessionId": session})
    } else {
        json!({"id": 1, "method": how, "params": {"targetId": target}})
    };
    std::thread::spawn(move || {
        let held = test_time::until("the page asks for /held", backstop, || {
            arrived.try_recv().ok()
        });
        send(&mut socket, end);
        held.unwrap_or_else(|stall| panic!("{stall}"));
        (arrived, socket)
    })
}

/// What the thread `ender` returned. Its panic, if it panicked, is re-raised here, so that the test
/// reports the thread's own failure rather than what came of the wait the thread was to end.
fn ended<T>(ender: std::thread::JoinHandle<T>) -> T {
    ender
        .join()
        .unwrap_or_else(|failure| std::panic::resume_unwind(failure))
}

#[test]
fn a_real_page_closed_or_crashed_under_a_wait_fails_it_at_once_saying_which() {
    let Some(binary) = find() else {
        eprintln!("no Chrome: real browser capability unavailable");
        return;
    };
    let (base, _release, arrived) = holding_server();
    let mut chrome = Chrome::launch(&binary, &leash()).unwrap();
    // A condition whose evaluation asks for /held, which the server never answers: Chrome answers
    // it only with the page's end. Without that, the crash would run to the backstop (six minutes
    // for the protocol's allowance) and the close would spend the allowance on the probes Chrome
    // refuses, each a turn of the page's.
    const HELD: &str = "fetch('/held').then(() => false)";
    chrome.open(&base).unwrap();
    let ender = end_the_page_once_held(&chrome, arrived, "Page.crash");
    let crashed = chrome.wait_for(HELD, PROTOCOL_CPU);
    // The ender's failure, if it failed, is the test's, before the wait's is judged.
    let (arrived, crasher) = ended(ender);
    let crashed = crashed.unwrap_err();
    assert!(!crashed.stalled() && !crashed.past_deadline(), "{crashed}");
    assert_eq!(
        crashed.to_string(),
        format!("{HELD}: the page under test's renderer crashed")
    );
    // A wait begun on the crashed page fails at once too; going to a document again starts another
    // renderer, which Chrome tells of before the document loads.
    assert_eq!(
        chrome
            .wait_for("true", PROTOCOL_CPU)
            .unwrap_err()
            .to_string(),
        "true: the page under test's renderer crashed"
    );
    chrome.open(&base).unwrap();
    assert!(
        chrome
            .wait_for("document.readyState !== 'loading'", PROTOCOL_CPU)
            .unwrap()
    );
    drop(crasher);

    let ender = end_the_page_once_held(&chrome, arrived, "Target.closeTarget");
    let closed = chrome.wait_for(HELD, PROTOCOL_CPU);
    let (_arrived, _closer) = ended(ender);
    let closed = closed.unwrap_err();
    assert!(!closed.stalled() && !closed.past_deadline(), "{closed}");
    assert_eq!(
        closed.to_string(),
        format!("{HELD}: the page under test closed: its session detached")
    );
    assert_eq!(
        chrome
            .wait_for("true", PROTOCOL_CPU)
            .unwrap_err()
            .to_string(),
        "true: the page under test closed: its session detached"
    );
}

/// Two more visits to a page's own URL after the first, in a browser launched as `binary`, traced:
/// the frames its renderers made for them, and the compositors of left documents they stopped.
/// Chrome replaces a page left for its own URL, never keeping it for Back, as the client's tests
/// that open one page twice see it.
fn same_url_visits(binary: &str, base: &str) -> (usize, usize) {
    let mut chrome = Chrome::launch(binary, &leash()).unwrap();
    chrome.open(&format!("{base}first")).unwrap();
    chrome
        .browser_call(
            "Tracing.start",
            json!({"transferMode": "ReturnAsStream", "traceConfig": {
                "includedCategories": ["navigation", "cc"]
            }}),
        )
        .unwrap();
    for _ in 0..2 {
        chrome.open(&format!("{base}first")).unwrap();
    }
    // A left document's compositor stops a second after the visit that left it.
    chrome.pump(Duration::from_millis(2500)).unwrap();
    chrome.browser_call("Tracing.end", json!({})).unwrap();
    let complete = chrome
        .wait_for_protocol_events("Tracing.tracingComplete", 1, PROTOCOL_CPU)
        .unwrap();
    let stream = complete[0]["stream"].as_str().unwrap().to_owned();
    let mut trace = String::new();
    loop {
        let read = chrome
            .browser_call("IO.read", json!({"handle": stream, "size": 1 << 20}))
            .unwrap();
        assert_ne!(
            read["base64Encoded"],
            json!(true),
            "a JSON trace comes as text"
        );
        trace.push_str(read["data"].as_str().unwrap_or(""));
        if read["eof"] != json!(false) {
            break;
        }
    }
    chrome
        .browser_call("IO.close", json!({"handle": stream}))
        .unwrap();
    let trace: Value = serde_json::from_str(&trace).unwrap();
    let events = trace["traceEvents"].as_array().unwrap();
    let count = |name: &str, phase: &str| {
        events
            .iter()
            .filter(|event| event["name"] == name && event["ph"] == phase)
            .count()
    };
    (count("CreateFrame", "b"), count("ProxyMain::Stop", "X"))
}

#[test]
fn a_visit_to_its_own_url_keeps_the_pages_frame_so_no_left_document_stops_a_compositor() {
    let Some(binary) = find() else {
        eprintln!("no Chrome: real browser capability unavailable");
        return;
    };
    let (base, _release, _arrived) = holding_server();
    assert_eq!(
        same_url_visits(&binary, &base),
        (0, 0),
        "the driver's browser made frames for the visits, or stopped a left document's compositor"
    );

    // The same browser with the features turned off before RenderDocument was: the last list
    // given is the one Chrome reads.
    struct Scratch(PathBuf);
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let scratch = Scratch(next_profile().unwrap());
    std::fs::create_dir(&scratch.0).unwrap();
    let before = scratch.0.join("browser");
    std::fs::write(
        &before,
        format!(
            "#!/bin/sh\nexec '{binary}' \"$@\" --disable-features=Translate,OptimizationHints,MediaRouter\n"
        ),
    )
    .unwrap();
    std::fs::set_permissions(&before, std::fs::Permissions::from_mode(0o700)).unwrap();
    let (frames, stopped) = same_url_visits(before.to_str().unwrap(), &base);
    eprintln!("without the flag: {frames} frames made, {stopped} compositors stopped");
    assert!(
        frames >= 1,
        "the visits made no frame without the flag: it no longer matters"
    );
}

#[test]
fn cpu_exhaustion_stops_renderer_work_before_the_poll_allowance() {
    use tungstenite::protocol::Role;

    let Some(binary) = find() else {
        eprintln!("no Chrome: real browser capability unavailable");
        return;
    };
    let mut chrome = Chrome::launch(&binary, &leash()).unwrap();
    chrome.open("data:text/html,<p>CPU fixture</p>").unwrap();
    chrome.eval("window.cpuPolls=0").unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (peer, _) = listener.accept().unwrap();
    let mut renderer = std::mem::replace(
        &mut chrome.socket,
        WebSocket::from_raw_socket(MaybeTlsStream::Plain(stream), Role::Client, None),
    );
    let observer = chrome.observer.clone().unwrap();
    let mut renderer_id = chrome.next_id;
    const WORK: &str = "(() => {window.cpuPolls++; let checksum=2166136261; for(let i=0;i<500000000;i++) {checksum=Math.imul(checksum ^ i,16777619);} window.cpuChecksum=checksum; return false;})()";
    let peer = std::thread::spawn(move || {
        let mut socket = WebSocket::from_raw_socket(peer, Role::Server, None);
        let mut evaluations = 0;
        while let Ok(Message::Text(text)) = socket.read() {
            let mut command: Value = serde_json::from_str(text.as_str()).unwrap();
            let id = command["id"].clone();
            if command["sessionId"] == observer {
                // Observer replies wait behind the renderer's fixed work in this
                // transport. They cannot spend the allowance while work is pending.
                socket
                    .send(Message::text(
                        json!({"id":id,"result":{"result":{"value":null}}}).to_string(),
                    ))
                    .unwrap();
                continue;
            }
            if command["params"]["expression"] == WORK {
                evaluations += 1;
                assert!(evaluations <= 4, "renderer exceeded its poll allowance");
            }
            renderer_id += 1;
            command["id"] = json!(renderer_id);
            renderer.send(Message::text(command.to_string())).unwrap();
            loop {
                let text = match renderer.read() {
                    Ok(Message::Text(text)) => text,
                    Ok(_) => continue,
                    Err(tungstenite::Error::Io(error))
                        if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
                    {
                        continue;
                    }
                    Err(error) => panic!("renderer relay: {error}"),
                };
                let mut reply: Value = serde_json::from_str(text.as_str()).unwrap();
                if reply["id"] == json!(renderer_id) {
                    reply["id"] = id;
                    socket.send(Message::text(reply.to_string())).unwrap();
                    break;
                }
            }
        }
        evaluations
    });
    assert!(!chrome.wait_for(WORK, Duration::from_millis(100)).unwrap());
    let polls = chrome.eval("window.cpuPolls").unwrap().as_u64().unwrap();
    assert!(
        (1..4).contains(&polls),
        "CPU must end the wait before four polls, got {polls}"
    );
    assert_eq!(
        chrome
            .eval("typeof window.cpuChecksum === 'number'")
            .unwrap(),
        json!(true)
    );
    drop(chrome);
    assert_eq!(peer.join().unwrap(), polls);
}

#[test]
fn requests_follow_each_id_through_cache_failure_and_cancellation() {
    let event = |method: &str, params: Value| json!({"method": method, "params": params});
    let events = [
        event(
            "Network.requestWillBeSent",
            json!({"requestId":"1","type":"Fetch","request":{"url":"http://a/api"}}),
        ),
        event(
            "Network.requestWillBeSent",
            json!({"requestId":"2","type":"Script","request":{"url":"http://a/s.js"}}),
        ),
        event(
            "Network.requestWillBeSent",
            json!({"requestId":"3","type":"Fetch","request":{"url":"http://a/gone"}}),
        ),
        event(
            "Network.requestWillBeSent",
            json!({"requestId":"4","type":"Fetch","request":{"url":"http://a/aborted"}}),
        ),
        // An event naming no request, and a request whose start was never seen, are left out.
        event("Page.loadEventFired", json!({})),
        event(
            "Network.loadingFinished",
            json!({"requestId":"9","encodedDataLength":5}),
        ),
        event(
            "Network.responseReceived",
            json!({"requestId":"1","response":{"status":404}}),
        ),
        event(
            "Network.dataReceived",
            json!({"requestId":"1","encodedDataLength":10}),
        ),
        event(
            "Network.loadingFinished",
            json!({"requestId":"1","encodedDataLength":12}),
        ),
        event("Network.requestServedFromCache", json!({"requestId":"2"})),
        event(
            "Network.loadingFailed",
            json!({"requestId":"3","errorText":"net::ERR_CONNECTION_REFUSED"}),
        ),
        event(
            "Network.loadingFailed",
            json!({"requestId":"4","errorText":"net::ERR_ABORTED","canceled":true}),
        ),
    ];
    let requests = crate::requests(&events, None);
    let urls: Vec<_> = requests
        .iter()
        .map(|request| request.url.as_str())
        .collect();
    assert_eq!(
        urls,
        [
            "http://a/api",
            "http://a/s.js",
            "http://a/gone",
            "http://a/aborted"
        ]
    );
    let [api, script, gone, aborted] = requests.as_slice() else {
        panic!("{requests:?}");
    };
    assert_eq!((api.status, api.bytes, api.finished), (404, 12.0, true));
    assert!(api.failed.is_none() && !api.canceled);
    assert!(script.from_cache && !script.finished && script.failed.is_none());
    assert_eq!(gone.failed.as_deref(), Some("net::ERR_CONNECTION_REFUSED"));
    assert!(!gone.canceled && !gone.finished);
    assert_eq!(aborted.failed.as_deref(), Some("net::ERR_ABORTED"));
    assert!(aborted.canceled);
}

/// An event as Chrome sends it on `session`.
fn on(session: &str, method: &str, params: Value) -> Value {
    json!({"sessionId": session, "method": method, "params": params})
}

/// `Network.requestWillBeSent` on `session` for request `id` of `kind` to `url`, made for the
/// frame `frame` (none for a worker's) and the document at `document`.
fn sent(
    session: &str,
    id: &str,
    kind: &str,
    frame: Option<&str>,
    document: &str,
    url: &str,
) -> Value {
    let mut params =
        json!({"requestId": id, "type": kind, "documentURL": document, "request": {"url": url}});
    if let Some(frame) = frame {
        params["frameId"] = json!(frame);
    }
    on(session, "Network.requestWillBeSent", params)
}

/// `Network.loadingFinished` on `session` for request `id`.
fn finished(session: &str, id: &str) -> Value {
    on(
        session,
        "Network.loadingFinished",
        json!({"requestId": id, "encodedDataLength": 10}),
    )
}

/// `Target.detachedFromTarget` for `session`, as the page's session tells of it.
fn detached(session: &str) -> Value {
    on(
        "page",
        "Target.detachedFromTarget",
        json!({"sessionId": session}),
    )
}

/// What [`crate::requests`] made of the request to `url` among `requests`: the frame it names,
/// whether it finished, why it failed and whether it was given up.
fn said<'a>(
    requests: &'a [crate::Request],
    url: &str,
) -> (Option<&'a str>, bool, Option<&'a str>, bool) {
    let request = requests
        .iter()
        .find(|request| request.url == url)
        .unwrap_or_else(|| panic!("{url} in {requests:#?}"));
    (
        request.frame.as_deref(),
        request.finished,
        request.failed.as_deref(),
        request.canceled,
    )
}

#[test]
fn a_frames_requests_are_named_by_its_document_and_its_documents_end_with_its_session() {
    let mock = "http://b/mock.html";
    let events = [
        // The page's own document, and a fetch of its own still open at the end.
        sent(
            "page",
            "main",
            "Document",
            Some("top"),
            "http://a/",
            "http://a/",
        ),
        finished("page", "main"),
        sent(
            "page",
            "api",
            "Fetch",
            Some("top"),
            "http://a/",
            "http://a/api",
        ),
        // A frame in a process of its own: its document starts on the page's session and is
        // answered on the frame's own.
        sent("page", "mock", "Document", Some("inner"), mock, mock),
        on(
            "frame",
            "Network.responseReceived",
            json!({"requestId": "mock", "response": {"status": 200}}),
        ),
        finished("frame", "mock"),
        // Its style, which finished, and its picture, still open when the frame's session ends.
        sent(
            "frame",
            "style",
            "Stylesheet",
            Some("inner"),
            mock,
            "http://b/s.css",
        ),
        finished("frame", "style"),
        sent(
            "frame",
            "picture",
            "Image",
            Some("inner"),
            mock,
            "http://b/p.png",
        ),
        // A frame in the page's process, told of on the page's session alone, and a worker's
        // request, made for no frame.
        sent(
            "page",
            "near",
            "Fetch",
            Some("near"),
            "about:srcdoc",
            "http://a/near",
        ),
        sent(
            "page",
            "worker",
            "Fetch",
            None,
            "http://a/w.js",
            "http://a/w",
        ),
        // Another session ending leaves the frame's requests as they are; the frame's ends what its
        // document made.
        detached("elsewhere"),
        detached("frame"),
    ];
    let requests = crate::requests(&events, Some("top"));
    assert_eq!(said(&requests, "http://a/"), (None, true, None, false));
    assert_eq!(said(&requests, "http://a/api"), (None, false, None, false));
    assert_eq!(said(&requests, mock), (Some(mock), true, None, false));
    assert_eq!(
        requests
            .iter()
            .find(|request| request.url == mock)
            .unwrap()
            .status,
        200
    );
    assert_eq!(
        said(&requests, "http://b/s.css"),
        (Some(mock), true, None, false)
    );
    assert_eq!(
        said(&requests, "http://b/p.png"),
        (Some(mock), false, Some(crate::DOCUMENT_GONE), true)
    );
    assert_eq!(
        said(&requests, "http://a/near"),
        (Some("about:srcdoc"), false, None, false)
    );
    assert_eq!(said(&requests, "http://a/w"), (None, false, None, false));
    // Before the page's first navigation no frame is known as its own: none is named.
    assert!(
        crate::requests(&events, None)
            .iter()
            .all(|request| request.frame.is_none())
    );
}

#[test]
fn a_frame_moving_between_processes_is_followed_by_its_id_and_only_its_removal_gives_it_up() {
    let back = "http://a/back.html";
    let out = "http://c/out.html";
    let slow = "http://b/slow.html";
    let deep = "http://d/deep.html";
    let mut events = vec![
        // Into the page's process: a frame of another site, in a process of its own (session
        // `away`), navigates to the page's site. Its document starts on its own session, which
        // ends as the document commits in the page's process; its picture there ends with it.
        sent(
            "away",
            "picture",
            "Image",
            Some("back"),
            "http://b/x.html",
            "http://b/x.png",
        ),
        sent("away", "back", "Document", Some("back"), back, back),
        on(
            "away",
            "Network.responseReceived",
            json!({"requestId": "back", "response": {"status": 200}}),
        ),
        on(
            "page",
            "Page.frameDetached",
            json!({"frameId": "back", "reason": "swap"}),
        ),
        detached("away"),
        // Out of it: a frame in the page's process navigates to another site. Its document starts
        // on the page's session, the frame leaves the page's process, and the document is answered
        // on the session of the process it goes to (`out`).
        sent("page", "out", "Document", Some("out"), out, out),
        on(
            "page",
            "Page.frameDetached",
            json!({"frameId": "out", "reason": "swap"}),
        ),
        on(
            "out",
            "Network.responseReceived",
            json!({"requestId": "out", "response": {"status": 200}}),
        ),
        // Removed: a frame whose document is still arriving, told of on the page's session alone,
        // and, nested in it, a frame (of a process of its own) whose document is too.
        sent("page", "slow", "Document", Some("slow"), slow, slow),
        on(
            "slow",
            "Page.frameAttached",
            json!({"frameId": "deep", "parentFrameId": "slow"}),
        ),
        sent("slow", "deep", "Document", Some("deep"), deep, deep),
        // A frame of the page's own removed with nothing open, and the frame the page holds.
        on(
            "page",
            "Page.frameAttached",
            json!({"frameId": "slow", "parentFrameId": "top"}),
        ),
        on(
            "page",
            "Page.frameDetached",
            json!({"frameId": "empty", "reason": "remove"}),
        ),
    ];
    // Until each document is answered or its frame removed, each frame still loads.
    let open = crate::requests(&events, Some("top"));
    for url in [back, out, slow, deep] {
        assert_eq!(said(&open, url), (Some(url), false, None, false), "{url}");
    }
    assert_eq!(
        said(&open, "http://b/x.png"),
        (
            Some("http://b/x.html"),
            false,
            Some(crate::DOCUMENT_GONE),
            true
        )
    );
    events.extend([
        finished("page", "back"),
        finished("out", "out"),
        on(
            "page",
            "Page.frameDetached",
            json!({"frameId": "slow", "reason": "remove"}),
        ),
    ]);
    let ended = crate::requests(&events, Some("top"));
    assert_eq!(said(&ended, back), (Some(back), true, None, false));
    assert_eq!(said(&ended, out), (Some(out), true, None, false));
    // The removed frame's document, and the one of the frame within it, are given up.
    for url in [slow, deep] {
        assert_eq!(
            said(&ended, url),
            (Some(url), false, Some(crate::FRAME_REMOVED), true),
            "{url}"
        );
    }
}

/// `Page.frameNavigated` on `session`: `frame`, within `parent` (none for the page's own), went to
/// the document of loader `loader`.
fn navigated(session: &str, frame: &str, parent: Option<&str>, loader: &str) -> Value {
    let mut told = json!({"id": frame, "loaderId": loader, "url": format!("http://{frame}/")});
    if let Some(parent) = parent {
        told["parentId"] = json!(parent);
    }
    on(session, "Page.frameNavigated", json!({"frame": told}))
}

/// `Page.frameAttached` on `session`: `frame` within `parent`.
fn attached_frame(session: &str, frame: &str, parent: &str) -> Value {
    on(
        session,
        "Page.frameAttached",
        json!({"frameId": frame, "parentFrameId": parent}),
    )
}

#[test]
fn a_frame_going_on_to_another_document_takes_the_frames_its_document_held_along() {
    let inner = "http://c/inner.html";
    let events = [
        // A frame of another site (`b`, in a process of its own, session `bs`) holds a frame of a
        // third (`c`, session `cs`) whose document still arrives, with a picture of its own open.
        attached_frame("page", "b", "top"),
        navigated("bs", "b", Some("top"), "b1"),
        attached_frame("bs", "c", "b"),
        sent("bs", "c1", "Document", Some("c"), inner, inner),
        on(
            "cs",
            "Network.responseReceived",
            json!({"requestId": "c1", "response": {"status": 200}}),
        ),
        sent("cs", "picture", "Image", Some("c"), inner, "http://c/p.png"),
        // `b` goes on to a document of the page's own site, in the page's process: its session
        // ends, and no removal of `c` is told of.
        sent(
            "bs",
            "b2",
            "Document",
            Some("b"),
            "http://a/b2.html",
            "http://a/b2.html",
        ),
        detached("bs"),
        attached_frame("page", "b", "top"),
        navigated("page", "b", Some("top"), "b2"),
        // Its new document holds a frame of its own, still loading.
        attached_frame("page", "d", "b"),
        sent(
            "page",
            "d1",
            "Document",
            Some("d"),
            "http://a/d.html",
            "http://a/d.html",
        ),
    ];
    let requests = crate::requests(&events, Some("top"));
    // What `c` held went with `b`'s document, whatever kind.
    assert_eq!(
        said(&requests, inner),
        (Some(inner), false, Some(crate::FRAME_REMOVED), true)
    );
    assert_eq!(
        said(&requests, "http://c/p.png"),
        (Some(inner), false, Some(crate::FRAME_REMOVED), true)
    );
    // `b` itself, and the frame its new document holds, still load.
    for url in ["http://a/b2.html", "http://a/d.html"] {
        assert_eq!(
            said(&requests, url),
            (Some(url), false, None, false),
            "{url}"
        );
    }
}

#[test]
fn a_frames_document_still_arriving_is_given_up_once_the_frame_goes_on_to_another() {
    let [one, two, three] = ["http://a/1.html", "http://a/2.html", "http://a/3.html"];
    let document =
        |id: &str, frame: &str, url: &str| sent("page", id, "Document", Some(frame), url, url);
    let mut events = vec![
        // `f`'s first document is answered, its body still arriving, when the page sends it to a
        // second, then a third before the second's arrival is told of.
        document("f1", "f", one),
        on(
            "page",
            "Network.responseReceived",
            json!({"requestId": "f1", "response": {"status": 200}}),
        ),
        navigated("page", "f", Some("top"), "f1"),
        document("f2", "f", two),
        document("f3", "f", three),
        navigated("page", "f", Some("top"), "f2"),
        // `g` goes on to a `srcdoc` document, no request of its own, leaving its first behind; what
        // its new one asks for after goes on.
        document("g1", "g", "http://a/g.html"),
        navigated("page", "g", Some("top"), "srcdoc"),
        sent(
            "page",
            "g-picture",
            "Image",
            Some("g"),
            "about:srcdoc",
            "http://a/g.png",
        ),
        // `h`'s first document finished before it went on: it stays finished.
        document("h1", "h", "http://a/h.html"),
        finished("page", "h1"),
        document("h2", "h", "http://a/h2.html"),
        navigated("page", "h", Some("top"), "h2"),
    ];
    let requests = crate::requests(&events, Some("top"));
    assert_eq!(
        said(&requests, one),
        (Some(one), false, Some(crate::DOCUMENT_LEFT), true)
    );
    // The document it went to, and a navigation started after it, still load.
    for url in [two, three, "http://a/h2.html"] {
        assert_eq!(
            said(&requests, url),
            (Some(url), false, None, false),
            "{url}"
        );
    }
    assert_eq!(
        said(&requests, "http://a/g.html"),
        (
            Some("http://a/g.html"),
            false,
            Some(crate::DOCUMENT_LEFT),
            true
        )
    );
    assert_eq!(
        said(&requests, "http://a/g.png"),
        (Some("about:srcdoc"), false, None, false)
    );
    assert_eq!(
        said(&requests, "http://a/h.html"),
        (Some("http://a/h.html"), true, None, false)
    );
    // The third arriving leaves the second behind in turn.
    events.push(navigated("page", "f", Some("top"), "f3"));
    let requests = crate::requests(&events, Some("top"));
    assert_eq!(
        said(&requests, two),
        (Some(two), false, Some(crate::DOCUMENT_LEFT), true)
    );
    assert_eq!(said(&requests, three), (Some(three), false, None, false));
}

/// `Target.attachedToTarget` on `on` for target `target` of `kind`, on session `session`, its
/// address `url`.
fn started(on: &str, session: &str, target: &str, kind: &str, url: &str) -> Value {
    json!({"method": "Target.attachedToTarget", "sessionId": on, "params": {
        "sessionId": session, "waitingForDebugger": true,
        "targetInfo": {"targetId": target, "type": kind, "url": url}}})
}

/// `Target.targetCreated` on the browser's own session for target `target` of `kind`, its address
/// `url`: told of by discovery, attached to nothing.
fn created(target: &str, kind: &str, url: &str) -> Value {
    json!({"method": "Target.targetCreated", "params": {
        "targetInfo": {"targetId": target, "type": kind, "url": url, "attached": false}}})
}

#[test]
fn a_followed_workers_requests_are_its_own_by_its_script_and_a_discovered_shared_workers_script_is_given_up()
 {
    let [dedicated, framed, late, shared, service] = [
        "http://a/w.js",
        "http://b/v.js",
        "http://a/late.js",
        "http://a/shared.js",
        "http://a/sw.js",
    ];
    // The page fetching its own address: its document's, by its loader, never a worker's script.
    let mut itself = sent(
        "page",
        "self",
        "Fetch",
        Some("top"),
        "http://a/",
        "http://a/",
    );
    itself["params"]["loaderId"] = json!("L");
    let events = [
        // A dedicated worker of the page: its script starts on the page's session, made for the
        // page's frame and named by the worker's target, and ends on the worker's own.
        sent("page", "W", "Script", Some("top"), dedicated, dedicated),
        started("page", "ws", "W", "worker", dedicated),
        finished("ws", "W"),
        sent("ws", "data", "Fetch", None, dedicated, "http://a/data.json"),
        // One of a frame in a process of its own (session `fs`): made for the frame, still a
        // worker's, never the frame named by the worker's script.
        sent("fs", "V", "Script", Some("inner"), framed, framed),
        started("fs", "vs", "V", "worker", framed),
        // A dedicated worker whose script's answer has not started: no target names it yet (it
        // attaches only then), and a target of another kind told of by its id names nothing.
        sent("page", "L", "Script", Some("top"), late, late),
        created("L", "worker", late),
        // A shared worker, which the browser runs and nothing follows: discovered, by the target
        // its script's request is named by, its script starts on the page's session, and its
        // answer goes to the worker.
        created("S", "shared_worker", shared),
        sent("page", "S", "Script", Some("top"), shared, shared),
        itself,
        // A service worker, not followed: its requests are its page's, as before.
        started("page", "sws", "SW", "service_worker", service),
        sent("sws", "cached", "Fetch", None, service, "http://a/cached"),
        // The dedicated worker ends with its fetch still open.
        detached("ws"),
    ];
    let requests = crate::requests(&events, Some("top"));
    let whose = |url: &str| {
        let request = requests
            .iter()
            .find(|request| request.url == url)
            .unwrap_or_else(|| panic!("{url} in {requests:#?}"));
        (
            request.worker.as_deref(),
            request.frame.as_deref(),
            request.finished,
            request.failed.as_deref(),
            request.canceled,
        )
    };
    assert_eq!(whose(dedicated), (Some(dedicated), None, true, None, false));
    assert_eq!(
        whose("http://a/data.json"),
        (
            Some(dedicated),
            None,
            false,
            Some(crate::WORKER_ENDED),
            true
        )
    );
    assert_eq!(whose(framed), (Some(framed), None, false, None, false));
    // Still open: waited for.
    assert_eq!(whose(late), (None, None, false, None, false));
    assert_eq!(
        whose(shared),
        (None, None, false, Some(crate::SHARED_WORKER_SCRIPT), true)
    );
    assert_eq!(whose("http://a/"), (None, None, false, None, false));
    assert_eq!(whose("http://a/cached"), (None, None, false, None, false));
}

#[test]
fn a_prefetch_is_named_by_its_purpose_header_and_served_from_the_prefetch_cache_counts_as_cached() {
    let event = |method: &str, params: Value| json!({"method": method, "params": params});
    let events = [
        event(
            "Network.requestWillBeSent",
            json!({"requestId": "warm", "type": "Other", "request": {"url": "http://h/p.wasm", "headers": {}}}),
        ),
        event(
            "Network.requestWillBeSentExtraInfo",
            json!({"requestId": "warm", "headers": {"Sec-Purpose": "prefetch"}}),
        ),
        event(
            "Network.requestWillBeSent",
            json!({"requestId": "use", "type": "Fetch", "request": {"url": "http://h/p.wasm", "headers": {"Purpose": "other"}}}),
        ),
        event(
            "Network.responseReceived",
            json!({"requestId": "use", "response": {"status": 200, "fromPrefetchCache": true}}),
        ),
    ];
    let requests = crate::requests(&events, None);
    let [warm, used] = requests.as_slice() else {
        panic!("two requests: {requests:#?}")
    };
    assert!(warm.prefetch && !warm.from_cache);
    assert!(!used.prefetch && used.from_cache);
}

#[test]
fn an_open_stream_counts_its_head_and_every_chunk_read_and_names_its_pings() {
    // As Chrome reported a stand-in feed: the head, `: open`, two pings and a delta. Each
    // chunk's encodedDataLength is the previous chunk's, so it never names the last one.
    let event = |method: &str, params: Value| json!({"method": method, "params": params});
    let chunk = |data: u32, encoded: u32| {
        event(
            "Network.dataReceived",
            json!({"requestId": "feed", "dataLength": data, "encodedDataLength": encoded}),
        )
    };
    let message = |name: &str| {
        event(
            "Network.eventSourceMessageReceived",
            json!({"requestId": "feed", "eventName": name, "data": ""}),
        )
    };
    let events = [
        event(
            "Network.requestWillBeSent",
            json!({"requestId": "feed", "type": "EventSource", "request": {"url": "http://h/api/feed"}}),
        ),
        event(
            "Network.requestWillBeSent",
            json!({"requestId": "glue", "type": "Script", "request": {"url": "http://h/a.js"}}),
        ),
        event(
            "Network.responseReceived",
            json!({"requestId": "feed", "response": {"status": 200, "encodedDataLength": 77}}),
        ),
        chunk(8, 0),
        event(
            "Network.dataReceived",
            json!({"requestId": "glue", "dataLength": 900, "encodedDataLength": 300}),
        ),
        event(
            "Network.loadingFinished",
            json!({"requestId": "glue", "encodedDataLength": 512}),
        ),
        chunk(19, 8),
        message(symphony_api::PING_EVENT),
        chunk(19, 19),
        message(symphony_api::PING_EVENT),
        chunk(45, 19),
        message(symphony_api::FEED_EVENT),
    ];
    let requests = crate::requests(&events, None);
    let [feed, glue] = requests.as_slice() else {
        panic!("two requests: {requests:#?}")
    };
    assert_eq!(
        (feed.kind.as_str(), feed.status, feed.bytes, feed.pings),
        ("EventSource", 200, 77.0 + 8.0 + 19.0 + 19.0 + 45.0, 2)
    );
    assert!(!feed.finished);
    assert_eq!((glue.bytes, glue.pings, glue.finished), (512.0, 0, true));
}

#[test]
fn a_websocket_counts_its_handshake_and_every_frame_and_names_its_pings() {
    let event = |method: &str, params: Value| json!({"method": method, "params": params});
    let frame = |id: &str, payload: &str| {
        event(
            "Network.webSocketFrameReceived",
            json!({"requestId": id, "response": {"opcode": 1, "mask": false, "payloadData": payload}}),
        )
    };
    let delta = format!(
        "{}\n{{\"version\":{}}}",
        symphony_api::FEED_EVENT,
        "9".repeat(200)
    );
    let ping = format!("{}\n", symphony_api::PING_EVENT);
    let events = [
        event(
            "Network.webSocketCreated",
            json!({"requestId": "feed", "url": "ws://h/api/feed?since=3"}),
        ),
        event(
            "Network.webSocketWillSendHandshakeRequest",
            json!({"requestId": "feed", "request": {"headers": {}}}),
        ),
        event(
            "Network.webSocketHandshakeResponseReceived",
            json!({"requestId": "feed", "response": {"status": 101, "statusText": "Switching Protocols", "headersText": "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n\r\n"}}),
        ),
        frame(
            "feed",
            &format!("{}\n\"Serving\"", symphony_api::SERVER_EVENT),
        ),
        frame("feed", &ping),
        frame("feed", &delta),
        frame("feed", &ping),
        // Another, told no text of its head, which is closed.
        event(
            "Network.webSocketCreated",
            json!({"requestId": "gone", "url": "ws://h/api/feed?since=4"}),
        ),
        event(
            "Network.webSocketHandshakeResponseReceived",
            json!({"requestId": "gone", "response": {"status": 101, "statusText": "Switching Protocols", "headers": {"Upgrade": "websocket", "Vary": "a\nb"}}}),
        ),
        frame("gone", &"x".repeat(70_000)),
        event("Network.webSocketClosed", json!({"requestId": "gone"})),
    ];
    let requests = crate::requests(&events, None);
    let [feed, gone] = requests.as_slice() else {
        panic!("two sockets: {requests:#?}")
    };
    let head = "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n\r\n".len();
    // A frame's head is 2 bytes up to 125 bytes of payload, 4 up to 65,535, 10 beyond.
    let frames = (2 + 16) + 2 * (2 + ping.len()) + (4 + delta.len());
    assert_eq!(
        (
            feed.url.as_str(),
            feed.kind.as_str(),
            feed.status,
            feed.bytes,
            feed.pings,
            feed.finished
        ),
        (
            "ws://h/api/feed?since=3",
            crate::WEB_SOCKET,
            101,
            (head + frames) as f64,
            2,
            false
        )
    );
    // Its status line, each header (a repeated one on each of its lines) and the blank line.
    let head = "HTTP/1.1 101 Switching Protocols\r\n".len()
        + "Upgrade: websocket\r\n".len()
        + "Vary: a\r\nVary: b\r\n".len()
        + 2;
    assert_eq!(
        (gone.bytes, gone.pings, gone.finished),
        ((head + 10 + 70_000) as f64, 0, true)
    );
}

#[test]
fn a_page_that_cannot_load_fails_navigate_naming_why_but_still_opens() {
    let Some(binary) = find() else {
        eprintln!("no Chrome: real browser capability unavailable");
        return;
    };
    // A port this test bound and let go: nothing listens there now.
    let refused = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        format!("http://{}/", listener.local_addr().unwrap())
    };
    let mut chrome = Chrome::launch(&binary, &leash()).unwrap();
    let why = chrome
        .navigate(&refused, PROTOCOL_CPU)
        .unwrap_err()
        .to_string();
    assert!(why.contains("ERR_CONNECTION_REFUSED"), "{why}");
    assert!(why.contains(&refused), "{why}");
    chrome.open(&refused).unwrap();

    // A server that takes the connection and never answers holds the page within the allowance.
    let silent = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let held = format!("http://{}/", silent.local_addr().unwrap());
    let holder = std::thread::spawn(move || {
        let (connection, _) = silent.accept().unwrap();
        connection
    });
    let started = Instant::now();
    let error = chrome
        .navigate(&held, Duration::from_millis(500))
        .unwrap_err();
    let why = error.to_string();
    assert!(
        why.contains("Page.navigate") || why.contains("Page.domContentEventFired"),
        "{why}"
    );
    // Its allowance ended the wait, counted in the page's turns, not the wall-clock backstop a
    // stall reaches: how long a loaded machine took to count them is no part of the promise
    // (RULES.md `tests/no-test-fails-on-load`).
    assert!(
        !error.stalled() && !error.past_deadline(),
        "a 500 ms allowance held the page for {:?}: {why}",
        started.elapsed()
    );
    drop(holder.join().unwrap());

    let navigated = Instant::now();
    chrome
        .navigate(
            "data:text/html,<p style='margin:0;height:40px;background:rgb(255,0,0)'>",
            PROTOCOL_CPU,
        )
        .unwrap();
    // This page's events, each with when it was read, in order; the earlier pages' are forgotten.
    let read: Vec<(Instant, &Value)> = chrome.events_read().collect();
    assert_eq!(read.len(), chrome.events().len());
    assert!(
        read.iter()
            .any(|(_, event)| event["method"] == "Page.domContentEventFired"),
        "{read:?}"
    );
    assert!(read.windows(2).all(|pair| pair[0].0 <= pair[1].0));
    let now = Instant::now();
    assert!(
        read.iter().all(|(at, _)| *at >= navigated && *at <= now),
        "{read:?}"
    );
    let picture = chrome
        .screenshot(json!({"clip": {"x": 0, "y": 0, "width": 30, "height": 20, "scale": 1}}))
        .unwrap();
    assert_eq!(&picture[..8], b"\x89PNG\r\n\x1a\n");
    // The IHDR chunk's width and height, big-endian, after its length and name.
    let width = u32::from_be_bytes(picture[16..20].try_into().unwrap());
    let height = u32::from_be_bytes(picture[20..24].try_into().unwrap());
    assert_eq!((width, height), (30, 20));
}

#[test]
fn a_deadline_ends_every_wait_soon_after_it_with_an_error_that_says_so() {
    let Some(binary) = find() else {
        eprintln!("no Chrome: real browser capability unavailable");
        return;
    };
    let mut chrome = Chrome::launch(&binary, &leash()).unwrap();
    chrome
        .navigate("data:text/html,<p>still</p>", PROTOCOL_CPU)
        .unwrap();
    // Without a deadline each of these would wait out the protocol's own allowance: a minute of
    // browser CPU or as many observation turns, at least a minute.
    const NEVER: &str = "new Promise(() => {})";
    let silent = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let held = format!("http://{}/", silent.local_addr().unwrap());
    type Wait = fn(&mut Chrome, &str) -> crate::Result<()>;
    let waits: [(&str, Wait); 3] = [
        ("an evaluation", |chrome, _| chrome.eval(NEVER).map(drop)),
        ("a protocol call", |chrome, _| {
            chrome
                .call(
                    "Runtime.evaluate",
                    json!({"expression": NEVER, "awaitPromise": true}),
                )
                .map(drop)
        }),
        ("a navigation", |chrome, held| {
            chrome.navigate(held, PROTOCOL_CPU)
        }),
    ];
    for (wait, run) in waits {
        let started = Instant::now();
        chrome.set_deadline(Some(started + Duration::from_millis(500)));
        let error = run(&mut chrome, &held).unwrap_err();
        let took = started.elapsed();
        assert!(error.past_deadline(), "{wait}: {error}");
        assert!(
            error.to_string().contains("no reply before the deadline"),
            "{wait}: {error}"
        );
        // A wait is a deadline only when the deadline is the check that ended it (`End::Deadline`),
        // whenever its error is made: had the protocol's own allowance or the backstop ended it,
        // `past_deadline` would be false. How much later than the deadline a loaded machine let it
        // end is no part of the promise (RULES.md `tests/no-test-fails-on-load`).
        assert!(
            took >= Duration::from_millis(500),
            "{wait} ended {took:?} after its start, its deadline 500 ms"
        );
    }
    // Lifted, the waits are the protocol's own again: the page still answers.
    chrome.set_deadline(None);
    chrome
        .navigate("data:text/html,<p>again</p>", PROTOCOL_CPU)
        .unwrap();
    assert_eq!(
        chrome.eval("document.body.textContent").unwrap(),
        json!("again")
    );
    // A failure of Chrome's own is no deadline.
    let thrown = chrome
        .eval("(() => { throw new Error('own') })()")
        .unwrap_err();
    assert!(!thrown.past_deadline(), "{thrown}");
    drop(silent);
}

#[test]
fn a_fresh_context_is_a_page_as_a_launch_leaves_it_and_ends_the_one_before_with_what_it_made() {
    let (mut chrome, peer) = with_peer(true, BACKSTOP_FLOOR, |peer| {
        // Every command but the observer's turns and the page's probes: its session, its method and
        // its parameters.
        let (mut seen, mut made) = (Vec::new(), 0);
        while let Some(command) = peer.next() {
            if marker(&command) || command["params"]["expression"] == "0" {
                peer.reply(&command, json!({"result": {"value": 0}}));
                continue;
            }
            seen.push((
                command["sessionId"].as_str().unwrap_or("").to_string(),
                command["method"].as_str().unwrap_or("").to_string(),
                command["params"].clone(),
            ));
            let result = match command["method"].as_str() {
                Some("Target.createBrowserContext") => {
                    made += 1;
                    json!({"browserContextId": format!("context-{made}")})
                }
                Some("Target.createTarget") => json!({"targetId": format!("target-{made}")}),
                Some("Target.attachToTarget") => json!({"sessionId": format!("session-{made}")}),
                // The page before ends as its context does, told of before the reply.
                Some("Target.closeTarget" | "Target.disposeBrowserContext") => {
                    let before = if made == 1 {
                        "page".to_string()
                    } else {
                        format!("session-{}", made - 1)
                    };
                    peer.send(json!({"method": "Target.detachedFromTarget",
                        "params": {"sessionId": before}}));
                    json!({})
                }
                _ => json!({}),
            };
            peer.reply(&command, result);
        }
        seen
    });
    chrome.follow_frames().unwrap();
    chrome.keep_in_front().unwrap();
    chrome.dismiss_dialogs();
    chrome.enable_in_frames(&["Runtime.enable"]);
    let deadline = Some(Instant::now() + Duration::from_secs(3600));
    chrome.set_deadline(deadline);
    chrome.frame = Some("left".into());
    chrome.fresh_context().unwrap();
    assert_eq!(chrome.session.as_deref(), Some("session-1"));
    assert_eq!(
        chrome.context,
        Some(("context-1".into(), "target-1".into()))
    );
    assert_eq!(chrome.own_pages, ["observer-target", "target-1"]);
    assert!(!chrome.following && !chrome.fronting && !chrome.dismissing);
    assert!(chrome.frame_domains.is_empty() && chrome.frame.is_none());
    assert_eq!(chrome.deadline, deadline);
    // The page before closing is not this page's end, and none of its events is kept.
    assert_eq!(chrome.gone, None);
    assert!(chrome.events().is_empty(), "{:?}", chrome.events());
    chrome.fresh_context().unwrap();
    assert_eq!(chrome.session.as_deref(), Some("session-2"));
    assert_eq!(chrome.own_pages, ["observer-target", "target-2"]);
    assert_eq!(chrome.gone, None);
    drop(chrome);
    let command = |session: &str, method: &str, params: Value| {
        (session.to_string(), method.to_string(), params)
    };
    let seen = peer.join().unwrap();
    let from = seen
        .iter()
        .position(|(_, method, params)| {
            method == "Target.setDiscoverTargets" && params["discover"] == false
        })
        .expect("the browser is asked to tell of no target any more");
    let fresh = |made: usize, end: (String, String, Value)| {
        let session = format!("session-{made}");
        [
            command("", "Target.createBrowserContext", json!({})),
            command(
                "",
                "Target.createTarget",
                json!({"url": "about:blank", "browserContextId": format!("context-{made}")}),
            ),
            command(
                "",
                "Target.attachToTarget",
                json!({"targetId": format!("target-{made}"), "flatten": true}),
            ),
            command(&session, "Page.enable", json!({})),
            command(&session, "Network.enable", json!({})),
            end,
        ]
    };
    let expected: Vec<_> = [command(
        "",
        "Target.setDiscoverTargets",
        json!({"discover": false, "filter": []}),
    )]
    .into_iter()
    // The launch's own page is closed; a context this made is disposed of, its pages with it.
    .chain(fresh(
        1,
        command("", "Target.closeTarget", json!({"targetId": "page-target"})),
    ))
    .chain(fresh(
        2,
        command(
            "",
            "Target.disposeBrowserContext",
            json!({"browserContextId": "context-1"}),
        ),
    ))
    .collect();
    assert_eq!(seen[from..], expected);
}

/// A server of one page, `/`, never kept, which asks for `/kept.js`, kept for an hour by any
/// cache that takes it: its address, and how many times each path was asked for.
fn caching_server() -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    use std::io::{BufRead, Write};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/", listener.local_addr().unwrap());
    let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = asked.clone();
    std::thread::spawn(move || {
        for connection in listener.incoming() {
            let Ok(connection) = connection else {
                return;
            };
            let seen = seen.clone();
            std::thread::spawn(move || {
                let mut reader = std::io::BufReader::new(connection.try_clone().unwrap());
                let mut head = String::new();
                while reader.read_line(&mut head).is_ok_and(|read| read > 2) {}
                let path = head.split(' ').nth(1).unwrap_or("/").to_string();
                let (kind, keep, body) = if path == "/kept.js" {
                    ("text/javascript", "max-age=3600", "window.kept = true;")
                } else {
                    (
                        "text/html",
                        "no-store",
                        "<!doctype html><script src=/kept.js></script>",
                    )
                };
                seen.lock().unwrap().push(path);
                let _ = write!(
                    &connection,
                    "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nCache-Control: {keep}\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            });
        }
    });
    (base, asked)
}

/// What a visit to `base` finds its browser holds of an earlier one (a cookie, a stored item, an
/// IndexedDB database), whether its script came from the cache, and how many times the server
/// has been asked for that script by then; it then leaves all three for the next visit.
fn visit_keeping(
    chrome: &mut Chrome,
    base: &str,
    asked: &std::sync::Mutex<Vec<String>>,
) -> (Value, bool, usize) {
    chrome.open("about:blank").unwrap();
    chrome.open(base).unwrap();
    assert!(
        chrome
            .wait_for("window.kept === true", PROTOCOL_CPU)
            .unwrap(),
        "the page's script never ran"
    );
    let script = format!("{base}kept.js");
    let requests = requests_until(chrome, "the page's script answered", |requests| {
        request(requests, &script).is_some_and(|request| request.finished || request.from_cache)
    });
    let held = chrome
        .eval(
            "(async () => { const held = [document.cookie, localStorage.getItem('seen'), \
             (await indexedDB.databases()).map(database => database.name)]; \
             document.cookie = 'seen=1; max-age=3600'; localStorage.setItem('seen', '1'); \
             await new Promise((resolve, reject) => { const open = indexedDB.open('seen'); \
             open.onsuccess = () => { open.result.close(); resolve(); }; open.onerror = reject; }); \
             return held; })()",
        )
        .unwrap();
    let count = asked
        .lock()
        .unwrap()
        .iter()
        .filter(|path| *path == "/kept.js")
        .count();
    (held, request(&requests, &script).unwrap().from_cache, count)
}

#[test]
fn a_fresh_context_shares_no_cookie_storage_or_cache_entry_and_keeps_its_own_between_visits() {
    let Some(binary) = find() else {
        eprintln!("no Chrome: real browser capability unavailable");
        return;
    };
    let (base, asked) = caching_server();
    let mut chrome = Chrome::launch(&binary, &leash()).unwrap();
    let nothing = json!(["", null, []]);
    let kept = json!(["seen=1", "1", ["seen"]]);
    // The launch's own profile, then a context after it, then a context after a context: each
    // first visit holds nothing and asks the server, each next visit holds its own and asks none.
    let mut visits = Vec::new();
    for context in 0..3 {
        if context > 0 {
            chrome.fresh_context().unwrap();
        }
        visits.push(visit_keeping(&mut chrome, &base, &asked));
        visits.push(visit_keeping(&mut chrome, &base, &asked));
    }
    assert_eq!(
        visits,
        [
            (nothing.clone(), false, 1),
            (kept.clone(), true, 1),
            (nothing.clone(), false, 2),
            (kept.clone(), true, 2),
            (nothing, false, 3),
            (kept, true, 3),
        ]
    );
}

/// The browser's pages but the observer's, each `(target, context)`, and its contexts but the
/// profile's own.
fn pages_and_contexts(chrome: &mut Chrome) -> (Vec<(String, String)>, Vec<String>) {
    let text = |value: &Value| value.as_str().unwrap_or("").to_string();
    let targets = chrome.browser_call("Target.getTargets", json!({})).unwrap();
    let pages = targets["targetInfos"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|target| target["type"] == "page" && target["targetId"] != chrome.own_pages[0])
        .map(|target| (text(&target["targetId"]), text(&target["browserContextId"])))
        .collect();
    let contexts = chrome
        .browser_call("Target.getBrowserContexts", json!({}))
        .unwrap();
    let contexts = contexts["browserContextIds"]
        .as_array()
        .unwrap()
        .iter()
        .map(text)
        .collect();
    (pages, contexts)
}

#[test]
fn a_context_left_for_a_fresh_one_leaves_no_target_and_the_page_runs_on_in_the_new_one() {
    let Some(binary) = find() else {
        eprintln!("no Chrome: real browser capability unavailable");
        return;
    };
    let (base, _asked) = caching_server();
    let mut chrome = Chrome::launch(&binary, &leash()).unwrap();
    chrome.open(&base).unwrap();
    let (launched, none) = pages_and_contexts(&mut chrome);
    assert_eq!((launched.len(), none), (1, Vec::<String>::new()));
    let mut before = launched;
    for _ in 0..2 {
        chrome.fresh_context().unwrap();
        let (pages, contexts) = pages_and_contexts(&mut chrome);
        let (context, target) = chrome.context.clone().unwrap();
        // The one page is the new context's, and neither the page before nor its context is left.
        assert_eq!(pages, [(target, context.clone())]);
        assert_eq!(contexts, [context]);
        assert_ne!(pages, before);
        // The page is one to drive: it opens, answers and waits as a launch's does.
        chrome.open(&base).unwrap();
        assert!(
            chrome
                .wait_for("window.kept === true", PROTOCOL_CPU)
                .unwrap()
        );
        assert_eq!(chrome.eval("1 + 1").unwrap(), json!(2));
        before = pages;
    }
}

#[test]
fn a_target_is_listed_only_by_its_own_id_among_the_answers_targets() {
    let targets = json!({"targetInfos": [{"targetId": "A1", "type": "page"}, {"targetId": "B2"}]});
    assert!(listed(&targets, "A1"));
    assert!(listed(&targets, "B2"));
    assert!(!listed(&targets, "A"), "a part of an id is not it");
    assert!(!listed(&json!({"targetInfos": []}), "A1"));
    assert!(!listed(&json!({}), "A1"), "an answer with no list");
}

#[test]
fn a_page_prefers_no_reduced_motion_until_a_caller_emulates_it() {
    let Some(binary) = find() else {
        eprintln!("no Chrome: real browser capability unavailable");
        return;
    };
    let (base, _asked) = caching_server();
    let mut chrome = Chrome::launch(&binary, &leash()).unwrap();
    chrome.open(&base).unwrap();
    let reduced = "matchMedia('(prefers-reduced-motion: reduce)').matches";
    assert_eq!(chrome.eval(reduced).unwrap(), json!(false));
    chrome
        .call(
            "Emulation.setEmulatedMedia",
            json!({"features": [{"name": "prefers-reduced-motion", "value": "reduce"}]}),
        )
        .unwrap();
    assert_eq!(chrome.eval(reduced).unwrap(), json!(true));
}

#[test]
fn a_condition_that_throws_is_asked_again_until_its_document_was_known_to_have_its_element() {
    let thrown = || json!({"exceptionDetails": {"text": "Uncaught", "exception": {"className": "TypeError"}}});
    // The stand-in page. `window.root` throws twice on a document with no element, the second
    // time as the page goes on, then is met. `window.raced` throws on one that got its element
    // before the driver looked, then on the document the page went to next, which has its own,
    // then is met. `window.broken` throws on a document that has its element throughout.
    let (mut chrome, peer) = with_peer(true, BACKSTOP_FLOOR, move |peer| {
        let mut asked: Vec<String> = Vec::new();
        while let Some(command) = peer.next() {
            if marker(&command) {
                peer.reply(&command, json!({"result": {"value": null}}));
                continue;
            }
            if probe(&command) {
                peer.reply(&command, json!({"result": {"value": 0}}));
                continue;
            }
            let expression = command["params"]["expression"].as_str().unwrap().to_owned();
            asked.push(expression.clone());
            let count = asked.iter().filter(|asked| **asked == expression).count();
            let value = |value: Value| json!({"result": {"value": value}});
            match (expression.as_str(), count) {
                ("window.root" | "window.raced", 1 | 2) | ("window.broken", _) => {
                    peer.reply(&command, thrown());
                }
                ("window.root" | "window.raced", _) => peer.reply(&command, value(json!(true))),
                (ROOTED, 1) => peer.reply(&command, value(Value::Null)),
                (ROOTED, 2) => refuse(peer, &command, "Execution context was destroyed."),
                (ROOTED, 3) => peer.reply(&command, value(json!(1.5))),
                (ROOTED, _) => peer.reply(&command, value(json!(2.5))),
                (other, count) => panic!("evaluation {count} of {other}"),
            }
        }
        asked
    });
    assert!(chrome.wait_for("window.root", PROTOCOL_CPU).unwrap());
    assert!(chrome.wait_for("window.raced", PROTOCOL_CPU).unwrap());
    let broken = chrome
        .wait_for("window.broken", PROTOCOL_CPU)
        .unwrap_err()
        .to_string();
    assert!(broken.starts_with("window.broken threw: "), "{broken}");
    // An evaluation on its own is asked once, and its throw is its answer wherever it is.
    assert!(chrome.eval("window.broken").is_err());
    drop(chrome);
    assert_eq!(
        peer.join().unwrap(),
        [
            "window.root",
            ROOTED,
            "window.root",
            ROOTED,
            "window.root",
            "window.raced",
            ROOTED,
            "window.raced",
            ROOTED,
            "window.raced",
            "window.broken",
            ROOTED,
            "window.broken",
            ROOTED,
            "window.broken"
        ]
    );
}

#[test]
fn a_wait_for_a_closed_page_takes_a_turn_of_the_browsers_between_two_looks() {
    // The stand-in browser lists the page for three looks, then no longer. What it saw: the
    // observer's turns that came before each look.
    let (mut chrome, peer) = with_peer(true, BACKSTOP_FLOOR, |peer| {
        let (mut looks, mut turns) = (Vec::new(), 0u32);
        while let Some(command) = peer.next() {
            if command["method"] == "Target.getTargets" {
                looks.push(std::mem::take(&mut turns));
                let listed = if looks.len() <= 3 {
                    json!([{"targetId": "closing", "type": "page"}])
                } else {
                    json!([])
                };
                peer.reply(&command, json!({"targetInfos": listed}));
            } else if marker(&command) {
                turns += 1;
                peer.reply(&command, json!({"result": {"value": null}}));
            } else if probe(&command) {
                peer.reply(&command, json!({"result": {"value": 0}}));
            } else {
                panic!("unexpected {command}");
            }
        }
        looks
    });
    chrome.wait_closed("closing").unwrap();
    drop(chrome);
    let looks = peer.join().unwrap();
    assert_eq!(looks.len(), 4, "{looks:?}");
    // A look's own call starts one observer turn; the pause between two looks is a turn waited
    // for, with its own.
    assert!(looks[1..].iter().all(|turns| *turns >= 2), "{looks:?}");
}
