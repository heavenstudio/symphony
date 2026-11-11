//! A check of the nightly: one command run to its end in the candidate's checkout, its output in a
//! log of its own, recorded through the candidate's `tools/record.sh` as one test operation of the
//! board's recorder (`tasks run`, docs/MACHINE_WORK.md), labelled `nightly NAME`. The steps inside
//! it are not recorded again. A candidate without the script runs it as it is.

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::children::command;
use symphony_board::sys::Stdio;

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
}

impl Ran {
    /// Whether it passed: it ran and exited 0.
    pub(crate) fn green(&self) -> bool {
        self.error.is_none() && self.exit_code == Some(0)
    }

    /// How it ended, in a few words: `exit 0`, `exit 101`, `signal 9`, or why it never ran.
    pub(crate) fn ended(&self) -> String {
        if let Some(error) = &self.error {
            return format!("did not run: {error}");
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

/// Run `step` in the candidate's checkout at `checkout`, to its end, its output in `log`.
pub(crate) fn run(step: &Step, checkout: &Path, log: &Path) -> Ran {
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
    let mut child = command(&line[0]);
    child
        .args(&line[1..])
        .current_dir(checkout)
        .stdin(Stdio::null())
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err));
    for (name, value) in &step.set {
        child.env(name, value);
    }
    for name in &step.unset {
        child.env_remove(name);
    }
    let started = Instant::now();
    let status = child.spawn().and_then(|mut child| child.wait());
    let seconds = started.elapsed().as_secs_f64();
    match status {
        Ok(status) => Ran {
            name: step.name.clone(),
            command: step.command.clone(),
            exit_code: status.code(),
            signal: status.signal(),
            seconds,
            log: log.to_path_buf(),
            error: None,
        },
        Err(e) => Ran::never(step, log, format!("cannot start {}: {e}", line[0])),
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

    fn script(tree: &Tree) {
        tree.write(
            RECORD,
            "#!/bin/sh\nprintf '%s|%s|%s\\n' \"$1\" \"$2\" \"$PWD\" >> \"$RECORDED\"\nshift 2\nexec \"$@\"\n",
        );
    }

    #[test]
    fn a_check_runs_in_the_checkout_with_its_output_in_its_log() {
        let tree = Tree::new();
        let root = tree.root.canonicalize().unwrap();
        let log = tree.path("verify.log");
        let ran = run(
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
        let green = run(&Step::shell("lint", "true"), &root, &tree.path("lint.log"));
        assert!(green.green(), "{green:?}");
        assert_eq!(green.ended(), "exit 0");
    }

    #[test]
    fn a_check_killed_by_a_signal_says_which() {
        let tree = Tree::new();
        let ran = run(
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
        let ran = run(&step, &root, &tree.path("verify.log"));
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
        let ran = run(&step, &root, &tree.path("x.log"));
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
        let ran = run(&step, &tree.root, &tree.path("verify.log"));
        assert!(!ran.green());
        assert!(
            ran.ended()
                .starts_with("did not run: cannot start /no/such/program"),
            "{ran:?}"
        );
        let unwritable = run(
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
