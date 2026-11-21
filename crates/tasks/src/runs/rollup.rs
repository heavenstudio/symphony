//! The runs' daily roll-up ([`symphony_api::runs_rollup`]): the day files of a project's run lines
//! read, and one [`RunsDay`] a day written beside them, in [`folder`]. The machine's one server rolls
//! them up once an hour, on the thread of the board's roll-up (apps/symphony-server,
//! `live/reports.rs`). It reads no board and writes none.
//!
//! A day file that is there but cannot be read writes nothing, so each roll-up stays as the last one
//! left it. A line that does not parse is passed over, as every reader of the day files passes over
//! one. Each roll-up is written whole, aside and renamed, while its server generation's gate stands
//! open ([`crate::recording`]).

use super::regular;
use crate::recording::{Gate, State};
use crate::reports::{DAY, spread};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};
use symphony_api::runs_rollup::{
    Cost, FLAKY_DAYS, FlakyTest, PipelineDay, RUNS_TOP, SuiteDay, TaskCost,
};
use symphony_api::{
    End, RUNS_DAY_SCHEMA, RUNS_ROLLUP, RunLine, RunsDay, fold, report_file, run_day,
};

/// Where the roll-ups of the run lines kept in `runs` go.
pub fn folder(runs: &Path) -> PathBuf {
    runs.join(RUNS_ROLLUP)
}

/// The lines of the day files of `days` (`YYYY-MM-DD`) in `runs`, each run once ([`fold`]). A day
/// with no file has none. A file that is there but is no regular file, or cannot be read, is the
/// error.
fn read(runs: &Path, days: &[String]) -> Result<Vec<RunLine>, String> {
    let mut lines = Vec::new();
    for day in days {
        let path = runs.join(format!("{day}.jsonl"));
        let unread = |e: io::Error| match e.kind() {
            // Its message names the file already.
            io::ErrorKind::Other => e.to_string(),
            _ => format!("{}: {e}", path.display()),
        };
        let mut bytes = Vec::new();
        match regular(&path, OpenOptions::new().read(true)) {
            Ok(mut file) => {
                file.read_to_end(&mut bytes).map_err(unread)?;
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(unread(e)),
        }
        lines.extend(
            bytes
                .split(|byte| *byte == b'\n')
                .filter_map(crate::objects::parse::<RunLine>),
        );
    }
    Ok(fold(lines))
}

/// The day from `start` (a UTC midnight) of the project `project`, read at `now` from `lines`
/// ([`read`]): the runs that ended in it, and the flaky tests of the [`FLAKY_DAYS`] days up to it.
fn day(project: &str, start: f64, now: f64, lines: &[RunLine]) -> RunsDay {
    let name = run_day(start);
    let first = run_day(start - f64::from(FLAKY_DAYS - 1) * DAY);
    let until = now.min(start + DAY);
    let ended = |line: &&RunLine| line.end.is_some();
    // A run whose steps report its tests: they are counted by the steps', once.
    let covered: HashSet<&str> = lines
        .iter()
        .filter(|line| reports_tests(line))
        .filter_map(|line| line.parent.as_deref())
        .collect();
    let today: Vec<&RunLine> = lines
        .iter()
        .filter(ended)
        .filter(|line| line.day() == name)
        .collect();
    let runs: Vec<&RunLine> = today
        .iter()
        .copied()
        .filter(|line| line.parent.is_none())
        .collect();
    let suites = suites(
        today
            .iter()
            .copied()
            .filter(|line| !covered.contains(line.run.as_str())),
    );
    let mut slowest: Vec<&SuiteDay> = suites.iter().collect();
    crate::order::sort_by(&mut slowest, &|a, b| {
        b.time
            .median
            .total_cmp(&a.time.median)
            .then(a.name.cmp(&b.name))
    });
    let flaky = flaky(lines.iter().filter(ended).filter(|line| {
        let day = line.day();
        first <= day && day <= name && !covered.contains(line.run.as_str())
    }));
    RunsDay {
        schema: RUNS_DAY_SCHEMA,
        project: project.to_string(),
        day: name,
        start,
        read: now,
        until,
        closed: until >= start + DAY,
        pipelines: pipelines(&runs),
        slowest: slowest
            .iter()
            .take(RUNS_TOP)
            .map(|suite| suite.name.clone())
            .collect(),
        suites,
        flaky,
        cost: cost(runs.iter().copied()),
        tasks: tasks(&runs),
    }
}

/// Whether `line` says what its tests did.
fn reports_tests(line: &RunLine) -> bool {
    line.suites.is_some() || line.failed.is_some()
}

fn pipelines(runs: &[&RunLine]) -> Vec<PipelineDay> {
    let mut by_name: BTreeMap<&str, (PipelineDay, Vec<f64>)> = BTreeMap::new();
    for run in runs {
        let (day, durations) = by_name.entry(&run.pipeline).or_default();
        day.runs += 1;
        match run.end {
            Some(End::Green) => day.green += 1,
            Some(End::Red) => day.red += 1,
            Some(End::TimedOut) => day.timed_out += 1,
            Some(End::Interrupted) => day.interrupted += 1,
            Some(End::Incomplete) | None => day.incomplete += 1,
        }
        if let Some(took) = wall(run) {
            durations.push(took);
        }
    }
    by_name
        .into_iter()
        .map(|(name, (mut day, durations))| {
            let judged = day.green + day.red + day.timed_out;
            day.pipeline = name.to_string();
            day.pass_rate = (judged > 0).then(|| f64::from(day.green) / f64::from(judged));
            day.duration = spread(durations);
            day
        })
        .collect()
}

/// How long `line` ran, from start to end; none when it never started.
fn wall(line: &RunLine) -> Option<f64> {
    Some((line.ended? - line.started?).max(0.0))
}

fn suites<'a>(lines: impl Iterator<Item = &'a RunLine>) -> Vec<SuiteDay> {
    let mut by_name: BTreeMap<&str, (u32, u32, Vec<f64>)> = BTreeMap::new();
    for suite in lines.filter_map(|line| line.suites.as_ref()).flatten() {
        let (runs, failed, times) = by_name.entry(&suite.name).or_default();
        *runs += 1;
        // One that ended with no result line passed nothing.
        *failed += u32::from(suite.failed > 0 || suite.error);
        times.push(suite.s);
    }
    by_name
        .into_iter()
        .map(|(name, (runs, failed, times))| SuiteDay {
            name: name.to_string(),
            runs,
            failed,
            pass_rate: f64::from(runs - failed) / f64::from(runs),
            time: spread(times).unwrap_or_default(),
        })
        .collect()
}

