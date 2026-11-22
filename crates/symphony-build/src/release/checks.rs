//! A check of the release: one command run in the candidate's checkout, its output in a log of its
//! own, recorded through the candidate's `tools/record.sh` as one test operation of the board's
//! recorder (`tasks run`, docs/workflow/MACHINE_WORK.md), labelled `release NAME`. The steps inside it are
//! not recorded again. A candidate without the script runs it as it is.
//!
//! A check runs in a process group of its own and inherits no descriptor of the run's: not its lock,
//! so nothing a check leaves running, in its group or out of it, holds the run's lock. It starts
//! with every signal at its default and none blocked, whatever the run was started ignoring. While it
//! runs, the run's `running.json` names its group and its leader's start ([`Checking`]), so a run
//! killed while its check runs leaves that said, and a later run waits while that group lives
//! ([`group::lives`]). It runs until its leader ends, the run's time is up or the run is told to
//! stop: then the group is stopped ([`group::stop`]), and the check is red or interrupted. When its
//! leader ends, whatever of its group is left is killed with it.

use std::fs::{self, File};
use std::io;
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use super::alone::Alone;
use super::group::{self, Bound};
use super::record::Checking;
use super::span::Span;
use symphony::children::command;
use symphony_board::sys::Stdio;

/// What a check runs within: the run's bound, and what says which check runs, and in which group,
/// for as long as it does (`running.json`).
#[derive(Clone, Copy)]
pub(crate) struct Within<'a> {
    pub(crate) bound: Bound<'a>,
    pub(crate) running: Option<&'a Says<'a>>,
}

/// What says the check that runs, or that none does.
pub(crate) type Says<'a> = dyn Fn(Option<&Checking>) + 'a;

#[cfg(test)]
impl<'a> Within<'a> {
    /// `bound`, with nothing said of the check.
    pub(crate) fn bound(bound: Bound<'a>) -> Within<'a> {
        Within {
            bound,
            running: None,
        }
    }
}

/// The script a checkout records its builds and tests through.
const RECORD: &str = "tools/record.sh";

/// One check to run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Step {
    /// Its name: what the board's red task for it is keyed by, and its log is named.
    pub(crate) name: String,
    /// The command as the record and the red task say it.
    pub(crate) command: String,
    /// What runs.
    pub(crate) argv: Vec<String>,
    /// Variables set for it.
    pub(crate) set: Vec<(String, String)>,
}

impl Step {
    /// The check `name` running `command` as `sh -c` runs it, in the candidate's checkout.
    pub(crate) fn shell(name: &str, command: &str) -> Step {
        Step {
            name: name.to_string(),
            command: command.to_string(),
            argv: vec!["sh".into(), "-c".into(), command.to_string()],
            set: Vec::new(),
        }
    }

    /// It, running the steps `names` of it alone: its command with their names after it, as
    /// `just verify STEP...` takes them, each one word to the command whatever it holds.
    pub(crate) fn only(&self, names: &[String]) -> Step {
        let argv = match self.argv.as_slice() {
            [shell, flag, command] if flag == "-c" => {
                let mut argv = vec![
                    shell.clone(),
                    flag.clone(),
                    format!("{command} \"$@\""),
                    shell.clone(),
                ];
                argv.extend(names.iter().cloned());
                argv
            }
            argv => argv.iter().chain(names).cloned().collect(),
        };
        Step {
            command: format!("{} {}", self.command, names.join(" ")),
            argv,
            ..self.clone()
        }
    }
}

