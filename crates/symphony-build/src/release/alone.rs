//! A red test run's failures rerun alone by the run itself (RULES.md
//! `tests/rerun-the-failure-alone`, docs/workflow/RELEASES.md "The release run", step 5). Once a check has
//! ended, each red step it listed that runs the tests' lanes ([`RUNNER`], verify's tests) and whose
//! log names at most [`MOST`] failures ([`super::rerun`]: each failed test, a target that named
//! none whole, a lane that listed none as that lane) has each of them rerun alone, one at a time,
//! in the same checkout and over the same build (`tools/test-lanes.sh --alone`, which runs it from
//! the run's own list of built binaries and builds nothing), each a check of its own with its own
//! log and the load it ended under ([`Alone`]).
//!
//! A step whose every failure passed alone is no red of the release ([`Alone::passed`]): it is
//! green as judged ([`Ran::green`]), its exit kept beside its reruns, and the run files one task for
//! the tests that did so ([`super::board`]). One that failed again stays red, its task saying each
//! rerun's result. Nothing is rerun, and the step stays red saying why ([`Alone::why_not`]), when it
//! names more than [`MOST`] failures (a break, not load), none at all, a target that found files of
//! its build gone, a lane red with no failure of its own named, or when its runner did not say it ran
//! every lane it was asked for to its end ([`ENDED`]): what failed is then more than its tests.

use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::{Deserialize, Serialize};

use super::checks::{self, Ran, Step, Within};
use super::{board, rerun, steps};

/// The failures a step's reruns take at most: as many as its red task lists. More is a break.
pub(crate) const MOST: usize = 12;
/// The tests' runner, whose `--alone` reruns one failure over the build of the run that made it.
pub(crate) const RUNNER: &str = "tools/test-lanes.sh";
/// What the runner says first, before any test runs: the folder of its lanes' logs, its build's
/// list of binaries (`binaries.json`) and the client it served (`client`).
const LOGS_IN: &str = "test-lanes: logs in ";
/// What the runner says last, and only once it ran every lane it was asked for.
pub(crate) const ENDED: &str = "test-lanes: ended: every lane asked for ran";
/// What the runner says of a lane red that listed no target of its own: `error: the lane NAME is
/// red (…)`, its line naming the lane's own arguments, which rerun it alone.
const LANE_RED: &str = "error: the lane ";

/// What came of rerunning a red step's failures alone.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Alone {
    /// Each failure rerun alone, in the order its log named them: a check named by the arguments
    /// that select it (`-p symphony --test release -- NAME --exact`), with its own log and load.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) reruns: Vec<Ran>,
    /// Why its reruns judge nothing, so it stays red: none was rerun, and why, or the checkout
    /// changed under them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) why_not: Option<String>,
}

impl Alone {
    /// Whether every failure passed alone, so the step is no red: some were rerun, and each green.
    pub(crate) fn passed(&self) -> bool {
        self.why_not.is_none() && !self.reruns.is_empty() && self.reruns.iter().all(Ran::green)
    }

    /// The reruns that passed alone, whatever the others did.
    pub(crate) fn passed_alone(&self) -> impl Iterator<Item = &Ran> {
        self.reruns.iter().filter(|rerun| rerun.green())
    }
}

/// Whether `command` runs the tests' lanes ([`RUNNER`]), whose failures can be rerun alone.
pub(crate) fn runs_lanes(command: &str) -> bool {
    command
        .split_whitespace()
        .any(|word| word == "test-lanes.sh" || word.ends_with("/test-lanes.sh"))
}

/// What the tests' runner said of itself in a step's log.
#[derive(Debug, Default, PartialEq, Eq)]
struct Said {
    /// The folder of its lanes' logs, said before any test ran.
    folder: Option<PathBuf>,
    /// Whether the last line it or a lane wrote is [`ENDED`].
    ended: bool,
    /// Each lane it said is red with no target of its own listed ([`LANE_RED`]).
    lanes: Vec<String>,
}

/// What the runner said in the log at `log`: nothing when it cannot be read. The folder is taken
/// only from its line before any target started, so a test printing one is not taken; the log's
/// own last lines that verify adds after it (`verify: …`) are passed over.
fn said(log: &Path) -> Said {
    let mut said = Said::default();
    let Ok(file) = File::open(log) else {
        return said;
    };
    let mut reader = BufReader::new(file);
    let mut started = false;
    let mut line = Vec::new();
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let text = String::from_utf8_lossy(&line);
        let text = text.trim_end_matches(['\n', '\r']);
        started = started || rerun::header(text).is_some();
        if !started
            && said.folder.is_none()
            && let Some(folder) = text.strip_prefix(LOGS_IN).filter(|f| !f.is_empty())
        {
            said.folder = Some(PathBuf::from(folder));
        }
        if let Some((lane, _)) = (text.strip_prefix(LANE_RED)).and_then(|r| r.split_once(" is red"))
            && !lane.is_empty()
            && !said.lanes.iter().any(|known| known == lane)
        {
            said.lanes.push(lane.to_string());
        }
        if !text.trim().is_empty() && !text.starts_with("verify: ") {
            said.ended = text == ENDED;
        }
    }
    said
}

