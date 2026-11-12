//! The CPU a process takes, the meter of the timings: user and system time, the work it did, which
//! the machine's load does not multiply as it multiplies a wall. [`taken`] reads a live process
//! (the board's server) as it runs, [`Quiet`] bills it what it takes for a piece of work, from
//! quiet before the work to quiet again after it, and [`reaped`] reads what a child took in all
//! once it ends.

use std::cell::OnceCell;
use std::io::Read as _;
use std::process::{ExitStatus, Output};
use std::time::{Duration, Instant};
use symphony_board::sys::Child;

/// How long each look at a process's CPU lasts while [`settle`] waits, nominally, and the most it
/// may take in that long to be quiet, a rate of 20 ms a second: a board's server with no client
/// takes about a millisecond a second.
const QUIET_OVER: Duration = Duration::from_millis(500);
const QUIET: Duration = Duration::from_millis(10);

/// A wait for quiet longer than this is said, with what the process took meanwhile.
const LONG: Duration = Duration::from_secs(2);

/// One look at a process's CPU: how long it lasted, as measured, and what the process took in it.
#[derive(Clone, Copy, Debug)]
struct Window {
    length: Duration,
    took: Duration,
}

impl Window {
    /// Whether the process was quiet in the look, on a meter that counts in `tick`s: it took under
    /// [`QUIET`] a [`QUIET_OVER`], as a rate over the look's measured length, which a sleep that
    /// oversleeps under load stretches past the nominal half second. A meter whose tick is
    /// [`QUIET`] or longer cannot read that rate: Linux's `/proc/PID/stat` counts the clock's
    /// ticks, 10 ms each at 100 Hz, one tick the whole of [`QUIET`], so there a process is quiet
    /// only when its reading did not move in the look, however long it lasted.
    fn quiet(&self, tick: Duration) -> bool {
        if tick >= QUIET {
            return self.took.is_zero();
        }
        self.took.as_nanos() * QUIET_OVER.as_nanos() < QUIET.as_nanos() * self.length.as_nanos()
    }

    /// What the process took in it, in milliseconds a second.
    fn rate(&self) -> f64 {
        self.took.as_secs_f64() * 1000.0 / self.length.as_secs_f64()
    }
}

/// What [`settle`] found: how long it waited, whether the process went quiet in that time, its
/// last look, what the process took over the whole wait, and its reading at the end.
struct Settled {
    waited: Duration,
    quiet: bool,
    last: Window,
    took: Duration,
    reading: Duration,
}

/// Waits until `pid`, a live process, is quiet in one look ([`Window::quiet`]), for at most
/// `most`. Each look starts where the one before it ended.
fn settle(pid: u32, most: Duration) -> Settled {
    let tick = tick();
    let (began, first) = (Instant::now(), taken(pid));
    let (mut at, mut before) = (began, first);
    loop {
        std::thread::sleep(QUIET_OVER);
        let (now, reading) = (Instant::now(), taken(pid));
        let last = Window {
            length: now - at,
            took: reading.saturating_sub(before),
        };
        let (waited, quiet) = (now - began, last.quiet(tick));
        if quiet || waited >= most {
            let took = reading.saturating_sub(first);
            return Settled {
                waited,
                quiet,
                last,
                took,
                reading,
            };
        }
        (at, before) = (now, reading);
    }
}

/// The waits for one process to go quiet, each for at most `most`, and what the process is billed
/// across them ([`Quiet::bill`]). A wait that runs out fails, saying what the process took, and so
/// does every wait after it, at once: a process that never goes quiet costs one wait, not one for
/// each piece of work timed (about 45 in the timings test: 3.6 hours at 300 s each).
pub struct Quiet {
    what: &'static str,
    pid: u32,
    most: Duration,
    /// Why a wait ran out, once one has.
    never: OnceCell<String>,
}

impl Quiet {
    /// The waits for `pid`, named `what` in what they say, each for at most `most`.
    pub fn new(what: &'static str, pid: u32, most: Duration) -> Quiet {
        Quiet {
            what,
            pid,
            most,
            never: OnceCell::new(),
        }
    }

