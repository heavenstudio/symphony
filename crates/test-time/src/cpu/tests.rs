use super::*;
use std::hint::black_box;
use std::io::{BufRead as _, BufReader, Write as _};
use std::process::{Command, Stdio};

/// Steps of arithmetic the optimiser cannot drop: milliseconds of CPU or more in any build, and
/// the same work whatever the load, as a count fixes it.
const STEPS: u64 = 20_000_000;

fn spin(steps: u64) -> u64 {
    let mut sum = 0u64;
    for step in 0..steps {
        sum = black_box(sum.wrapping_add(black_box(step)));
    }
    sum
}

/// What a meter gained from `before` to `after`: a CPU meter never runs backwards.
fn since(before: Duration, after: Duration) -> Duration {
    after
        .checked_sub(before)
        .unwrap_or_else(|| panic!("a CPU meter ran backwards, from {before:?} to {after:?}"))
}

/// A busy loop is charged to the thread that ran it, a sleep three hundred milliseconds long
/// next to nothing, as the thread took no CPU while it slept, however long the wall.
#[test]
fn a_busy_loop_is_charged_to_its_thread_and_a_sleep_is_not() {
    let (_, busy) = spent(|| spin(STEPS));
    let ((), slept) = spent(|| std::thread::sleep(Duration::from_millis(300)));
    eprintln!("cpu: {STEPS} steps took {busy:?}, a 300 ms sleep {slept:?}");
    assert!(busy >= Duration::from_millis(1), "{STEPS} steps: {busy:?}");
    assert!(slept < Duration::from_millis(10), "a sleep: {slept:?}");
    assert!(slept < busy, "a sleep {slept:?}, the loop {busy:?}");
}

/// Another thread's work is the process's and not the calling thread's.
#[test]
fn another_threads_work_is_its_processs_and_not_the_calling_threads() {
    let (mine, all) = (thread(), process());
    let theirs = std::thread::spawn(|| spent(|| spin(STEPS)).1)
        .join()
        .unwrap();
    let (mine, all) = (since(mine, thread()), since(all, process()));
    eprintln!("cpu: the other thread took {theirs:?}, this one {mine:?}, the process {all:?}");
    assert!(theirs >= Duration::from_millis(1), "{theirs:?}");
    assert!(mine < theirs, "this thread {mine:?}, the other {theirs:?}");
    assert!(
        all >= theirs,
        "the process {all:?}, the other thread {theirs:?}"
    );
}

/// A shell that steps a hundred thousand times, as `sh` counts, which takes it tens of
/// milliseconds of CPU or more whatever the load, then says so and waits on its input.
const SHELL: &str = "i=0; while [ $i -lt 100000 ]; do i=$((i+1)); done; echo worked; read _";

/// A child's CPU read while it lives, once it has done its work and waits on its input, is what
/// its reap says it took, near enough; its wait between is charged nothing; and the reap adds it to
/// what this process's reaped children took.
#[test]
#[expect(
    clippy::zombie_processes,
    reason = "`reap` waits on the child by its pid, to read the CPU its reap reports"
)]
fn a_childs_work_is_read_while_it_waits_and_at_its_reap_and_its_wait_is_not() {
    let children_before = children();
    let mut child = Command::new("sh")
        .args(["-c", SHELL])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    stdout.read_line(&mut line).unwrap();
    assert_eq!(line, "worked\n");
    let worked = of(child.id()).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let waited = of(child.id()).unwrap();
    child.stdin.take().unwrap().write_all(b"\n").unwrap();
    let (status, reaped) = reap(child.id()).unwrap();
    let grew = since(children_before, children());
    eprintln!("cpu: worked {worked:?}, after its wait {waited:?}, reaped {reaped:?}");
    assert!(status.success(), "{status:?}");
    assert!(worked >= Duration::from_millis(20), "{worked:?}");
    let idle = since(worked, waited);
    assert!(idle < Duration::from_millis(20), "its wait took {idle:?}");
    let last = since(waited, reaped);
    assert!(
        last < Duration::from_millis(50),
        "read while it waited {waited:?}, reaped {reaped:?}"
    );
    assert!(
        grew >= reaped,
        "the reaped children took {grew:?}, this one {reaped:?}"
    );
}

/// A process that is gone has no CPU to read, and a process that is not this one's child cannot
/// be reaped: each an error, not a reading of none.
#[test]
#[expect(
    clippy::zombie_processes,
    reason = "`reap` waits on the child by its pid; the test is that a second reap is refused"
)]
fn a_gone_process_and_one_not_a_child_are_errors() {
    let child = Command::new("true").spawn().unwrap();
    let pid = child.id();
    assert!(reap(pid).unwrap().0.success());
    assert!(of(pid).is_err(), "a reaped child's CPU");
    assert!(reap(pid).is_err(), "a child reaped twice");
    assert!(reap(1).is_err(), "launchd or init is no child of a test");
}

/// The meter of a live process counts in this machine's ticks: under a microsecond on macOS; on
/// Linux the clock's, which `/proc/PID/stat` counts.
#[test]
fn the_meters_tick_is_this_machines() {
    let tick = tick();
    assert!(!tick.is_zero());
    #[cfg(target_os = "macos")]
    assert!(tick < Duration::from_micros(1), "{tick:?}");
    #[cfg(target_os = "linux")]
    assert_eq!(tick, Duration::from_nanos(1_000_000_000 / ticks_a_second()));
}

/// A stat line's user and system ticks are read after the command's last parenthesis, whatever
/// the command's name holds; a line cut short reads none.
#[test]
fn a_stat_line_is_read_after_its_commands_last_parenthesis() {
    let stat = "10 (chrome (helper) 2) S 1 10 10 0 0 0 0 0 0 0 25 5 900 800";
    assert_eq!(stat_ticks(stat), Some(30));
    assert_eq!(stat_ticks("10 (sh) S 1 10"), None);
    assert_eq!(stat_ticks("malformed"), None);
}