/// A test as a failure names it: its binary, when known, and its name.
type Test<'a> = (Option<&'a str>, &'a str);

/// The tests of `window`'s lines that failed and passed on the same tree ([`FlakyTest`]).
fn flaky<'a>(window: impl Iterator<Item = &'a RunLine>) -> Vec<FlakyTest> {
    // The runs of each tree, as a rerun names it: pipeline, step, label and tree, all clean, each
    // that ran to an end of its own (green or red).
    let mut trees: HashMap<(&str, Option<&str>, &str, &str), Vec<&RunLine>> = HashMap::new();
    for line in window {
        if let (Some(tree), Some(false), Some(End::Green | End::Red)) =
            (line.tree.as_deref(), line.dirty, line.end)
        {
            trees
                .entry((&line.pipeline, line.step.as_deref(), &line.label, tree))
                .or_default()
                .push(line);
        }
    }
    let mut counts: BTreeMap<Test, (u32, u32)> = BTreeMap::new();
    for runs in trees.values() {
        let mut fails: BTreeMap<Test, u32> = BTreeMap::new();
        for run in runs {
            for test in failures(run) {
                *fails.entry(test).or_default() += 1;
            }
        }
        for (test, times) in fails {
            let (flaky, red) = counts.entry(test).or_default();
            if runs.iter().any(|run| passed(run, test)) {
                *flaky += times;
            } else {
                *red += times;
            }
        }
    }
    let mut flaky: Vec<FlakyTest> = counts
        .into_iter()
        .filter(|(_, (flaky, _))| *flaky > 0)
        .map(|((suite, name), (flaky, red))| FlakyTest {
            suite: suite.map(str::to_string),
            name: name.to_string(),
            flaky,
            red,
        })
        .collect();
    crate::order::sort_by(&mut flaky, &|a, b| {
        b.flaky.cmp(&a.flaky).then(b.red.cmp(&a.red))
    });
    flaky
}