    /// Waits until the process is quiet ([`settle`]): its reading then, or why it never went quiet,
    /// at this wait or an earlier one. A long wait is said.
    fn wait(&self) -> Result<Duration, String> {
        if let Some(never) = self.never.get() {
            return Err(format!(
                "{never} (at an earlier wait; this one did not wait)"
            ));
        }
        let settled = settle(self.pid, self.most);
        if !settled.quiet {
            let quiet = Window {
                length: QUIET_OVER,
                took: QUIET,
            };
            let never = format!(
                "{} never went quiet in {:.1?}: it took {:.3?} of CPU in that time, {:.3?} in its last look of {:.3?}, {:.1} ms a second, where quiet is under {:.1}",
                self.what,
                settled.waited,
                settled.took,
                settled.last.took,
                settled.last.length,
                settled.last.rate(),
                quiet.rate()
            );
            return Err(self.never.get_or_init(|| never).clone());
        }
        if settled.waited > LONG {
            eprintln!(
                "perf: {} went quiet in {:.1?}, taking {:.3?} of CPU meanwhile",
                self.what, settled.waited, settled.took
            );
        }
        Ok(settled.reading)
    }

    /// `work` done, and the CPU the process took for it: from the end of a wait for quiet before
    /// it to the end of one after it, so what the process does for `work` once `work` has its
    /// answer (an event run late, a flush) is billed to `work`, not to what is timed next or to
    /// nothing. The wait after `work` is billed whole: what the process took in its looks before
    /// the last is the work's, and in the last it took under [`QUIET`]'s rate; the wait before
    /// `work` is not billed. `work` is not done once a wait has run out.
    pub fn bill<T>(&self, work: impl FnOnce() -> T) -> Result<(T, Duration), String> {
        let before = self.wait()?;
        let done = work();
        let after = self.wait()?;
        Ok((done, after.saturating_sub(before)))
    }
}

/// Whether a reading of a process's CPU across `what`, work the process does, read it working:
/// a meter that reads none reads another process, and every bound holds at none.
pub fn worked(what: &str, cpu: Duration) -> Result<(), String> {
    if cpu.is_zero() {
        return Err(format!(
            "{what}: the meter read no CPU, so it reads another process"
        ));
    }
    Ok(())
}

/// The user and system CPU `pid`, a live process, has taken so far: its own threads', not its
/// children's.
#[cfg(target_os = "macos")]
pub fn taken(pid: u32) -> Duration {
    let raw = libc::pid_t::try_from(pid).expect("a process id");
    // SAFETY: plain data, for which all zeroes is a value.
    let mut info: libc::rusage_info_v2 = unsafe { std::mem::zeroed() };
    // SAFETY: writes one rusage_info_v2, the size of the flavor asked for, into `info`.
    let read = unsafe { libc::proc_pid_rusage(raw, libc::RUSAGE_INFO_V2, (&raw mut info).cast()) };
    assert_eq!(
        read,
        0,
        "the CPU of process {pid}: {}",
        std::io::Error::last_os_error()
    );
    // The kernel counts it in the machine's ticks (`task_power_info`), not nanoseconds.
    let ticks = u128::from(info.ri_user_time) + u128::from(info.ri_system_time);
    let (numer, denom) = timebase();
    Duration::from_nanos(u64::try_from(ticks * numer / denom).expect("CPU in range"))
}

/// How long one of the ticks [`taken`] counts in lasts: the machine's (a few tens of nanoseconds).
#[cfg(target_os = "macos")]
fn tick() -> Duration {
    let (numer, denom) = timebase();
    Duration::from_nanos(u64::try_from(numer.div_ceil(denom)).expect("a tick in range"))
}

/// The nanoseconds of the machine's ticks, as a fraction.
#[cfg(target_os = "macos")]
#[expect(
    deprecated,
    reason = "libc's mach_timebase_info, which points at the mach2 crate this workspace does not take"
)]
fn timebase() -> (u128, u128) {
    let mut base = libc::mach_timebase_info { numer: 0, denom: 0 };
    // SAFETY: writes the timebase into `base`.
    let read = unsafe { libc::mach_timebase_info(&raw mut base) };
    assert!(
        read == 0 && base.numer != 0 && base.denom != 0,
        "the machine's timebase: {read}"
    );
    (u128::from(base.numer), u128::from(base.denom))
}

