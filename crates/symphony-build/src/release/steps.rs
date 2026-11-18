//! The steps a check lists: a check of the run is given a folder of its own among the run's logs
//! ([`VAR`] names it), and one that lists the steps it ran there ([`FILE`]), as `verify` does its
//! lint, client check, size budgets, docs audit, crates and semver checks and tests (tools/verify.sh), is told
//! by its steps.
//! Each listed step is a check of its own in the run's record, under the check (`verify/tests`),
//! with its own status and log, and so on the board ([`super::board`]): a red step is a task of its
//! own, keyed by the check and the step (`tests/release-red-verify-tests`), closed when that step is
//! green. The check's own task is filed only for a red no step it listed holds: it was stopped,
//! never ran, ended by a signal, or ended red with every step it listed green.
//!
//! A list that does not read is taken for none, said in the run's output: the check is then told
//! as one, as a check that lists nothing is, so no red hides behind a line that does not read.

use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use super::board;
use super::checks::{self, Load, Ran, Step, Within};

/// What a list says in place of an exit status for a step the check did not run because a step
/// it needs was red (verify's tests after a red runner setup): the step is listed, so the run
/// knows it is there and not run, it is no red of its own, and a rerun always runs it.
pub(crate) const NOT_RUN: &str = "-";
/// Why such a step did not run, as its record says it.
const NEEDED: &str = "a step it needs was red";

/// Whether the step `ran` is one its check listed as not run ([`NOT_RUN`]).
pub(crate) fn not_run(ran: &Ran) -> bool {
    ran.exit_code.is_none() && ran.signal.is_none() && ran.error.as_deref() == Some(NEEDED)
}

/// What names the folder a check lists its steps in, set for each check the run starts.
pub(crate) const VAR: &str = "SYMPHONY_CHECK_STEPS";
/// The list in that folder: a line for each step as it ended, its name, exit status, seconds, log
/// and command, and the machine's load when it ended ([`said_load`]), tab-separated; a list with no
/// load field reads, its steps' load none. A log named by a relative path is in the folder. A
/// step named `STEP/PART` is a part of the step STEP listed before it (a lane of the tests): it is
/// kept in that step, whose red stays one red, and a release candidate's rerun can run it alone.
/// A step the check did not run for a red step before it is listed too, [`NOT_RUN`] its status.
pub(crate) const FILE: &str = "steps.tsv";

/// The folder the check `name` lists its steps in, among the run's `logs`: `KEY.steps`, beside its
/// log `KEY.log`, and named as nothing else there is.
pub(crate) fn folder(logs: &Path, name: &str) -> PathBuf {
    logs.join(format!("{}.steps", board::key(name)))
}

/// Run the check `step` in `checkout` as [`checks::run`] does, its log at `log`, naming it its
/// folder among the run's `logs` to list its steps in, and take the steps it listed into what it
/// ran. What keeps it from listing (a folder that cannot be made, a list that does not read) is
/// said on `out`, and the check is told as one.
pub(crate) fn run(
    step: &Step,
    checkout: &Path,
    logs: &Path,
    log: &Path,
    within: Within<'_>,
    out: &mut dyn Write,
) -> Ran {
    let folder = folder(logs, &step.name);
    let mut step = step.clone();
    match fs::create_dir_all(&folder) {
        Ok(()) => step
            .set
            .push((VAR.into(), folder.to_string_lossy().into_owned())),
        Err(e) => {
            let _ = writeln!(
                out,
                "release: `{}` is told as one check: {}",
                step.name,
                symphony::text::os_error(&e, &folder)
            );
        }
    }
    let mut ran = checks::run(&step, checkout, log, within);
    match read(&folder, &ran.name) {
        Ok(listed) => ran.steps = listed,
        Err(why) => {
            let _ = writeln!(
                out,
                "release: `{}` is told as one check: its steps do not read: {why}",
                ran.name
            );
        }
    }
    ran
}