/// What reruns the failures of the step `step` alone: none when it is not a red test run of the
/// lanes, which nothing reruns; else the folder of its lanes' logs and the arguments of each
/// failure ([`rerun::arguments`]), or why none is rerun.
pub(crate) fn plan(step: &Ran) -> Option<Result<(PathBuf, Vec<String>), String>> {
    if step.green() || steps::not_run(step) || !runs_lanes(&step.command) {
        return None;
    }
    Some(planned(step))
}

/// [`plan`]'s answer for a red test run of the lanes.
fn planned(step: &Ran) -> Result<(PathBuf, Vec<String>), String> {
    if step.exit_code.is_none() || step.signal.is_some() || step.error.is_some() {
        return Err(format!(
            "it ended without a status of its own ({})",
            step.ended()
        ));
    }
    let said = said(&step.log);
    if !said.ended {
        return Err(format!(
            "its runner did not say it ran every lane it was asked for to its end (`{ENDED}`), so \
             what failed may be more than its tests"
        ));
    }
    let Some(folder) = said.folder else {
        return Err(format!(
            "its runner named no folder of its lanes' logs (`{LOGS_IN}…`), which holds the build \
             a failure is rerun over"
        ));
    };
    let log = rerun::read(&step.log);
    if log.targets.iter().any(|failed| !failed.gone.is_empty()) {
        return Err(
            "a target found files of its build gone (the environment's, below), so no failure of \
             it is a test's to rerun"
                .into(),
        );
    }
    // Each red lane names a failure of its own: one red with none named (a runner that died, a
    // list it could not make) is no test's to rerun, and would pass for green beside the others.
    for part in (step.steps.iter()).filter(|part| !part.green()) {
        let lane = part.name.rsplit('/').next().unwrap_or(&part.name);
        let own = if part.log == step.log {
            !log.targets.is_empty()
        } else {
            !rerun::read(&part.log).targets.is_empty()
        };
        if !own && !said.lanes.iter().any(|said| said == lane) {
            return Err(format!(
                "its lane `{lane}` is red ({}) and names no failed test or target",
                part.ended()
            ));
        }
    }
    let each = rerun::arguments(&log.targets);
    if each.is_empty() {
        return Err("its log names no failed test or target".into());
    }
    if each.len() > MOST {
        return Err(format!(
            "it names {} failures, more than the {MOST} the run reruns: a break, not load",
            each.len()
        ));
    }
    Ok((folder, each))
}

/// Rerun alone, one at a time, the failures of each red test run of the lanes that the check `ran`
/// listed ([`plan`]), in `checkout`, each with its log among the run's `logs` (`KEY.alone/N.log`)
/// and within `within`, and keep what came of it in the step ([`Ran::alone`]), said on `out`. A
/// rerun the run's time leaves out is red, never passed over; one stopped because the run was told
/// to stop ends the reruns. Whether any rerun started.
pub(crate) fn rerun(
    ran: &mut Ran,
    checkout: &Path,
    logs: &Path,
    within: Within<'_>,
    out: &mut dyn Write,
) -> bool {
    let mut started = false;
    for step in &mut ran.steps {
        let Some(planned) = plan(step) else {
            continue;
        };
        let none = |why: String| Alone {
            reruns: Vec::new(),
            why_not: Some(why),
        };
        let (folder, each) = match planned {
            Ok(planned) => planned,
            Err(why) => {
                let _ = writeln!(out, "release:   {} reruns nothing alone: {why}", step.name);
                step.alone = Some(none(why));
                continue;
            }
        };
        let dir = logs.join(format!("{}.alone", board::key(&step.name)));
        if let Err(e) = fs::create_dir_all(&dir) {
            let why = format!(
                "the run cannot make the folder of its reruns' logs: {}",
                symphony::text::os_error(&e, &dir)
            );
            let _ = writeln!(out, "release:   {} reruns nothing alone: {why}", step.name);
            step.alone = Some(none(why));
            continue;
        }
        let _ = writeln!(
            out,
            "release:   {} reruns its {} failure{} alone, one at a time, over its own build:",
            step.name,
            each.len(),
            if each.len() == 1 { "" } else { "s" }
        );
        let over = rerun::quoted(&folder.to_string_lossy());
        let mut reruns = Vec::new();
        let mut stopped = false;
        for (n, args) in each.iter().enumerate() {
            let check = Step::shell(args, &format!("{RUNNER} --alone {over} {args}"));
            let log = dir.join(format!("{}.log", n + 1));
            let rerun = if Instant::now() >= within.bound.until {
                Ran::never(&check, &log, super::NOT_STARTED.into())
            } else {
                started = true;
                checks::run(&check, checkout, &log, within)
            };
            let _ = writeln!(
                out,
                "release:     `{args}` {} ({}, {:.0} s); log {}",
                said_of(&rerun),
                rerun.ended(),
                rerun.seconds,
                rerun.log.display()
            );
            stopped = rerun.interrupted;
            reruns.push(rerun);
            if stopped {
                break;
            }
        }
        let alone = Alone {
            reruns,
            why_not: None,
        };
        let _ = writeln!(out, "release:   {}", verdict(&step.name, &alone));
        step.alone = Some(alone);
        if stopped {
            break;
        }
    }
    started
}