/// The user and system CPU `pid`, a live process, has taken so far: its own threads', not its
/// children's, from `/proc/PID/stat` in the clock's ticks.
#[cfg(target_os = "linux")]
pub fn taken(pid: u32) -> Duration {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .unwrap_or_else(|why| panic!("the CPU of process {pid}: {why}"));
    // After the command's closing parenthesis, the state is the first field and utime and stime
    // the twelfth and thirteenth (`proc(5)`'s 14 and 15).
    let after = stat.rsplit_once(')').expect("a stat line").1;
    let fields: Vec<&str> = after.split_whitespace().collect();
    let ticks: u64 = fields[11].parse::<u64>().unwrap() + fields[12].parse::<u64>().unwrap();
    Duration::from_nanos(ticks * 1_000_000_000 / ticks_a_second())
}

/// How long one of the ticks [`taken`] counts in lasts: the clock's, 10 ms at 100 Hz.
#[cfg(target_os = "linux")]
fn tick() -> Duration {
    Duration::from_nanos(1_000_000_000 / ticks_a_second())
}

/// The clock's ticks a second, which `/proc/PID/stat` counts in.
#[cfg(target_os = "linux")]
fn ticks_a_second() -> u64 {
    // SAFETY: sysconf reads a configuration value.
    let per_second = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    u64::try_from(per_second).expect("clock ticks a second")
}

/// `child`, started with its outputs piped, read to their end and reaped: what it said, how it
/// ended, and the user and system CPU it took with every child it reaped, as its reap reports it.
pub fn reaped(mut child: Child) -> (Output, Duration) {
    drop(child.stdin.take());
    let stderr = child.stderr.take().map(|mut stderr| {
        std::thread::spawn(move || {
            let mut said = Vec::new();
            stderr.read_to_end(&mut said).map(|_| said)
        })
    });
    let mut stdout = Vec::new();
    if let Some(mut out) = child.stdout.take() {
        out.read_to_end(&mut stdout).expect("the child's stdout");
    }
    let stderr = match stderr {
        Some(reader) => reader.join().unwrap().expect("the child's stderr"),
        None => Vec::new(),
    };
    let pid = libc::pid_t::try_from(child.id()).expect("a process id");
    loop {
        let mut status = 0;
        // SAFETY: plain data, for which all zeroes is a value.
        let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
        // SAFETY: reaps this process's child `pid`, writing its status and its usage. `Child`
        // neither reaps nor signals it once dropped.
        let waited = unsafe { libc::wait4(pid, &raw mut status, 0, &raw mut usage) };
        if waited == pid {
            use std::os::unix::process::ExitStatusExt as _;
            let output = Output {
                status: ExitStatus::from_raw(status),
                stdout,
                stderr,
            };
            return (output, time(usage.ru_utime) + time(usage.ru_stime));
        }
        let error = std::io::Error::last_os_error();
        assert_eq!(
            error.kind(),
            std::io::ErrorKind::Interrupted,
            "the reap of {pid}: {error}"
        );
    }
}

/// A `timeval` as a duration.
fn time(value: libc::timeval) -> Duration {
    let seconds = u64::try_from(value.tv_sec).expect("a time since none");
    let micros = u64::try_from(value.tv_usec).expect("a time since none");
    Duration::from_secs(seconds) + Duration::from_micros(micros)
}

#[cfg(test)]
mod meter {
    use super::*;
    use std::cell::Cell;
    use std::io::{BufRead as _, BufReader, Write as _};
    use symphony_board::sys::{self, Command};

    /// `sh -c script`, its input and output piped.
    fn sh(script: &str) -> Child {
        let mut command = Command::new("sh");
        command.args(["-c", script]);
        sys::spawn_piped(&mut command).expect("sh starts")
    }

    /// A shell that steps a hundred thousand times, as `sh` counts, which takes it tens of
    /// milliseconds of CPU or more whatever the load.
    const STEPS: &str = "i=0; while [ $i -lt 100000 ]; do i=$((i+1)); done";