/// The steps the check `check` listed in `folder`, oldest first, each named `CHECK/STEP`: none
/// when it listed none; an error naming the first line that does not read, or a step listed twice.
pub(crate) fn read(folder: &Path, check: &str) -> Result<Vec<Ran>, String> {
    let path = folder.join(FILE);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(symphony::text::os_error(&e, &path)),
    };
    let mut steps: Vec<Ran> = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let Some(step) = parse(line, folder, check) else {
            return Err(format!(
                "{} line {}, {line:?}, is not a step's name, exit status, seconds, log, command and \
                 load, tab-separated",
                path.display(),
                n + 1
            ));
        };
        // A name with a `/` is a part of the step named before it (`tests/browser`, a lane of
        // the tests), listed after that step and kept in it.
        let listed = step
            .name
            .strip_prefix(check)
            .and_then(|n| n.strip_prefix('/'));
        let part = listed.and_then(|listed| listed.split_once('/'));
        if part.is_some_and(|(_, part)| part.is_empty() || part.contains('/')) {
            return Err(format!(
                "{} lists `{}`, which is no step and no part of one",
                path.display(),
                step.name
            ));
        }
        let parent = part.map(|(parent, _)| format!("{check}/{parent}"));
        let among = match &parent {
            Some(parent) => match steps.iter_mut().find(|known| known.name == *parent) {
                Some(known) => &mut known.steps,
                None => {
                    return Err(format!(
                        "{} lists `{}` before the step it is a part of",
                        path.display(),
                        step.name
                    ));
                }
            },
            None => &mut steps,
        };
        if among
            .iter()
            .any(|known| board::key(&known.name) == board::key(&step.name))
        {
            return Err(format!("{} lists `{}` twice", path.display(), step.name));
        }
        among.push(step);
    }
    Ok(steps)
}

/// The step a line of the list says, of the check `check`, its log in `folder` when relative.
fn parse(line: &str, folder: &Path, check: &str) -> Option<Ran> {
    let fields: Vec<&str> = line.split('\t').collect();
    let (name, exit, seconds, log, command, load) = match fields[..] {
        [name, exit, seconds, log, command] => (name, exit, seconds, log, command, None),
        [name, exit, seconds, log, command, load] => {
            (name, exit, seconds, log, command, said_load(load)?)
        }
        _ => return None,
    };
    let seconds: f64 = seconds.parse().ok()?;
    if name.trim().is_empty() || log.is_empty() || !seconds.is_finite() || seconds < 0.0 {
        return None;
    }
    let skipped = exit == NOT_RUN;
    Some(Ran {
        name: format!("{check}/{name}"),
        command: command.to_string(),
        exit_code: if skipped {
            None
        } else {
            Some(exit.parse().ok()?)
        },
        signal: None,
        seconds,
        log: folder.join(log),
        steps: Vec::new(),
        error: skipped.then(|| NEEDED.to_string()),
        timed_out: false,
        interrupted: false,
        load,
    })
}

/// The load a list's sixth field says, the machine's load averages over 1, 5 and 15 minutes and its
/// processors, space-separated (`3.21 2.50 2.10 10`): none for an empty field, where the system did
/// not say; `None` for one that does not read.
fn said_load(field: &str) -> Option<Option<Load>> {
    if field.is_empty() {
        return Some(None);
    }
    let words: Vec<&str> = field.split(' ').collect();
    let [one, five, fifteen, processors] = words[..] else {
        return None;
    };
    let mut averages = [0.0_f64; 3];
    for (average, word) in averages.iter_mut().zip([one, five, fifteen]) {
        *average = word
            .parse()
            .ok()
            .filter(|n: &f64| n.is_finite() && *n >= 0.0)?;
    }
    Some(Some(Load {
        averages,
        processors: processors.parse().ok()?,
    }))
}

/// Whether the red of the check `ran` is more than its steps say: it is red, and it did not end on
/// its own with a status while a step it listed is red. Green is never beyond.
pub(crate) fn beyond(ran: &Ran) -> bool {
    if ran.green() {
        return false;
    }
    let ended = ran.error.is_none()
        && !ran.timed_out
        && !ran.interrupted
        && ran.signal.is_none()
        && ran.exit_code.is_some();
    !(ended && ran.steps.iter().any(|step| !step.green() && !not_run(step)))
}