/// The reruns of the check `ran`'s steps made to judge nothing, for `why`: the run stopped, or its
/// checkout changed, before what they found could count, so each step they ran for stays red.
pub(crate) fn void(ran: &mut Ran, why: &str) {
    for alone in (ran.steps.iter_mut()).filter_map(|step| step.alone.as_mut()) {
        if alone.why_not.is_none() {
            alone.why_not = Some(why.to_string());
        }
    }
}

/// Each failure of the checks `checks` and the steps they listed that passed when the run reran it
/// alone, with the step it failed in, in their order; none of reruns that judge nothing
/// ([`Alone::why_not`]).
pub(crate) fn passed_in(checks: &[Ran]) -> Vec<(&Ran, &Ran)> {
    (checks.iter())
        .flat_map(|check| std::iter::once(check).chain(&check.steps))
        .filter_map(|step| {
            let alone = step
                .alone
                .as_ref()
                .filter(|alone| alone.why_not.is_none())?;
            Some(alone.passed_alone().map(move |rerun| (step, rerun)))
        })
        .flatten()
        .collect()
}

/// What a release's notes say of the failures `checks` held that passed alone, `task` the one the
/// run files for them: a line saying so, then one for each with both results and the load at each;
/// none when there are none.
pub(crate) fn notes(checks: &[Ran], task: &str) -> Vec<String> {
    let passed = passed_in(checks);
    if passed.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![format!(
        "Failed in this run and passed rerun alone, so no red of it ({task} is theirs):"
    )];
    for (step, rerun) in passed {
        lines.push(format!(
            "- `{}`, in {}: failed ({}, {}); passed alone ({}, {:.0} s, {}).",
            rerun.name,
            step.name,
            failed(step),
            load_of(step),
            rerun.ended(),
            rerun.seconds,
            load_of(rerun)
        ));
    }
    lines
}

/// How the step `step` ended in the run, before its reruns judged it: its exit status.
pub(crate) fn failed(step: &Ran) -> String {
    step.exit_code
        .filter(|_| step.signal.is_none())
        .map_or_else(|| step.ended(), |code| format!("exit {code}"))
}

/// The load `ran` ended under, in a few words.
pub(crate) fn load_of(ran: &Ran) -> String {
    ran.load.as_ref().map_or_else(
        || "load not recorded".into(),
        |load| format!("load {}", load.said()),
    )
}

/// Whether the test a rerun ran failed alone too: it ran to an end of its own and was red, neither
/// stopped by the run nor left out of it.
pub(crate) fn failed_again(rerun: &Ran) -> bool {
    !rerun.green() && !rerun.interrupted && !rerun.timed_out && rerun.error.is_none()
}

/// What a rerun alone came to, in a few words.
pub(crate) fn said_of(rerun: &Ran) -> &'static str {
    if rerun.green() {
        "passed alone"
    } else if failed_again(rerun) {
        "failed again alone"
    } else if rerun.interrupted {
        "stopped"
    } else if rerun.timed_out {
        "stopped at the run's time"
    } else {
        "not rerun"
    }
}

/// What the run says of the step `name` once its failures were rerun alone.
fn verdict(name: &str, alone: &Alone) -> String {
    if alone.passed() {
        return format!(
            "{name}: every failure passed alone, so it is no red of the release; the run files \
             one task for them"
        );
    }
    let again = alone.reruns.iter().filter(|rerun| !rerun.green()).count();
    format!(
        "{name} stays red: {again} of its {} failures did not pass alone",
        alone.reruns.len()
    )
}

#[cfg(test)]
#[path = "alone_tests.rs"]
mod tests;