    /// A child's CPU read while it lives, once it has done its work and waits on its input, is what
    /// its reap says it took in all, in the same unit; the wait between counts for nothing; and the
    /// work counts whatever the load, as a count of steps, not a time, fixes it.
    #[test]
    fn a_childs_cpu_read_while_it_waits_is_what_its_reap_says_and_its_wait_is_none() {
        let mut child = sh(&format!("{STEPS}; echo worked; read _"));
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        assert_eq!(line, "worked\n");
        let worked = taken(child.id());
        std::thread::sleep(Duration::from_millis(300));
        let waited = taken(child.id());
        child.stdin.take().unwrap().write_all(b"\n").unwrap();
        child.stdout = Some(stdout.into_inner());
        let (output, all) = reaped(child);
        assert!(output.status.success(), "{output:?}");
        eprintln!("cpu: worked {worked:?}, after its wait {waited:?}, reaped {all:?}");
        assert!(
            worked >= Duration::from_millis(20),
            "a hundred thousand steps of sh took {worked:?}"
        );
        assert!(
            waited - worked < Duration::from_millis(20),
            "its wait took {:?}",
            waited - worked
        );
        assert!(
            waited <= all && all - waited < Duration::from_millis(50),
            "read while it waited {waited:?}, reaped {all:?}"
        );
    }

    /// A child waiting on its input settles before the wait runs out; one at work, which load
    /// slows but does not stop, does not settle, and the wait ends when it runs out.
    #[test]
    fn a_waiting_child_settles_and_a_working_one_waits_until_the_wait_runs_out() {
        let mut waiting = sh("read _");
        let mut working = sh("while :; do :; done");
        let (most, short) = (Duration::from_secs(10), Duration::from_secs(2));
        let quiet = settle(waiting.id(), most);
        let busy = settle(working.id(), short);
        working.kill().unwrap();
        working.wait().unwrap();
        waiting.stdin.take().unwrap().write_all(b"\n").unwrap();
        assert!(reaped(waiting).0.status.success());
        eprintln!(
            "cpu: the waiting child settled in {:?}, the working one waited {:?}, taking {:?}",
            quiet.waited, busy.waited, busy.took
        );
        assert!(
            quiet.quiet && quiet.waited < most,
            "the waiting child: quiet {} after {:?}",
            quiet.quiet,
            quiet.waited
        );
        assert!(
            !busy.quiet && busy.waited >= short,
            "the working child: quiet {} after {:?}",
            busy.quiet,
            busy.waited
        );
        assert!(
            busy.took >= busy.last.took && busy.last.took >= QUIET,
            "the working child took {:?}, {:?} in its last look",
            busy.took,
            busy.last.took
        );
    }

    fn window(took_ms: u64, length_ms: u64) -> Window {
        Window {
            length: Duration::from_millis(length_ms),
            took: Duration::from_millis(took_ms),
        }
    }

    /// On a meter finer than [`QUIET`], a look is quiet under 20 ms a second of its measured
    /// length: a look an oversleep stretched to a second is quiet at 15 ms, which the nominal half
    /// second would call busy, and busy at 25 ms; a short one is held to its own length too.
    #[test]
    fn a_look_is_quiet_under_the_rate_over_its_measured_length() {
        let fine = Duration::from_nanos(42);
        assert!(window(0, 500).quiet(fine));
        assert!(window(9, 500).quiet(fine));
        assert!(!window(10, 500).quiet(fine));
        assert!(window(15, 1000).quiet(fine), "an oversleep's look");
        assert!(window(19, 1000).quiet(fine));
        assert!(!window(20, 1000).quiet(fine));
        assert!(!window(25, 1000).quiet(fine));
        assert!(window(4, 250).quiet(fine));
        assert!(!window(6, 250).quiet(fine), "a short look's rate");
        for (look, rate) in [(window(15, 1000), 15.0), (window(10, 500), 20.0)] {
            assert!(
                (look.rate() - rate).abs() < 1e-9,
                "{look:?}: {}",
                look.rate()
            );
        }
    }

    /// On a meter of ticks as long as [`QUIET`] (Linux's at 100 Hz), a look is quiet only when
    /// the reading did not move, however long the look: one tick in two seconds is busy.
    #[test]
    fn on_a_meter_of_ten_millisecond_ticks_a_look_is_quiet_only_when_the_reading_did_not_move() {
        for tick in [QUIET, Duration::from_millis(20)] {
            assert!(window(0, 500).quiet(tick));
            assert!(window(0, 5000).quiet(tick));
            assert!(!window(10, 500).quiet(tick));
            assert!(!window(10, 2000).quiet(tick), "one tick in two seconds");
        }
    }

