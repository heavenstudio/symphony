//! A check of the nightly: one command run in the candidate's checkout, its output in a log of its
//! own, recorded through the candidate's `tools/record.sh` as one test operation of the board's
//! recorder (`tasks run`, docs/MACHINE_WORK.md), labelled `nightly NAME`. The steps inside it are
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
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::{Deserialize, Serialize};

use super::group::{self, Bound};
use super::record::Checking;
use crate::children::command;
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
    /// The folder it runs in; the recorder still runs in the candidate's checkout.
    pub(crate) cwd: Option<PathBuf>,
    /// Variables set for it, and variables removed.
    pub(crate) set: Vec<(String, String)>,
    pub(crate) unset: Vec<String>,
}

impl Step {
    /// The check `name` running `command` as `sh -c` runs it, in the candidate's checkout.
    pub(crate) fn shell(name: &str, command: &str) -> Step {
        Step {
            name: name.to_string(),
            command: command.to_string(),
            argv: vec!["sh".into(), "-c".into(), command.to_string()],
            cwd: None,
            set: Vec::new(),
            unset: Vec::new(),
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
    pub(crate) seconds: f64,
    /// Its whole output.
    pub(crate) log: PathBuf,
    /// Why it never ran, when it did not.
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
    /// Whether it passed: it ran to its end and exited 0.
    pub(crate) fn green(&self) -> bool {
        self.error.is_none() && !self.timed_out && !self.interrupted && self.exit_code == Some(0)
    }

    /// How it ended, in a few words: `exit 0`, `exit 101`, `signal 9`, stopped at the run's time or
    /// when the run was told to stop, or why it never ran.
    pub(crate) fn ended(&self) -> String {
        if let Some(error) = &self.error {
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
            error: Some(error),
            timed_out: false,
            interrupted: false,
            load: None,
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

/// What `argv` becomes to run in `cwd` while its recorder runs in the checkout: the working folder
/// a record keeps, and the board's work folder it is written to, are the checkout's.
fn moved(cwd: Option<&Path>, argv: &[String]) -> Vec<String> {
    match cwd {
        None => argv.to_vec(),
        Some(cwd) => {
            let mut line = vec![
                "sh".to_string(),
                "-c".into(),
                "cd \"$1\" && shift && exec \"$@\"".into(),
                "nightly".into(),
                cwd.to_string_lossy().into_owned(),
            ];
            line.extend(argv.iter().cloned());
            line
        }
    }
}

/// Run `step` in the candidate's checkout at `checkout`, within `within`, its output in `log`.
pub(crate) fn run(step: &Step, checkout: &Path, log: &Path, within: Within<'_>) -> Ran {
    let line = recorded(
        checkout,
        &format!("nightly {}", step.name),
        &moved(step.cwd.as_deref(), &step.argv),
    );
    let opened = File::create(log).and_then(|mut file| {
        writeln!(file, "$ {}", step.command)?;
        if let Some(cwd) = &step.cwd {
            writeln!(file, "(in {})", cwd.display())?;
        }
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
        .default_signals();
    for (name, value) in &step.set {
        child.env(name, value);
    }
    for name in &step.unset {
        child.env_remove(name);
    }
    let started = Instant::now();
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
    while !group::ended(pid) {
        interrupted = within.bound.interrupted();
        timed_out = !interrupted && Instant::now() >= within.bound.until;
        if interrupted || timed_out {
            group::stop(pid, group::GRACE);
            break;
        }
        std::thread::sleep(group::POLL);
    }
    // The load as it ended, before what it left is stopped: what a red's reader judges it under.
    let load = Load::now();
    // What its leader left of its group goes with it, before the leader is reaped and its id freed.
    group::signal(
        libc::pid_t::try_from(pid).unwrap_or(libc::pid_t::MAX),
        libc::SIGKILL,
    );
    let status = child.wait();
    if let Some(running) = within.running {
        running(None);
    }
    let seconds = started.elapsed().as_secs_f64();
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
    match status {
        Ok(status) => Ran {
            name: step.name.clone(),
            command: step.command.clone(),
            exit_code: status.code(),
            signal: status.signal(),
            seconds,
            log: log.to_path_buf(),
            error: None,
            timed_out,
            interrupted,
            load,
        },
        Err(e) => Ran::never(step, log, format!("cannot wait for {}: {e}", line[0])),
    }
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
    use super::*;
    use crate::test_tree::Tree;
    use std::cell::RefCell;
    use std::time::Duration;

    fn script(tree: &Tree) {
        tree.write(
            RECORD,
            "#!/bin/sh\nprintf '%s|%s|%s\\n' \"$1\" \"$2\" \"$PWD\" >> \"$RECORDED\"\nshift 2\nexec \"$@\"\n",
        );
    }

    /// `step` run with a minute to spare.
    fn checked(step: &Step, checkout: &Path, log: &Path) -> Ran {
        let within = Within::bound(Bound::until(Instant::now() + Duration::from_secs(60)));
        run(step, checkout, log, within)
    }

    /// Whether the process whose pid `pid` holds still lives after a few seconds' wait for it to
    /// end and be reaped by whoever inherited it.
    fn alive(pid: &Path) -> bool {
        let pid: i64 = fs::read_to_string(pid).unwrap().trim().parse().unwrap();
        let until = Instant::now() + std::time::Duration::from_secs(10);
        while symphony_board::sys::pid_alive(pid) && Instant::now() < until {
            std::thread::sleep(group::POLL);
        }
        symphony_board::sys::pid_alive(pid)
    }

    #[test]
    fn a_check_past_the_runs_time_is_stopped_with_its_group_and_is_red() {
        let tree = Tree::new();
        let helper = tree.path("helper.pid");
        let step = Step::shell(
            "verify",
            &format!(
                "sleep 30 & echo $! > '{}'; echo started; wait",
                helper.display()
            ),
        );
        let started = Instant::now();
        let within = Within::bound(Bound::until(Instant::now() + Duration::from_millis(500)));
        let ran = run(&step, &tree.root, &tree.path("verify.log"), within);
        assert!(started.elapsed().as_secs() < 20, "{ran:?}");
        assert!(ran.timed_out && !ran.green(), "{ran:?}");
        assert!(
            ran.ended()
                .starts_with("timed out: stopped after 1 s, when the run's time was up")
                || ran
                    .ended()
                    .starts_with("timed out: stopped after 0 s, when the run's time was up"),
            "{ran:?}"
        );
        let log = fs::read_to_string(tree.path("verify.log")).unwrap();
        assert!(log.contains("started\n"), "{log}");
        assert!(log.contains("s: the run's time was up)\n"), "{log}");
        assert!(!alive(&helper), "what the check started went with it");
    }

    #[test]
    fn what_a_check_leaves_running_in_its_group_goes_when_it_ends() {
        let tree = Tree::new();
        let helper = tree.path("helper.pid");
        let step = Step::shell(
            "verify",
            &format!("sleep 30 & echo $! > '{}'", helper.display()),
        );
        let started = Instant::now();
        let ran = checked(&step, &tree.root, &tree.path("verify.log"));
        assert!(ran.green(), "{ran:?}");
        assert!(started.elapsed().as_secs() < 20, "{ran:?}");
        assert!(!alive(&helper));
    }

    #[test]
    fn a_check_of_a_run_told_to_stop_is_stopped_with_its_group() {
        let tree = Tree::new();
        let helper = tree.path("helper.pid");
        let step = Step::shell(
            "verify",
            &format!("sleep 30 & echo $! > '{}'; wait", helper.display()),
        );
        let started = Instant::now();
        let heard = move || started.elapsed() >= Duration::from_millis(300);
        let within = Within::bound(Bound::hearing(
            Instant::now() + Duration::from_secs(60),
            &heard,
        ));
        let ran = run(&step, &tree.root, &tree.path("verify.log"), within);
        assert!(started.elapsed().as_secs() < 20, "{ran:?}");
        assert!(ran.interrupted && !ran.timed_out && !ran.green(), "{ran:?}");
        assert!(
            ran.ended().starts_with("interrupted: stopped after "),
            "{ran:?}"
        );
        let log = fs::read_to_string(tree.path("verify.log")).unwrap();
        assert!(log.contains("s: the run was told to stop)\n"), "{log}");
        assert!(!alive(&helper), "what the check started went with it");
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
            bound: Bound::until(Instant::now() + Duration::from_secs(60)),
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
            format!("test|nightly verify|{}\n", root.display())
        );
        assert!(
            fs::read_to_string(tree.path("verify.log"))
                .unwrap()
                .contains("checked\n")
        );
    }

    #[test]
    fn a_check_in_another_folder_is_recorded_from_the_checkout_and_runs_there() {
        let tree = Tree::new();
        let root = tree.root.canonicalize().unwrap();
        script(&tree);
        let elsewhere = root.join("elsewhere");
        fs::create_dir_all(&elsewhere).unwrap();
        let recorded = tree.path("recorded");
        let step = Step {
            name: "conciliate-x".into(),
            command: "cargo test".into(),
            argv: vec!["sh".into(), "-c".into(), "pwd; echo \"$GONE-$KEPT\"".into()],
            cwd: Some(elsewhere.clone()),
            set: vec![
                ("RECORDED".into(), recorded.to_string_lossy().into()),
                ("KEPT".into(), "kept".into()),
                ("GONE".into(), "set, then removed".into()),
            ],
            unset: vec!["GONE".into()],
        };
        let ran = checked(&step, &root, &tree.path("x.log"));
        assert!(ran.green(), "{ran:?}");
        assert_eq!(
            fs::read_to_string(&recorded).unwrap(),
            format!("test|nightly conciliate-x|{}\n", root.display()),
            "the recorder runs in the checkout"
        );
        let log = fs::read_to_string(tree.path("x.log")).unwrap();
        assert!(
            log.contains(&format!("(in {})\n", elsewhere.display())),
            "{log}"
        );
        assert!(
            log.contains(&format!("{}\n-kept\n", elsewhere.display())),
            "{log}"
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
