//! The CPU a process takes, the meter of the timings: user and system time, the work it did, which
//! the machine's load does not multiply as it multiplies a wall. [`taken`] reads a live process as
//! it runs, [`tree_taken`] a server that answers from processes of its own (the board's
//! PostgreSQL: its postmaster, the backend of each connection and its own workers), [`Quiet`]
//! bills either what it takes for a piece of work, from quiet before the work to quiet again after
//! it, and [`reaped`] reads what a child took in all once it ends. The readings are the shared
//! test helper's (`crates/test-time`, RULES.md `tests/no-test-fails-on-load`).

use crate::children::Child as _;
use std::cell::OnceCell;
use std::io::Read as _;
use std::process::Output;
use std::time::{Duration, Instant};
use symphony_board::sys::Child;
use test_time::cpu::tick;

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

/// A reading of what a process has taken so far, as [`taken`] reads a live one.
type Meter = Box<dyn Fn() -> Duration + Send>;

/// Waits until the process `meter` reads, on a meter of `tick`s, is quiet in one look
/// ([`Window::quiet`]), for at most `most`. Each look starts where the one before it ended. A
/// process the scheduler leaves without the CPU for a whole look, as a loaded machine can, reads
/// quiet in it: it took nothing there.
fn settle(meter: &dyn Fn() -> Duration, tick: Duration, most: Duration) -> Settled {
    let (began, first) = (Instant::now(), meter());
    let (mut at, mut before) = (began, first);
    loop {
        #[expect(
            clippy::disallowed_methods,
            reason = "no-timed-waits baseline: a meter reads the CPU a process took over a window of this length, where the process's own mark that it is idle is the event"
        )]
        std::thread::sleep(QUIET_OVER);
        let (now, reading) = (Instant::now(), meter());
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
    meter: Meter,
    tick: Duration,
    most: Duration,
    /// Why a wait ran out, once one has.
    never: OnceCell<String>,
}

impl Quiet {
    /// The waits for `pid`, a live process named `what` in what they say, each for at most `most`.
    #[allow(dead_code, reason = "a meter of one process, beside the server's tree")]
    pub fn new(what: &'static str, pid: u32, most: Duration) -> Quiet {
        Quiet::reading(what, Box::new(move || taken(pid)), tick(), most)
    }

    /// The waits for the server whose first process is `root`, a live one named `what` in what
    /// they say, each for at most `most`: its CPU and that of every process under it
    /// ([`tree_taken`]).
    pub fn tree(what: &'static str, root: u32, most: Duration) -> Quiet {
        Quiet::reading(what, Box::new(move || tree_taken(root)), tick(), most)
    }