    /// This machine's meter counts in ticks [`Window::quiet`] reads a rate in: under a microsecond
    /// on macOS; on Linux the clock's, which `/proc/PID/stat` counts.
    #[test]
    fn the_meters_tick_is_this_machines() {
        let tick = tick();
        assert!(!tick.is_zero());
        #[cfg(target_os = "macos")]
        assert!(tick < Duration::from_micros(1), "{tick:?}");
        #[cfg(target_os = "linux")]
        assert_eq!(tick, Duration::from_nanos(1_000_000_000 / ticks_a_second()));
    }

    /// A process at work that never goes quiet fails the wait once it runs out, saying what it took
    /// and at what rate, and every wait after it fails at once, the same, its work not done.
    #[test]
    fn a_wait_that_runs_out_fails_and_every_wait_after_it_fails_at_once() {
        let mut working = sh("while :; do :; done");
        let short = Duration::from_secs(2);
        let waits = Quiet::new("the working child", working.id(), short);
        let began = Instant::now();
        let first = waits.wait();
        let waited = began.elapsed();
        let began = Instant::now();
        let again = waits.wait();
        let done = Cell::new(false);
        let billed = waits.bill(|| done.set(true));
        let after = began.elapsed();
        working.kill().unwrap();
        working.wait().unwrap();
        eprintln!("cpu: the first wait, after {waited:?}: {first:?}");
        let why = first.expect_err("a working child never goes quiet");
        assert!(waited >= short, "the first wait waited {waited:?}");
        assert!(
            why.starts_with("the working child never went quiet in ")
                && why.contains("of CPU in that time")
                && why.contains("ms a second, where quiet is under 20.0"),
            "{why}"
        );
        let earlier = format!("{why} (at an earlier wait; this one did not wait)");
        assert_eq!(again.expect_err("a wait after it"), earlier);
        assert_eq!(billed.expect_err("work billed after it"), earlier);
        assert!(!done.get(), "work billed after a wait ran out was done");
        assert!(after < QUIET_OVER, "the waits after it took {after:?}");
    }

    /// Work billed from quiet to quiet again bears what the process did for it after it answered:
    /// a child that answers and then steps a hundred thousand times bills those steps to the work
    /// that asked, and the next work, which it answers and does nothing more for, bears none.
    #[test]
    fn what_a_process_does_after_it_answered_is_billed_to_the_work_that_asked_it() {
        let mut child = sh(&format!(
            "read _; echo answered; {STEPS}; read _; echo answered; read _"
        ));
        let mut input = child.stdin.take().unwrap();
        let mut output = BufReader::new(child.stdout.take().unwrap());
        let waits = Quiet::new("the child", child.id(), Duration::from_secs(120));
        let ((first, after_first), (second, after_second)) = {
            let mut ask = || {
                input.write_all(b"\n").unwrap();
                let mut line = String::new();
                output.read_line(&mut line).unwrap();
                line
            };
            (waits.bill(&mut ask).unwrap(), waits.bill(&mut ask).unwrap())
        };
        // The line its last read takes, so it ends well.
        input.write_all(b"\n").unwrap();
        child.stdin = Some(input);
        child.stdout = Some(output.into_inner());
        let (ended, all) = reaped(child);
        assert!(ended.status.success(), "{ended:?}");
        eprintln!(
            "cpu: billed {after_first:?} for the first, {after_second:?} for the second, of {all:?} in all"
        );
        assert_eq!([first.as_str(), second.as_str()], ["answered\n"; 2]);
        assert!(
            after_first >= Duration::from_millis(20),
            "the steps after the first answer were billed to it: {after_first:?}"
        );
        assert!(
            after_second < Duration::from_millis(20),
            "the second bore none of them: {after_second:?}"
        );
        assert!(
            after_first + after_second <= all,
            "billed {after_first:?} and {after_second:?} of {all:?}"
        );
    }

    /// A reading of work that read no CPU fails, as a meter reading another process would; any
    /// reading above none passes.
    #[test]
    fn a_reading_of_work_at_no_cpu_fails() {
        assert_eq!(
            worked("say", Duration::ZERO),
            Err("say: the meter read no CPU, so it reads another process".into())
        );
        assert_eq!(worked("say", Duration::from_nanos(1)), Ok(()));
        assert_eq!(worked("say", Duration::from_secs(2)), Ok(()));
    }
}