/// How a check ran.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Ran {
    pub(crate) name: String,
    pub(crate) command: String,
    pub(crate) exit_code: Option<i32>,
    pub(crate) signal: Option<i32>,
    /// How long it took by the wall clock, a sleep of the machine counted ([`super::span`]).
    pub(crate) seconds: f64,
    /// Its whole output.
    pub(crate) log: PathBuf,
    /// The steps it ran and listed, each as a check of its own ([`super::steps`]); none for a
    /// check that lists none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) steps: Vec<Ran>,
    /// Why it could not run or its process group could not be cleaned up.
    #[serde(default)]
    pub(crate) error: Option<String>,
    /// Whether it was stopped when the run's time was up.
    #[serde(default)]
    pub(crate) timed_out: bool,
    /// Whether it was stopped because the run was told to stop (HUP, INT or TERM).
    #[serde(default)]
    pub(crate) interrupted: bool,
    /// The machine's load when it ended; none when it never ran or the system did not say.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) load: Option<Load>,
    /// Its failures rerun alone by the run, when it is a red test run of the lanes
    /// ([`super::alone`]): each rerun, or why none was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) alone: Option<Alone>,
}

/// The machine's load at a moment: its load averages over 1, 5 and 15 minutes and the processors
/// it has, so a red that the machine's load made can be told from one the code made.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Load {
    pub(crate) averages: [f64; 3],
    pub(crate) processors: usize,
}

impl Load {
    /// The load now; none when the system does not say it.
    pub(crate) fn now() -> Option<Load> {
        let mut averages = [0.0_f64; 3];
        // SAFETY: getloadavg writes at most `nelem` doubles, and `averages` has room for 3.
        let said = unsafe { libc::getloadavg(averages.as_mut_ptr(), 3) };
        if said != 3 {
            return None;
        }
        let processors = std::thread::available_parallelism().map_or(0, std::num::NonZero::get);
        Some(Load {
            averages,
            processors,
        })
    }

    /// As a red task says it: `31.92, 70.68, 171.07 (over 1, 5 and 15 minutes) on 10 processors`.
    pub(crate) fn said(&self) -> String {
        let [one, five, fifteen] = self.averages;
        let on = match self.processors {
            0 => String::new(),
            1 => " on 1 processor".into(),
            n => format!(" on {n} processors"),
        };
        format!("{one:.2}, {five:.2}, {fifteen:.2} (over 1, 5 and 15 minutes){on}")
    }
}

impl Ran {
    /// Whether it passed, as the release is judged: it ran to its end and exited 0, and every step
    /// it listed passed; or it is red only in failures that each passed when the run reran it alone
    /// ([`super::alone`]): its own, its lanes' reds being those failures, or, for a check that
    /// listed its steps, those of a step of it, every other step green.
    pub(crate) fn green(&self) -> bool {
        if self.error.is_some() || self.timed_out || self.interrupted || self.exit_code.is_none() {
            return false;
        }
        if self.alone.as_ref().is_some_and(Alone::passed) {
            return true;
        }
        (self.exit_code == Some(0) || self.passed_alone_in_steps())
            && self.steps.iter().all(Ran::green)
    }

    /// Whether a step it listed is green only by its failures passing alone: a check that exited
    /// red for that step is no red when every other step is green.
    fn passed_alone_in_steps(&self) -> bool {
        (self.steps.iter()).any(|step| step.alone.as_ref().is_some_and(Alone::passed))
    }

    /// How it ended, in a few words: `exit 0`, `exit 101`, `signal 9`, stopped at the run's time or
    /// when the run was told to stop, or why it never ran.
    pub(crate) fn ended(&self) -> String {
        if let Some(error) = &self.error {
            if self.exit_code.is_some()
                || self.signal.is_some()
                || self.interrupted
                || self.timed_out
            {
                return format!("failed: {error}");
            }
            return format!("did not run: {error}");
        }
        if self.interrupted {
            return format!(
                "interrupted: stopped after {:.0} s, when the run was told to stop",
                self.seconds
            );
        }
        if self.timed_out {
            return format!(
                "timed out: stopped after {:.0} s, when the run's time was up",
                self.seconds
            );
        }
        match (self.exit_code, self.signal) {
            (Some(code), _) if code != 0 && self.green() => {
                format!("exit {code}, every failure passed alone")
            }
            (Some(code), _) => format!("exit {code}"),
            (None, Some(signal)) => format!("signal {signal}"),
            (None, None) => "ended without a status".into(),
        }
    }