    /// The waits for the process `meter` reads, on a meter of `tick`s.
    fn reading(what: &'static str, meter: Meter, tick: Duration, most: Duration) -> Quiet {
        Quiet {
            what,
            meter,
            tick,
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
        let settled = settle(&self.meter, self.tick, self.most);
        if !settled.quiet {
            let quiet = Window {
                length: QUIET_OVER,
                took: QUIET,
            };
            let never = format!(
                "{} never went quiet in {:.1?}: it took {:.3?} of CPU in that time, {:.3?} in its last look of {:.3?}, {:.1} ms a second, where quiet is under {:.1}; {}",
                self.what,
                settled.waited,
                settled.took,
                settled.last.took,
                settled.last.length,
                settled.last.rate(),
                quiet.rate(),
                test_time::Load::now()
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
/// children's, in the machine's ticks ([`tick`]).
pub fn taken(pid: u32) -> Duration {
    test_time::cpu::of(pid).unwrap_or_else(|why| panic!("the CPU of process {pid}: {why}"))
}

/// How many times [`tree_taken`] reads a server's tree before it takes a reading during which a
/// process under it ended: a server that ends processes faster than that never goes quiet anyway.
const TREE_READINGS: usize = 5;

/// What the server whose first process is `root` has taken so far: `root`'s own CPU and what the
/// children it reaped took, and each live process under it, its own and what it reaped
/// ([`tree_cpu`]). PostgreSQL answers each connection from a backend of its own, which its
/// postmaster reaps once the connection ends, so a backend's work is in the reading while it lives
/// and after. A process that ends while the tree is read moves from the live ones to its parent's
/// reaped: the reading is taken again until `root`'s reaped did not move across it, so none is
/// missed or counted twice ([`TREE_READINGS`] at most). `root` must be live: its own reading is
/// needed.
pub fn tree_taken(root: u32) -> Duration {
    let read = |pid: u32| {
        Some((
            test_time::cpu::of(pid).ok()?,
            test_time::cpu::reaped(pid).ok()?,
        ))
    };
    let mut reading = Duration::ZERO;
    for _ in 0..TREE_READINGS {
        let before = (test_time::cpu::reaped(root))
            .unwrap_or_else(|why| panic!("the CPU of process {root}: {why}"));
        let under = descendants(&table(), root);
        reading = (tree_cpu(root, &under, read))
            .unwrap_or_else(|| panic!("the CPU of process {root}: it is gone"));
        if test_time::cpu::reaped(root).ok() == Some(before) {
            break;
        }
    }
    reading
}

/// The CPU of `root` and the processes `under` it, as `read` gives each one's own and what the
/// children it reaped took: one that `read` no longer finds ended after the table was read, and
/// is passed over, as its parent counts it once it reaps it. The root's is needed.
fn tree_cpu(
    root: u32,
    under: &[u32],
    read: impl Fn(u32) -> Option<(Duration, Duration)>,
) -> Option<Duration> {
    let (own, reaped) = read(root)?;
    let rest = under.iter().filter_map(|pid| read(*pid));
    Some(own + reaped + rest.map(|(own, reaped)| own + reaped).sum::<Duration>())
}

/// The machine's processes now, each its id and its parent's, as `ps` lists them.
fn table() -> Vec<(u32, u32)> {
    let mut command = symphony_board::sys::command("ps");
    command.args(["-axo", "pid=,ppid="]);
    let said = symphony_board::sys::output(&mut command).expect("ps lists the processes");
    rows(&String::from_utf8_lossy(&said.stdout))
}

/// `ps`'s lines of a process id and its parent's, each as a pair: a line of any other shape is
/// passed over.
fn rows(ps: &str) -> Vec<(u32, u32)> {
    (ps.lines())
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let row = (fields.next()?.parse().ok()?, fields.next()?.parse().ok()?);
            fields.next().is_none().then_some(row)
        })
        .collect()
}

/// Every process under `root` in `rows`, its children first, then theirs.
fn descendants(rows: &[(u32, u32)], root: u32) -> Vec<u32> {
    let mut found = vec![root];
    let mut at = 0;
    while let Some(&parent) = found.get(at) {
        found.extend(
            (rows.iter())
                .filter(|row| row.1 == parent && row.0 != root && !found.contains(&row.0))
                .map(|row| row.0)
                .collect::<Vec<_>>(),
        );
        at += 1;
    }
    found.remove(0);
    found
}

/// `child`, started with its outputs piped, read to their end and reaped: what it said, how it
/// ended, and the user and system CPU it took with every child it reaped, as its reap reports it.
/// `Child` neither reaps nor signals it once dropped.
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
    let pid = child.id();
    let (status, cpu) =
        test_time::cpu::reap(pid).unwrap_or_else(|why| panic!("the reap of {pid}: {why}"));
    let output = Output {
        status,
        stdout,
        stderr,
    };
    (output, cpu)
}

#[cfg(test)]
mod meter {
    use super::*;
    use std::cell::Cell;
    use std::io::Write as _;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};
    use symphony_board::sys::{self, Command};

    /// `sh -c script`, its input and output piped.
    fn sh(script: &str) -> Child {
        let mut command = Command::of("sh");
        command.args(["-c", script]);
        sys::spawn_piped(&mut command).expect("sh starts")
    }

    /// A shell that steps a hundred thousand times, as `sh` counts, which takes it tens of
    /// milliseconds of CPU or more whatever the load.
    const STEPS: &str = "i=0; while [ $i -lt 100000 ]; do i=$((i+1)); done";

    /// A child reaped says what it wrote on each output, how it ended and the CPU it took, which
    /// counts its work whatever the load, as a count of steps, not a time, fixes it. What the meter
    /// reads of a child while it lives and at its reap is the helper's to test
    /// (`crates/test-time`).
    #[test]
    fn a_reaped_child_says_what_it_wrote_how_it_ended_and_what_it_took() {
        let child = sh(&format!("{STEPS}; echo worked; echo said >&2; exit 3"));
        let (output, took) = reaped(child);
        eprintln!("cpu: reaped {took:?}: {output:?}");
        assert_eq!(output.status.code(), Some(3), "{output:?}");
        assert_eq!(
            (output.stdout.as_slice(), output.stderr.as_slice()),
            (&b"worked\n"[..], &b"said\n"[..])
        );
        assert!(
            took >= Duration::from_millis(20),
            "a hundred thousand steps of sh took {took:?}"
        );
    }

    /// A meter's tick finer than [`QUIET`], as macOS's is.
    const FINE: Duration = Duration::from_nanos(42);
    const SECOND: Duration = Duration::from_secs(1);
    const HOUR: Duration = Duration::from_secs(3600);

    /// A process the tests run by hand, behind a [`Meter`]: what it has taken so far, and what it
    /// owes, work it was given to do later, of which it does a `step` at each reading, as a process
    /// at work goes on between looks. A real child at work cannot stand in: at load 140 to 170,
    /// beside the board's copy, the scheduler left a `while :; do :; done` without the CPU for a
    /// whole look, which then read quiet, rightly. What the meter reads of a real child is the
    /// first test's.
    #[derive(Clone)]
    struct Fake {
        state: Arc<Mutex<(Duration, Duration)>>,
        step: Duration,
        /// How many times the meter was read.
        reads: Arc<AtomicU64>,
    }

    impl Fake {
        fn new(step: Duration) -> Fake {
            Fake {
                state: Arc::default(),
                step,
                reads: Arc::default(),
            }
        }

        /// How many times the meter was read so far: one at the start of a wait and one at the end
        /// of each of its looks.
        fn reads(&self) -> u64 {
            self.reads.load(Ordering::Relaxed)
        }

        fn meter(&self) -> Meter {
            let fake = self.clone();
            Box::new(move || {
                fake.reads.fetch_add(1, Ordering::Relaxed);
                let mut state = fake.state.lock().unwrap();
                let (took, owed) = &mut *state;
                let done = (*owed).min(fake.step);
                *owed -= done;
                *took += done;
                *took
            })
        }

        /// Work done `now`, and work owed for `later`.
        fn works(&self, now: Duration, later: Duration) {
            let mut state = self.state.lock().unwrap();
            state.0 += now;
            state.1 += later;
        }
    }

    /// A child waiting on its input settles; a process at work, a second of CPU at each look, does
    /// not, and the wait ends when it runs out, having read what it took. The child's wait is held
    /// to the shared backstop only, as a loaded machine can stretch each look's sleep.
    #[test]
    fn a_waiting_child_settles_and_a_working_process_waits_until_the_wait_runs_out() {
        let mut waiting = sh("read _");
        let working = Fake::new(SECOND);
        working.works(Duration::ZERO, HOUR);
        let (most, short) = (
            test_time::backstop(Duration::from_secs(10)),
            Duration::from_secs(2),
        );
        let quiet = settle(&|| taken(waiting.id()), tick(), most);
        let busy = settle(&working.meter(), FINE, short);
        waiting.stdin.take().unwrap().write_all(b"\n").unwrap();
        assert!(reaped(waiting).0.status.success());
        eprintln!(
            "cpu: the waiting child settled in {:?}, the working process waited {:?}, taking {:?}",
            quiet.waited, busy.waited, busy.took
        );
        assert!(
            quiet.quiet,
            "the waiting child: not quiet after {:?}, taking {:?}; {}",
            quiet.waited,
            quiet.took,
            test_time::Load::now()
        );
        assert!(
            !busy.quiet && busy.waited >= short,
            "the working process: quiet {} after {:?}",
            busy.quiet,
            busy.waited
        );
        assert_eq!(busy.last.took, SECOND, "its last look");
        let looks = u32::try_from(busy.took.as_secs()).unwrap();
        assert!(
            (1..=4).contains(&looks) && busy.took == SECOND * looks,
            "a second a look: {:?}",
            busy.took
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

    /// A process at work that never goes quiet fails the wait once it runs out, saying what it took,
    /// at what rate and the machine's load, and every wait after it fails at once, the same, its
    /// work not done: they read the meter not once more.
    #[test]
    fn a_wait_that_runs_out_fails_and_every_wait_after_it_fails_at_once() {
        let working = Fake::new(SECOND);
        working.works(Duration::ZERO, HOUR);
        let short = Duration::from_secs(2);
        let waits = Quiet::reading("the working process", working.meter(), FINE, short);
        let began = Instant::now();
        let first = waits.wait();
        let waited = began.elapsed();
        let looked = working.reads();
        let again = waits.wait();
        let done = Cell::new(false);
        let billed = waits.bill(|| done.set(true));
        eprintln!("cpu: the first wait, after {waited:?} and {looked} reads: {first:?}");
        let why = first.expect_err("a working process never goes quiet");
        assert!(waited >= short, "the first wait waited {waited:?}");
        assert!(
            why.starts_with("the working process never went quiet in ")
                && why.contains("s of CPU in that time, 1.000s in its last look of ")
                && why.contains("ms a second, where quiet is under 20.0; load average "),
            "{why}"
        );
        let earlier = format!("{why} (at an earlier wait; this one did not wait)");
        assert_eq!(again.expect_err("a wait after it"), earlier);
        assert_eq!(billed.expect_err("work billed after it"), earlier);
        assert!(!done.get(), "work billed after a wait ran out was done");
        assert!(
            (2..=5).contains(&looked),
            "the first wait read the meter at its start and at each of its looks: {looked}"
        );
        assert_eq!(working.reads(), looked, "the waits after it read the meter");
    }

    /// Work billed from quiet to quiet again bears what the process did for it after it answered:
    /// work that took 50 ms and left 3 s for later, which the process does a second at each look,
    /// is billed 3.05 s, all of it, and the next, which took 5 ms and left nothing, is billed 5 ms,
    /// none of the first's. Billed up to its answer, the first would bear 50 ms, and the 3 s
    /// nothing: the wait before the next would wait them out.
    #[test]
    fn what_a_process_does_after_it_answered_is_billed_to_the_work_that_asked_it() {
        let ms = Duration::from_millis;
        let process = Fake::new(SECOND);
        let waits = Quiet::reading(
            "the process",
            process.meter(),
            FINE,
            Duration::from_secs(60),
        );
        let ((), first) = waits.bill(|| process.works(ms(50), 3 * SECOND)).unwrap();
        let ((), second) = waits.bill(|| process.works(ms(5), Duration::ZERO)).unwrap();
        assert_eq!((first, second), (ms(3050), ms(5)));
    }

    /// A server's tree is its first process and every process under it, its children's children
    /// too, and none beside it; `ps`'s header or a line of another shape is passed over.
    #[test]
    fn a_servers_tree_is_every_process_under_its_first() {
        let ps = "  PID  PPID\n    1     0\n  100     1\n  101   100\n  102   100\n  103   101\n  200     1\n  oops\n";
        let rows = rows(ps);
        assert_eq!(rows.len(), 6, "{rows:?}");
        assert_eq!(descendants(&rows, 100), [101, 102, 103]);
        assert_eq!(descendants(&rows, 103), Vec::<u32>::new());
        assert_eq!(descendants(&rows, 999), Vec::<u32>::new());
    }

    /// A tree's CPU is its first process's own and reaped, and each live process's under it; one
    /// ended since the table was read is passed over, its parent counting it once it reaps it; a
    /// first process gone is no reading.
    #[test]
    fn a_trees_cpu_is_each_live_processs_own_and_reaped() {
        let ms = Duration::from_millis;
        let read = |pid: u32| match pid {
            100 => Some((ms(10), ms(300))),
            101 => Some((ms(5), ms(0))),
            102 => None,
            103 => Some((ms(1), ms(2))),
            _ => None,
        };
        assert_eq!(tree_cpu(100, &[101, 102, 103], read), Some(ms(318)));
        assert_eq!(tree_cpu(100, &[], read), Some(ms(310)));
        assert_eq!(tree_cpu(102, &[101], read), None);
    }

    /// The tree of a live shell that started a child reads its CPU, the child's within it.
    #[test]
    fn the_tree_of_a_live_process_reads_its_childrens_cpu() {
        let mut parent = sh(&format!("({STEPS}) & read _; wait"));
        let root = parent.id();
        let worked = test_time::until(
            "the child's steps in its parent's tree",
            test_time::backstop(Duration::from_secs(10)),
            || {
                let took = tree_taken(root);
                (took >= Duration::from_millis(20)).then_some(took)
            },
        );
        parent.stdin.take().unwrap().write_all(b"\n").unwrap();
        assert!(reaped(parent).0.status.success());
        let took = worked.unwrap_or_else(|stall| panic!("{stall}"));
        eprintln!("cpu: the shell's tree took {took:?}");
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