/// The reds of the check `ran` as the run tells them: the check itself when its red is
/// [`beyond`] its steps, then each red step it listed.
pub(crate) fn reds(ran: &Ran) -> Vec<&Ran> {
    let mut reds: Vec<&Ran> = Vec::new();
    if beyond(ran) {
        reds.push(ran);
    }
    reds.extend((ran.steps.iter()).filter(|step| !step.green() && !not_run(step)));
    reds
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::release::waits::far;
    use crate::test_tree::Tree;

    /// The check `name`, run for `command` with an exit status of `code`, listing `steps`.
    fn check(name: &str, code: i32, steps: Vec<Ran>) -> Ran {
        Ran {
            name: name.into(),
            command: "just verify".into(),
            exit_code: Some(code),
            signal: None,
            seconds: 9.0,
            log: PathBuf::from(format!("/logs/{name}.log")),
            steps,
            error: None,
            timed_out: false,
            interrupted: false,
            load: None,
        }
    }

    fn listed(name: &str, code: i32) -> Ran {
        Ran {
            name: format!("verify/{name}"),
            ..check(name, code, Vec::new())
        }
    }

    #[test]
    fn a_list_reads_as_the_checks_steps_each_with_its_status_seconds_log_and_command() {
        let tree = Tree::new();
        tree.write(
            FILE,
            "lint\t0\t12\t/abs/lint.log\tjust lint-rs\n\
             size-check\t1\t3\tsize-check.log\tjust size-check\n\
             tests\t101\t2.5\t/abs/tests.log\tSYMPHONY_X=\"$PWD/a b\" tools/cargo.sh test\n",
        );
        let steps = read(&tree.root, "verify").unwrap();
        let said: Vec<_> = (steps.iter())
            .map(|step| {
                (
                    step.name.as_str(),
                    step.exit_code,
                    step.seconds,
                    step.log.clone(),
                    step.command.as_str(),
                    step.green(),
                )
            })
            .collect();
        assert_eq!(
            said,
            [
                (
                    "verify/lint",
                    Some(0),
                    12.0,
                    PathBuf::from("/abs/lint.log"),
                    "just lint-rs",
                    true
                ),
                (
                    "verify/size-check",
                    Some(1),
                    3.0,
                    tree.path("size-check.log"),
                    "just size-check",
                    false
                ),
                (
                    "verify/tests",
                    Some(101),
                    2.5,
                    PathBuf::from("/abs/tests.log"),
                    "SYMPHONY_X=\"$PWD/a b\" tools/cargo.sh test",
                    false
                ),
            ]
        );
        assert_eq!(steps[1].ended(), "exit 1");
        assert!(steps.iter().all(|step| step.steps.is_empty()));
        assert!(
            steps.iter().all(|step| step.load.is_none()),
            "a list of five fields says no load"
        );
    }

    #[test]
    fn a_lists_sixth_field_is_the_load_each_step_ended_under_and_an_empty_one_none() {
        let tree = Tree::new();
        tree.write(
            FILE,
            "lint\t0\t12\tlint.log\tjust lint-rs\t3.21 2.50 2.10 10\n\
             size-check\t1\t3\tsize-check.log\tjust size-check\t\n",
        );
        let steps = read(&tree.root, "verify").unwrap();
        assert_eq!(
            steps[0].load,
            Some(Load {
                averages: [3.21, 2.5, 2.1],
                processors: 10
            })
        );
        assert_eq!(steps[1].load, None, "the system did not say");
        assert_eq!(steps[1].command, "just size-check");
        for load in [
            "3.21 2.50 2.10",
            "3.21 2.50 2.10 10 1",
            "3.21  2.50 2.10 10",
            "3,21 2.50 2.10 10",
            "-1 2.50 2.10 10",
            "inf 2.50 2.10 10",
            "3.21 2.50 2.10 ten",
            "busy",
        ] {
            assert_eq!(said_load(load), None, "{load:?}");
        }
    }

    #[test]
    fn no_list_is_no_steps_and_an_empty_one_too() {
        let tree = Tree::new();
        assert_eq!(read(&tree.path("none"), "verify"), Ok(Vec::new()));
        tree.write(FILE, "");
        assert_eq!(read(&tree.root, "verify"), Ok(Vec::new()));
    }

    #[test]
    fn a_line_that_does_not_read_or_a_step_listed_twice_leaves_no_step() {
        let tree = Tree::new();
        let good = "lint\t0\t1\tlint.log\tjust lint-rs\n";
        for (bad, why) in [
            ("tests\t1\t2\ttests.log\n", "line 2"),
            ("tests\t1\t2\ttests.log\tcmd\textra\n", "line 2"),
            ("tests\t1\t2\ttests.log\tcmd\t1 2 3 4\textra\n", "line 2"),
            ("\t1\t2\ttests.log\tcmd\n", "line 2"),
            ("tests\tred\t2\ttests.log\tcmd\n", "line 2"),
            ("tests\t1\tlong\ttests.log\tcmd\n", "line 2"),
            ("tests\t1\t-2\ttests.log\tcmd\n", "line 2"),
            ("tests\t1\tinf\ttests.log\tcmd\n", "line 2"),
            ("tests\t1\t2\t\tcmd\n", "line 2"),
            (
                "lint\t1\t2\tlint.log\tjust lint-rs\n",
                "lists `verify/lint` twice",
            ),
        ] {
            tree.write(FILE, &format!("{good}{bad}"));
            let refused = read(&tree.root, "verify").unwrap_err();
            assert!(refused.contains(why), "{bad:?}: {refused}");
        }
        let unreadable = tree.path("folder-as-list");
        fs::create_dir_all(unreadable.join(FILE)).unwrap();
        assert!(read(&unreadable, "verify").is_err());
    }

    #[test]
    fn a_step_listed_as_not_run_is_known_and_red_in_nothing_of_its_own() {
        let tree = Tree::new();
        tree.write(
            FILE,
            "lint\t0\t1\tlint.log\tjust lint-rs\n\
             nextest-setup\t2\t1\tsetup.log\tjust nextest-setup\n\
             tests\t-\t0\tsetup.log\ttools/test-lanes.sh\n",
        );
        let steps = read(&tree.root, "verify").unwrap();
        assert_eq!(steps.len(), 3);
        let tests = &steps[2];
        assert_eq!(
            (tests.name.as_str(), tests.exit_code),
            ("verify/tests", None)
        );
        assert!(not_run(tests) && !tests.green());
        assert_eq!(tests.ended(), "did not run: a step it needs was red");
        assert!(!not_run(&steps[0]) && !not_run(&steps[1]));
        // The check's red is its setup's alone: one red, and not beyond its steps.
        let verify = check("verify", 1, steps);
        assert!(!beyond(&verify));
        let reds: Vec<&str> = reds(&verify).iter().map(|red| red.name.as_str()).collect();
        assert_eq!(reds, ["verify/nextest-setup"]);
        // Red with nothing but a step not run to show for it: the check's own red.
        let mut alone = verify.clone();
        alone.steps.remove(1);
        assert!(beyond(&alone));
        // A check that never ran for another reason is not this.
        let mut never = verify.steps[2].clone();
        never.error = Some("cannot start sh".into());
        assert!(!not_run(&never));
    }

    #[test]
    fn a_step_named_under_another_is_a_part_of_it_and_no_step_or_red_of_the_check() {
        let tree = Tree::new();
        tree.write(
            FILE,
            "lint\t0\t1\tlint.log\tjust lint-rs\n\
             tests\t101\t9\ttests.log\ttools/test-lanes.sh\n\
             tests/browser\t101\t7\ttests-lanes/browser.log\ttools/test-lanes.sh browser\n\
             tests/perf\t0\t2\ttests-lanes/perf.log\ttools/test-lanes.sh perf\t1.00 2.00 3.00 8\n",
        );
        let steps = read(&tree.root, "verify").unwrap();
        let names: Vec<&str> = steps.iter().map(|step| step.name.as_str()).collect();
        assert_eq!(names, ["verify/lint", "verify/tests"]);
        assert!(steps[0].steps.is_empty());
        let parts: Vec<(&str, Option<i32>)> = (steps[1].steps.iter())
            .map(|part| (part.name.as_str(), part.exit_code))
            .collect();
        assert_eq!(
            parts,
            [
                ("verify/tests/browser", Some(101)),
                ("verify/tests/perf", Some(0))
            ]
        );
        assert_eq!(
            steps[1].steps[0].log,
            tree.root.join("tests-lanes/browser.log")
        );
        assert!(steps[1].steps[1].load.is_some());
        // The step's red is one red, whatever its parts: a part is filed nowhere.
        let verify = check("verify", 1, steps);
        let reds: Vec<&str> = reds(&verify).iter().map(|red| red.name.as_str()).collect();
        assert_eq!(reds, ["verify/tests"]);

        // A part listed before its step, twice, or under a part reads as no list.
        for (bad, why) in [
            (
                "tests/browser\t0\t1\tb.log\tcmd\n",
                "lists `verify/tests/browser` before the step it is a part of",
            ),
            (
                "tests\t0\t1\tt.log\tcmd\ntests/a\t0\t1\ta.log\tcmd\ntests/a\t0\t1\ta.log\tcmd\n",
                "lists `verify/tests/a` twice",
            ),
            (
                "tests\t0\t1\tt.log\tcmd\ntests/a\t0\t1\ta.log\tcmd\ntests/a/deep\t0\t1\ta.log\tcmd\n",
                "lists `verify/tests/a/deep`, which is no step and no part of one",
            ),
            (
                "tests\t0\t1\tt.log\tcmd\ntests/\t0\t1\ta.log\tcmd\n",
                "lists `verify/tests/`, which is no step and no part of one",
            ),
        ] {
            tree.write(FILE, bad);
            let refused = read(&tree.root, "verify").unwrap_err();
            assert!(refused.contains(why), "{bad:?}: {refused}");
        }
    }

    #[test]
    fn a_check_red_in_its_steps_alone_is_told_by_them_and_any_other_red_by_itself_too() {
        let both = check("verify", 1, vec![listed("lint", 0), listed("tests", 101)]);
        assert!(!beyond(&both));
        let names =
            |ran: &Ran| -> Vec<String> { reds(ran).iter().map(|red| red.name.clone()).collect() };
        assert_eq!(names(&both), ["verify/tests"]);

        // Red with every step it listed green, or with none listed: the check itself.
        let unexplained = check("verify", 1, vec![listed("lint", 0)]);
        assert!(beyond(&unexplained));
        assert_eq!(names(&unexplained), ["verify"]);
        assert_eq!(names(&check("verify", 2, Vec::new())), ["verify"]);

        // Stopped, killed or never run: the check, beside each red step it listed before.
        for stopped in [
            Ran {
                timed_out: true,
                ..both.clone()
            },
            Ran {
                exit_code: None,
                signal: Some(9),
                ..both.clone()
            },
            Ran {
                error: Some("cannot start sh".into()),
                ..both.clone()
            },
        ] {
            assert!(beyond(&stopped), "{stopped:?}");
            assert_eq!(names(&stopped), ["verify", "verify/tests"]);
        }

        // Green, with green steps: nothing. A check that exits 0 while a step it lists is red is
        // red, in that step.
        let green = check("verify", 0, vec![listed("lint", 0)]);
        assert!(green.green() && !beyond(&green) && reds(&green).is_empty());
        let hiding = check("verify", 0, vec![listed("lint", 3)]);
        assert!(!hiding.green(), "a red step is the check's red");
        assert_eq!(names(&hiding), ["verify/lint"]);
    }

    #[test]
    fn a_check_lists_its_steps_in_its_folder_and_one_whose_list_does_not_read_is_one_check() {
        let tree = Tree::new();
        let root = tree.root.canonicalize().unwrap();
        let logs = root.join("logs");
        fs::create_dir_all(&logs).unwrap();
        let within = Within::bound(far());
        // What the check sees of the variable, and a list it writes with a relative log.
        let step = Step::shell(
            "Verify",
            "echo \"steps in $SYMPHONY_CHECK_STEPS\"; echo said > \"$SYMPHONY_CHECK_STEPS/t.log\"; \
             printf 'tests\\t101\\t4\\tt.log\\tcargo test\\n' > \"$SYMPHONY_CHECK_STEPS/steps.tsv\"; \
             exit 1",
        );
        let mut out = Vec::new();
        let ran = run(
            &step,
            &root,
            &logs,
            &logs.join("verify.log"),
            within,
            &mut out,
        );
        assert_eq!(String::from_utf8(out).unwrap(), "");
        let folder = logs.join("verify.steps");
        assert!(
            fs::read_to_string(&ran.log)
                .unwrap()
                .contains(&format!("steps in {}\n", folder.display()))
        );
        assert_eq!(ran.steps.len(), 1, "{ran:?}");
        assert_eq!(ran.steps[0].name, "Verify/tests");
        assert_eq!(fs::read_to_string(&ran.steps[0].log).unwrap(), "said\n");
        assert_eq!(reds(&ran).len(), 1);

        // A list that does not read: said, and the check told as one.
        let garbled = Step::shell(
            "verify",
            "echo 'not a step' > \"$SYMPHONY_CHECK_STEPS/steps.tsv\"; exit 1",
        );
        let mut out = Vec::new();
        let ran = run(
            &garbled,
            &root,
            &logs,
            &logs.join("verify.log"),
            within,
            &mut out,
        );
        let said = String::from_utf8(out).unwrap();
        assert!(
            said.starts_with("release: `verify` is told as one check: its steps do not read: "),
            "{said}"
        );
        assert!(ran.steps.is_empty());
        assert_eq!(reds(&ran).len(), 1);

        // A folder that cannot be made: said, and the check runs without one.
        let blocked = root.join("blocked");
        fs::write(&blocked, "a file where the logs go").unwrap();
        let mut out = Vec::new();
        let ran = run(
            &Step::shell("verify", "echo \"${SYMPHONY_CHECK_STEPS-none}\""),
            &root,
            &blocked,
            &root.join("blocked.log"),
            within,
            &mut out,
        );
        assert!(ran.green(), "{ran:?}");
        assert!(
            String::from_utf8(out)
                .unwrap()
                .starts_with("release: `verify` is told as one check: "),
        );
        assert!(fs::read_to_string(&ran.log).unwrap().contains("\nnone\n"));
    }
}