    /// The check `step` red before it ran, for `error`, its log at `log`.
    pub(crate) fn never(step: &Step, log: &Path, error: String) -> Ran {
        let _ = fs::write(log, format!("$ {}\n{error}\n", step.command));
        Ran {
            name: step.name.clone(),
            command: step.command.clone(),
            exit_code: None,
            signal: None,
            seconds: 0.0,
            log: log.to_path_buf(),
            steps: Vec::new(),
            error: Some(error),
            timed_out: false,
            interrupted: false,
            load: None,
            alone: None,
        }
    }
}

/// What runs for `argv`, recorded as `label` through the checkout's script when `checkout` has one.
fn recorded(checkout: &Path, label: &str, argv: &[String]) -> Vec<String> {
    let script = checkout.join(RECORD);
    if !script.is_file() {
        return argv.to_vec();
    }
    let mut line = vec![
        "bash".to_string(),
        script.to_string_lossy().into_owned(),
        "test".into(),
        label.to_string(),
    ];
    line.extend(argv.iter().cloned());
    line
}

/// Run `step` in the candidate's checkout at `checkout`, within `within`, its output in `log`.
pub(crate) fn run(step: &Step, checkout: &Path, log: &Path, within: Within<'_>) -> Ran {
    let line = recorded(checkout, &format!("release {}", step.name), &step.argv);
    let opened = File::create(log).and_then(|mut file| {
        writeln!(file, "$ {}", step.command)?;
        let err = file.try_clone()?;
        Ok((file, err))
    });
    let (out, err) = match opened {
        Ok(files) => files,
        Err(e) => {
            return Ran::never(
                step,
                log,
                format!("cannot write its log {}: {e}", log.display()),
            );
        }
    };
    // With every signal at its default, as launchd or cron starts a job: a run started as a
    // background job of a non-interactive shell (INT ignored) or under `nohup` (HUP) keeps that for
    // itself, but a check's tests that end a child by such a signal must see it act.
    let mut child = command(&line[0]);
    child
        .args(&line[1..])
        .current_dir(checkout)
        .stdin(Stdio::null())
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err))
        .process_group(0)
        .default_signals()
        // A check lists its steps only in the folder its run names for it.
        .env_remove(super::steps::VAR);
    for (name, value) in &step.set {
        child.env(name, value);
    }
    let started = Span::begin();
    let mut child = match child.spawn() {
        Ok(child) => child,
        Err(e) => return Ran::never(step, log, format!("cannot start {}: {e}", line[0])),
    };
    let pid = child.id();
    if let Some(running) = within.running {
        running(Some(&Checking {
            name: step.name.clone(),
            group: i64::from(pid),
            start: group::stamp(i64::from(pid)),
        }));
    }
    let (mut timed_out, mut interrupted) = (false, false);
    let mut cleanup = Ok(());
    while !group::ended(pid) {
        interrupted = within.bound.interrupted();
        timed_out = !interrupted && Instant::now() >= within.bound.until;
        if interrupted || timed_out {
            cleanup = group::stop(pid, group::GRACE);
            break;
        }
        #[expect(
            clippy::disallowed_methods,
            reason = "no-timed-waits baseline: looks again for a check's process group to end, which no wait reports"
        )]
        std::thread::sleep(group::POLL);
    }
    // The load as it ended, before what it left is stopped: what a red's reader judges it under.
    let load = Load::now();
    // What its leader left of its group goes with it, before the leader is reaped and its id freed.
    if !interrupted && !timed_out {
        cleanup = group::kill(pid);
    }
    // An unsuccessful cleanup must not become an unbounded blocking wait on a live leader.
    let status = if cleanup.is_ok() || group::ended(pid) {
        child.wait().map(Some)
    } else {
        child.try_wait()
    };
    let (status, error) = outcome(pid, status, cleanup);
    if let Some(running) = within.running {
        running(None);
    }
    // By the wall clock, as its log's times and its steps' own seconds are.
    let seconds = started.took().seconds;
    let why = if interrupted {
        Some("the run was told to stop")
    } else if timed_out {
        Some("the run's time was up")
    } else {
        None
    };
    if let Some(why) = why {
        let _ = fs::OpenOptions::new()
            .append(true)
            .open(log)
            .and_then(|mut file| writeln!(file, "(stopped after {seconds:.0} s: {why})"));
    }
    if let Some(error) = &error {
        let _ = fs::OpenOptions::new()
            .append(true)
            .open(log)
            .and_then(|mut file| writeln!(file, "({error})"));
    }
    Ran {
        name: step.name.clone(),
        command: step.command.clone(),
        exit_code: status.and_then(|status| status.code()),
        signal: status.and_then(|status| status.signal()),
        seconds,
        log: log.to_path_buf(),
        steps: Vec::new(),
        error,
        timed_out,
        interrupted,
        load,
        alone: None,
    }
}