/// The tests `run` names as failed, each once.
fn failures(run: &RunLine) -> HashSet<Test<'_>> {
    run.failed
        .iter()
        .flatten()
        .map(|test| (test.suite.as_deref(), test.name.as_str()))
        .collect()
}

/// Whether `test` passed in `run`: not among its failures, and the run green, or red with the
/// test's binary among the ones it reported, and that one ended with its result line.
fn passed(run: &RunLine, test: Test) -> bool {
    if failures(run).contains(&test) {
        return false;
    }
    match run.end {
        Some(End::Green) => true,
        Some(End::Red) => test.0.is_some_and(|suite| {
            run.failed.is_some()
                && run
                    .suites
                    .iter()
                    .flatten()
                    .any(|reported| reported.name == suite && !reported.error)
        }),
        _ => false,
    }
}

fn cost<'a>(runs: impl Iterator<Item = &'a RunLine>) -> Cost {
    let mut cost = Cost::default();
    for run in runs {
        cost.runs += 1;
        cost.cpu_s += run.cpu_s.unwrap_or(0.0);
        cost.wall_s += wall(run).unwrap_or(0.0);
        // A run that never started took none; one that did with no count may have taken some.
        let unknown = run.started.is_some() && run.cpu_s.is_none();
        cost.cpu_partial |= unknown || run.cpu_partial == Some(true);
    }
    cost
}

fn tasks(runs: &[&RunLine]) -> Vec<TaskCost> {
    let mut by_task: BTreeMap<&str, Vec<&RunLine>> = BTreeMap::new();
    for run in runs {
        if let Some(task) = run.task.as_deref() {
            by_task.entry(task).or_default().push(run);
        }
    }
    let mut tasks: Vec<TaskCost> = by_task
        .into_iter()
        .map(|(task, runs)| TaskCost {
            task: task.to_string(),
            cost: cost(runs.into_iter()),
        })
        .collect();
    crate::order::sort_by(&mut tasks, &|a, b| {
        b.cost
            .cpu_s
            .total_cmp(&a.cost.cpu_s)
            .then(b.cost.wall_s.total_cmp(&a.cost.wall_s))
    });
    tasks.truncate(RUNS_TOP);
    tasks
}

/// The roll-up of `day.day` written in `folder`: a whole file in its place, written aside and
/// renamed, so a reader finds the last one or the one before it, never a part.
fn write(folder: &Path, day: &RunsDay) -> Result<(), String> {
    let mut text = serde_json::to_vec(day).map_err(|e| e.to_string())?;
    text.push(b'\n');
    crate::board_health::write_whole(&folder.join(report_file(&day.day)), &text)
}

/// Roll the run lines kept in `runs` up into [`folder`] at `now`, for the project `project`: one
/// file for each day of `days` (each a UTC midnight, oldest first; the days due, as the board's
/// roll-up names them), from one read of their day files and of the [`FLAKY_DAYS`] days before.
/// Each is written while `gate` stands open: once a reload closes it nothing more is written.
/// The days written, or why none was.
pub fn roll_up(
    runs: &Path,
    project: &str,
    now: f64,
    days: &[f64],
    gate: &Gate,
) -> Result<Vec<String>, String> {
    let (Some(first), Some(last)) = (days.first(), days.last()) else {
        return Ok(Vec::new());
    };
    let mut names = Vec::new();
    let mut start = first - f64::from(FLAKY_DAYS - 1) * DAY;
    while start <= *last {
        names.push(run_day(start));
        start += DAY;
    }
    let lines = read(runs, &names)?;
    let out = folder(runs);
    let mut written = Vec::new();
    for start in days {
        let day = day(project, *start, now, &lines);
        let open = gate.with(|state| match state {
            State::Open { .. } => write(&out, &day).map(|()| true),
            State::Held | State::Closed(_) => Ok(false),
        })?;
        if !open {
            break;
        }
        written.push(day.day);
    }
    Ok(written)
}

#[cfg(test)]
mod tests;