/// Keep the leader's status even when its group's cleanup failed. A reap error belongs beside
/// the cleanup error in the existing log, never in a replacement that discards the check's output.
fn outcome(
    pid: u32,
    status: io::Result<Option<ExitStatus>>,
    cleanup: io::Result<()>,
) -> (Option<ExitStatus>, Option<String>) {
    let mut errors: Vec<String> = cleanup
        .err()
        .map(|error| error.to_string())
        .into_iter()
        .collect();
    let status = match status {
        Ok(status) => status,
        Err(error) => {
            errors.push(format!("cannot wait for process {pid}: {error}"));
            None
        }
    };
    (status, (!errors.is_empty()).then(|| errors.join("; ")))
}

/// The end of the log at `path`: its last `lines` lines within its last `bytes` bytes, a line cut
/// by that bound left out. Empty when it cannot be read.
pub(crate) fn tail(path: &Path, lines: usize, bytes: u64) -> String {
    let Ok(mut file) = File::open(path) else {
        return String::new();
    };
    let len = file.metadata().map_or(0, |meta| meta.len());
    let from = len.saturating_sub(bytes);
    if file.seek(SeekFrom::Start(from)).is_err() {
        return String::new();
    }
    let mut read = Vec::new();
    if file.read_to_end(&mut read).is_err() {
        return String::new();
    }
    let text = String::from_utf8_lossy(&read);
    let text = if from > 0 {
        text.split_once('\n').map_or("", |(_, rest)| rest)
    } else {
        &text
    };
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::super::waits::{far, gone, pid_written, written_pid};
    use super::*;
    use crate::test_tree::Tree;
    use std::cell::RefCell;

    fn script(tree: &Tree) {
        tree.write(
            RECORD,
            "#!/bin/sh\nprintf '%s|%s|%s\\n' \"$1\" \"$2\" \"$PWD\" >> \"$RECORDED\"\nshift 2\nexec \"$@\"\n",
        );
    }

    /// `step` run within a bound only a stall reaches.
    fn checked(step: &Step, checkout: &Path, log: &Path) -> Ran {
        run(step, checkout, log, Within::bound(far()))
    }

    /// A step that starts a helper which outlives any wait of a test, says `started` and writes the
    /// helper's pid in `helper`, then waits for it.
    fn helper_step(helper: &Path) -> Step {
        Step::shell(
            "verify",
            &format!(
                "sleep 1000 & h=$!; echo started; echo $h > '{}'; wait",
                helper.display()
            ),
        )
    }

    #[test]
    fn a_check_run_for_some_of_its_steps_hands_each_name_to_its_command_as_one_word() {
        let tree = Tree::new();
        let step = Step::shell("verify", "printf '<%s>' \"$0\"");
        let names = vec!["lint".to_string(), "a step; rm -f x".to_string()];
        let only = step.only(&names);
        assert_eq!(only.name, "verify");
        assert_eq!(only.command, "printf '<%s>' \"$0\" lint a step; rm -f x");
        let ran = checked(&only, &tree.root, &tree.path("verify.log"));
        assert!(ran.green(), "{ran:?}");
        let log = fs::read_to_string(tree.path("verify.log")).unwrap();
        assert!(log.contains("<sh><lint><a step; rm -f x>"), "{log}");
        // A step not run through a shell takes the names after its own words.
        let direct = Step {
            argv: vec!["just".into(), "verify".into()],
            ..step
        };
        assert_eq!(
            direct.only(&names[..1]).argv,
            vec!["just".to_string(), "verify".into(), "lint".into()]
        );
    }

    #[test]
    fn a_check_past_the_runs_time_is_stopped_with_its_group_and_is_red() {
        let tree = Tree::new();
        let helper = tree.path("helper.pid");
        // The run's time is up once the check has started its helper: what says the check runs is
        // told before the run looks at its time, and waits there for the helper's pid.
        let running = |check: Option<&Checking>| {
            if check.is_some() {
                written_pid(&helper);
            }
        };
        let within = Within {
            bound: Bound::until(Instant::now()),
            running: Some(&running),
        };
        let ran = run(
            &helper_step(&helper),
            &tree.root,
            &tree.path("verify.log"),
            within,
        );
        assert!(ran.timed_out && !ran.interrupted && !ran.green(), "{ran:?}");
        let said = Ran {
            seconds: 1.0,
            ..ran.clone()
        };
        assert_eq!(
            said.ended(),
            "timed out: stopped after 1 s, when the run's time was up"
        );
        let log = fs::read_to_string(tree.path("verify.log")).unwrap();
        assert!(log.contains("started\n"), "{log}");
        assert!(log.contains("s: the run's time was up)\n"), "{log}");
        gone(written_pid(&helper), "what the check started");
    }

    #[test]
    fn what_a_check_leaves_running_in_its_group_goes_when_it_ends() {
        let tree = Tree::new();
        let helper = tree.path("helper.pid");
        let step = Step::shell(
            "verify",
            &format!("sleep 1000 & echo $! > '{}'", helper.display()),
        );
        let ran = checked(&step, &tree.root, &tree.path("verify.log"));
        assert!(ran.green(), "{ran:?}");
        gone(written_pid(&helper), "what the check left running");
    }

    #[test]
    fn a_check_of_a_run_told_to_stop_is_stopped_with_its_group() {
        let tree = Tree::new();
        let helper = tree.path("helper.pid");
        // The run is told to stop once the check has started its helper.
        let heard = || pid_written(&helper);
        let within = Within::bound(Bound::hearing(far().until, &heard));
        let ran = run(
            &helper_step(&helper),
            &tree.root,
            &tree.path("verify.log"),
            within,
        );
        assert!(ran.interrupted && !ran.timed_out && !ran.green(), "{ran:?}");
        let said = Ran {
            seconds: 2.0,
            ..ran.clone()
        };
        assert_eq!(
            said.ended(),
            "interrupted: stopped after 2 s, when the run was told to stop"
        );
        let log = fs::read_to_string(tree.path("verify.log")).unwrap();
        assert!(log.contains("started\n"), "{log}");
        assert!(log.contains("s: the run was told to stop)\n"), "{log}");
        gone(written_pid(&helper), "what the check started");
    }

    #[test]
    fn a_stopping_check_drains_a_helper_forked_by_its_term_handler_and_keeps_its_status() {
        let tree = Tree::new();
        let ready = tree.path("ready");
        let helper = tree.path("helper.pid");
        // `ready` is written by the check's foreground child, which the stop's TERM to the group
        // ends whenever it comes; bash runs its trap once that command completes. A shell that says
        // it is ready and then blocks in a builtin can take the TERM before the block starts, and
        // never runs the trap.
        let script = format!(
            "trap 'sleep 1000 & echo $! > \"{}\"; echo term; exit 37' TERM; \
             sh -c 'echo ready > \"{}\"; exec sleep 1000'; exit 1",
            helper.display(),
            ready.display()
        );
        let step = Step {
            argv: vec!["bash".into(), "-c".into(), script.clone()],
            ..Step::shell("verify", &script)
        };
        let heard = || ready.exists();
        let checking = RefCell::new(None);
        let running = |check: Option<&Checking>| {
            if let Some(check) = check {
                *checking.borrow_mut() = Some(check.clone());
            }
        };
        let ran = run(
            &step,
            &tree.root,
            &tree.path("verify.log"),
            Within {
                bound: Bound::hearing(far().until, &heard),
                running: Some(&running),
            },
        );
        assert!(ran.interrupted && !ran.timed_out && !ran.green(), "{ran:?}");
        assert_eq!(
            (ran.exit_code, ran.signal, ran.error),
            (Some(37), None, None)
        );
        let helper = written_pid(&helper);
        let check = checking.into_inner().unwrap();
        test_time::until_true(
            format!("the stopped check's group and forked helper {helper} gone"),
            test_time::FLOOR,
            || !group::lives(check.group, check.start.as_ref()),
        )
        .unwrap_or_else(|stall| panic!("{stall}"));
        let log = fs::read_to_string(tree.path("verify.log")).unwrap();
        assert!(log.contains("\nterm\n"), "{log}");
        assert!(log.ends_with("s: the run was told to stop)\n"), "{log}");
    }

    #[test]
    fn cleanup_errors_keep_a_checks_status_and_make_it_red_alongside_any_reap_error() {
        let status = ExitStatus::from_raw(37 << 8);
        assert_eq!(outcome(10, Ok(Some(status)), Ok(())), (Some(status), None));
        let (kept, error) = outcome(
            10,
            Ok(Some(status)),
            Err(io::Error::other("live members remain")),
        );
        assert_eq!(
            (kept, error.as_deref()),
            (Some(status), Some("live members remain"))
        );
        let tree = Tree::new();
        let ran = Ran {
            exit_code: kept.and_then(|status| status.code()),
            error,
            ..Ran::never(
                &Step::shell("verify", "true"),
                &tree.path("verify.log"),
                "not started".into(),
            )
        };
        assert!(!ran.green());
        assert_eq!(ran.ended(), "failed: live members remain");
        assert_eq!(
            serde_json::from_value::<Ran>(serde_json::to_value(&ran).unwrap()).unwrap(),
            ran
        );
        assert_eq!(
            outcome(10, Ok(None), Err(io::Error::other("not stopped"))),
            (None, Some("not stopped".into()))
        );
        assert_eq!(
            outcome(
                10,
                Err(io::Error::other("reap refused")),
                Err(io::Error::other("kill refused"))
            ),
            (
                None,
                Some("kill refused; cannot wait for process 10: reap refused".into())
            )
        );
        assert_eq!(
            outcome(10, Err(io::Error::other("reap refused")), Ok(())),
            (
                None,
                Some("cannot wait for process 10: reap refused".into())
            )
        );
    }

    #[test]
    fn a_check_inherits_no_descriptor_of_the_runs_and_its_group_is_said_while_it_runs() {
        let tree = Tree::new();
        // The run's lock, held here as the run holds it.
        let lock = File::create(tree.path("run.lock")).unwrap();
        lock.lock().unwrap();
        let fd = std::os::fd::AsRawFd::as_raw_fd(&lock);
        let leader = tree.path("leader.pid");
        let step = Step::shell(
            "verify",
            &format!("echo $$ > '{}'; test ! -e /dev/fd/{fd}", leader.display()),
        );
        let said = RefCell::new(Vec::new());
        let running = |check: Option<&Checking>| said.borrow_mut().push(check.cloned());
        let within = Within {
            bound: far(),
            running: Some(&running),
        };
        let ran = run(&step, &tree.root, &tree.path("verify.log"), within);
        assert!(ran.green(), "the lock is not open in the check: {ran:?}");
        let leader: i64 = fs::read_to_string(&leader).unwrap().trim().parse().unwrap();
        let said = said.into_inner();
        assert_eq!(said.len(), 2, "{said:?}");
        let checking = said[0].as_ref().unwrap();
        assert_eq!(
            (checking.name.as_str(), checking.group),
            ("verify", leader),
            "while it runs: its group, which its leader's id names"
        );
        assert_eq!(said[1], None, "once it ended: none");
    }

    #[test]
    fn a_check_runs_in_the_checkout_with_its_output_in_its_log() {
        let tree = Tree::new();
        let root = tree.root.canonicalize().unwrap();
        let log = tree.path("verify.log");
        let ran = checked(
            &Step::shell("verify", "echo out; echo err >&2; pwd; exit 3"),
            &root,
            &log,
        );
        assert_eq!(ran.exit_code, Some(3));
        assert_eq!(ran.signal, None);
        assert!(!ran.green());
        assert_eq!(ran.ended(), "exit 3");
        let text = fs::read_to_string(&log).unwrap();
        assert!(
            text.starts_with("$ echo out; echo err >&2; pwd; exit 3\n"),
            "{text}"
        );
        for said in ["out\n", "err\n", &format!("{}\n", root.display())] {
            assert!(text.contains(said), "{said:?} in {text}");
        }
        let green = checked(&Step::shell("lint", "true"), &root, &tree.path("lint.log"));
        assert!(green.green(), "{green:?}");
        assert_eq!(green.ended(), "exit 0");
    }

    /// A release run a land started inherits how that land was asked for: the unasked recipes'
    /// mark and the `land` recipe's. Its checks run without either (`sys::MARKS`), or every test
    /// of a command that writes outside the checkout is refused there, and a check could land. Nor
    /// does a check see a steps folder it inherited ([`super::super::steps::VAR`]): only the one
    /// its run names for it.
    #[test]
    fn a_check_runs_without_the_marks_of_how_the_run_was_asked_for() {
        // The marks are this test's own process's, set when it is started: set here, they would
        // reach every other test's children too.
        const CASE: &str = "SYMPHONY_RELEASE_MARKS_CASE";
        if std::env::var_os(CASE).is_some() {
            let tree = Tree::new();
            let log = tree.path("verify.log");
            let ran = checked(
                &Step::shell(
                    "verify",
                    "echo \"marks ${SYMPHONY_UNASKED-unset} ${SYMPHONY_LAND-unset} \
                     ${SYMPHONY_RELEASE_KEPT-unset} ${SYMPHONY_CHECK_STEPS-unset}\"",
                ),
                &tree.root,
                &log,
            );
            assert!(ran.green(), "{ran:?}");
            let text = fs::read_to_string(&log).unwrap();
            assert!(text.contains("\nmarks unset unset kept unset\n"), "{text}");
            return;
        }
        // This test's name as the harness lists it: its module's path below the crate.
        let (_, module) = module_path!().split_once("::").unwrap();
        let name = format!("{module}::a_check_runs_without_the_marks_of_how_the_run_was_asked_for");
        let done = symphony_board::sys::output(
            symphony_board::sys::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", name.as_str()])
                .env(CASE, "1")
                .envs(symphony_board::sys::MARKS.map(|name| (name, "1")))
                .env("SYMPHONY_RELEASE_KEPT", "kept")
                .env(super::super::steps::VAR, "/elsewhere"),
        )
        .unwrap();
        let said = String::from_utf8_lossy(&done.stdout);
        assert!(
            done.status.success() && said.contains("1 passed"),
            "{said}{}",
            String::from_utf8_lossy(&done.stderr)
        );
    }

    #[test]
    fn a_check_killed_by_a_signal_says_which() {
        let tree = Tree::new();
        let ran = checked(
            &Step::shell("verify", "kill -TERM $$"),
            &tree.root,
            &tree.path("verify.log"),
        );
        assert_eq!((ran.exit_code, ran.signal), (None, Some(15)));
        assert_eq!(ran.ended(), "signal 15");
        assert!(!ran.green());
    }

    #[test]
    fn a_check_goes_through_the_checkouts_record_script_as_one_test_operation() {
        let tree = Tree::new();
        let root = tree.root.canonicalize().unwrap();
        script(&tree);
        let recorded = tree.path("recorded");
        let mut step = Step::shell("verify", "echo checked");
        step.set
            .push(("RECORDED".into(), recorded.to_string_lossy().into()));
        let ran = checked(&step, &root, &tree.path("verify.log"));
        assert!(ran.green(), "{ran:?}");
        assert_eq!(
            fs::read_to_string(&recorded).unwrap(),
            format!("test|release verify|{}\n", root.display())
        );
        assert!(
            fs::read_to_string(tree.path("verify.log"))
                .unwrap()
                .contains("checked\n")
        );
    }

    #[test]
    fn a_check_that_cannot_start_is_red_and_says_why() {
        let tree = Tree::new();
        let step = Step {
            argv: vec!["/no/such/program".into()],
            ..Step::shell("verify", "/no/such/program")
        };
        let ran = checked(&step, &tree.root, &tree.path("verify.log"));
        assert!(!ran.green());
        assert!(
            ran.ended()
                .starts_with("did not run: cannot start /no/such/program"),
            "{ran:?}"
        );
        let unwritable = checked(
            &Step::shell("verify", "true"),
            &tree.root,
            &tree.path("missing/folder/verify.log"),
        );
        assert!(!unwritable.green());
        assert!(
            unwritable.ended().contains("cannot write its log"),
            "{unwritable:?}"
        );
    }

    #[test]
    fn a_check_that_ran_keeps_the_load_it_ended_under_and_one_that_never_ran_none() {
        let tree = Tree::new();
        let ran = checked(
            &Step::shell("verify", "exit 1"),
            &tree.root,
            &tree.path("verify.log"),
        );
        let load = ran.load.expect("this system says its load");
        assert!(load.processors >= 1, "{load:?}");
        assert!(
            load.averages.iter().all(|a| a.is_finite() && *a >= 0.0),
            "{load:?}"
        );
        let never = Ran::never(
            &Step::shell("verify", "true"),
            &tree.path("never.log"),
            "no checkout".into(),
        );
        assert_eq!(never.load, None);
    }

    #[test]
    fn a_load_is_said_with_its_three_averages_and_the_processors() {
        let load = |processors| Load {
            averages: [31.916, 70.5, 171.0],
            processors,
        };
        assert_eq!(
            load(10).said(),
            "31.92, 70.50, 171.00 (over 1, 5 and 15 minutes) on 10 processors"
        );
        assert_eq!(
            load(1).said(),
            "31.92, 70.50, 171.00 (over 1, 5 and 15 minutes) on 1 processor"
        );
        assert_eq!(
            load(0).said(),
            "31.92, 70.50, 171.00 (over 1, 5 and 15 minutes)",
            "processors the system did not say are left out"
        );
    }

    #[test]
    fn a_record_without_a_load_reads_and_none_is_not_written() {
        let ran = Ran::never(
            &Step::shell("verify", "true"),
            Path::new("/no/such/folder/verify.log"),
            "no checkout".into(),
        );
        let written = serde_json::to_value(&ran).unwrap();
        assert!(written.get("load").is_none(), "{written}");
        let read: Ran = serde_json::from_value(written).unwrap();
        assert_eq!(read, ran, "a record from before the load reads as none");
        let loaded = Ran {
            load: Some(Load {
                averages: [1.0, 2.0, 3.0],
                processors: 4,
            }),
            ..ran
        };
        let written = serde_json::to_value(&loaded).unwrap();
        assert_eq!(
            written["load"],
            serde_json::json!({"averages": [1.0, 2.0, 3.0], "processors": 4})
        );
        assert_eq!(serde_json::from_value::<Ran>(written).unwrap(), loaded);
    }

    #[test]
    fn the_tail_of_a_log_is_its_last_whole_lines() {
        let tree = Tree::new();
        let path = tree.write("log", "one\ntwo\nthree\nfour\n");
        assert_eq!(tail(&path, 2, 1000), "three\nfour");
        assert_eq!(tail(&path, 10, 1000), "one\ntwo\nthree\nfour");
        assert_eq!(
            tail(&path, 10, 12),
            "three\nfour",
            "a line cut by the bound is left out"
        );
        assert_eq!(tail(&tree.path("none"), 10, 100), "");
    }
}
