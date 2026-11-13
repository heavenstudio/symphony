//! The board's part of a project's day files ([`symphony_api::reports`]): how tasks moved hour by
//! hour, the waits that ended, the review rounds, the reds and the sessions that registered, from
//! the one read the hourly roll-up makes of the board ([`crate::Store::report_records`]). Nothing
//! here reads the board or the clock: [`day`] is given the read and the times. Where the day files
//! are kept ([`folder`]), and their write ([`write`]) and read ([`read`]), are here too.
//!
//! Each task is replayed as `tasks show` replays it ([`crate::stages::stages`]): its stretches in
//! one status are what the flow counts. A status held at an instant is the one in force just before
//! it, so an hour's `held` holds the tasks its `entered` and the hours before it brought in. A line's
//! time is its own, never before its task was made.

use crate::cli::Env;
use crate::stages::{self, CLAIMED, CLOSED, Facts, Hold, Line, RELEASED, Segment, Stages};
use crate::{ReportRead, ReportTask, red};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use symphony_api::reports::{
    DayFlow, Flow, Red, ReportDay, ReportHour, Round, Sitting, Spread, Started, Wait, Waits,
};

pub const HOUR: f64 = 3_600.0;
pub const DAY: f64 = 86_400.0;
/// How many of the open tasks longest in their status a day names.
pub const LONGEST: usize = 10;
/// The folder of day files in the git common dir: in the one folder of Symphony's there, beside
/// the runs' (DECISIONS: symphony-own-names).
pub const FOLDER: &str = "symphony/reports";
/// Names the folder of day files instead, for a test or a board `TASKS_SERVER` names.
pub const FOLDER_VAR: &str = "SYMPHONY_REPORTS";
/// What starts the verdict a landing writes on the thread it closes (`land.rs`, `close`).
pub const LANDED: &str = "landed ";

/// The first second of the UTC day `at` falls in.
pub fn day_start(at: f64) -> f64 {
    (at / DAY).floor() * DAY
}

/// A new day file of `project` for the UTC day from `start`, its board figures from `read` up to
/// `until` (the read's time `now`, or the day's end once it ended). Its decisions and git are left
/// none for their own readers to fill.
pub fn day(project: &str, start: f64, until: f64, now: f64, read: &ReportRead) -> ReportDay {
    let until = until.clamp(start, start + DAY);
    let replayed: Vec<Replayed> = read.tasks.iter().map(|task| replay(task, now)).collect();
    let mut children: HashMap<&str, Vec<&Replayed>> = HashMap::new();
    for task in &replayed {
        if let Some(parent) = &task.task.parent
            && task.task.instruction
        {
            children.entry(parent.as_str()).or_default().push(task);
        }
    }
    let events = Events::of(&replayed, &children);
    let hours = hour_starts(start, until)
        .map(|(hour, from)| {
            let to = (from + HOUR).min(until);
            let within = |at: f64| from <= at && at < to;
            let moved = |top: bool| {
                flow(
                    replayed.iter().filter(|r| r.task.parent.is_none() == top),
                    from,
                    to,
                    now,
                )
            };
            let reds = || replayed.iter().filter(|r| r.series.is_some());
            ReportHour {
                hour,
                start: from,
                top: moved(true),
                subtasks: moved(false),
                landed: count(events.landed.iter().filter(|at| within(**at))),
                reds_filed: count(reds().filter(|r| within(r.task.created))),
                reds_closed: count(
                    reds()
                        .flat_map(|r| &r.stages.segments)
                        .filter(|s| CLOSED.contains(&s.status.as_str()) && within(s.from)),
                ),
                waits: events.waits_in(&within),
                decisions: None,
                git: None,
            }
        })
        .collect();
    let mut rounds: BTreeMap<u32, Round> = BTreeMap::new();
    for (at, changes, round) in &events.verdicts {
        if start <= *at && *at < until {
            let entry = rounds.entry(*round).or_insert(Round {
                number: *round,
                ..Round::default()
            });
            if *changes {
                entry.changes += 1;
            } else {
                entry.approved += 1;
            }
        }
    }
    ReportDay {
        schema: symphony_api::REPORT_SCHEMA,
        project: project.into(),
        day: symphony_api::run_day(start),
        start,
        read: now,
        until,
        closed: until >= start + DAY,
        hours,
        top: day_flow(
            replayed.iter().filter(|r| r.task.parent.is_none()),
            start,
            until,
        ),
        subtasks: day_flow(
            replayed.iter().filter(|r| r.task.parent.is_some()),
            start,
            until,
        ),
        rounds: rounds.into_values().collect(),
        reds: reds(&replayed, start, until),
        sessions: sessions(read, start, until),
        decisions: None,
        git: None,
    }
}

/// Each hour of the day from `start` up to the one `until` falls in, by its number and first second;
/// the first hour always, even when `until` is the day's first second.
fn hour_starts(start: f64, until: f64) -> impl Iterator<Item = (u32, f64)> {
    (0..24u32)
        .map(move |hour| (hour, start + f64::from(hour) * HOUR))
        .take_while(move |(hour, from)| *hour == 0 || *from < until)
}

fn count<T>(items: impl Iterator<Item = T>) -> u32 {
    u32::try_from(items.count()).unwrap_or(u32::MAX)
}

/// A task replayed for the report.
struct Replayed<'a> {
    task: &'a ReportTask,
    stages: Stages,
    /// Who held its executor and reviewer roles.
    holds: Vec<Hold>,
    /// Its lines in the order written.
    lines: Vec<&'a Line>,
    /// The series of reds it is filed in, `main`, `nightly`, or `red` for one marked outside them;
    /// none for a task that is no red.
    series: Option<&'static str>,
}

impl Replayed<'_> {
    /// When `line` counts as written: its time, never before the task was made.
    fn at(&self, line: &Line) -> f64 {
        line.ts.max(self.task.created)
    }

    /// The stretch the task was in just before `at`: the one it took before `at` and had not left
    /// by then.
    fn holding(&self, at: f64) -> Option<&Segment> {
        self.stages
            .segments
            .iter()
            .rev()
            .find(|s| s.from < at && s.to.is_none_or(|to| to >= at))
    }

    fn review_segments(&self) -> impl Iterator<Item = &Segment> {
        self.stages.segments.iter().filter(|s| s.status == "review")
    }

    /// Whether `handle` held the task's `role` at `at`.
    fn held_by(&self, handle: &str, role: &str, at: f64) -> bool {
        self.holds.iter().any(|hold| {
            hold.handle == handle
                && hold.role == role
                && hold.from <= at
                && hold.to.is_none_or(|to| to > at)
        })
    }
}

fn replay(task: &ReportTask, now: f64) -> Replayed<'_> {
    let facts = Facts {
        status: &task.status,
        created: task.created,
        updated: task.updated,
    };
    let mut lines: Vec<&Line> = task.lines.iter().collect();
    lines.sort_by_key(|line| line.id);
    lines.dedup_by_key(|line| line.id);
    let series = if let Some(series) = red::SERIES.iter().find(|s| red::filed_in(s, &task.slug)) {
        Some(if *series == red::SERIES[0] {
            "main"
        } else {
            "nightly"
        })
    } else {
        task.marked_red.then_some("red")
    };
    Replayed {
        task,
        stages: stages::stages(facts, &task.lines, now),
        holds: stages::holds(&task.lines, task.created),
        lines,
        series,
    }
}

/// Whether a verdict's text is this verdict, alone or before its `: text`.
fn verdict_is(text: &str, verdict: &str) -> bool {
    text.strip_prefix(verdict)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(':'))
}

fn is_claim(line: &Line) -> bool {
    line.kind == "system" && line.text.contains(CLAIMED) && !line.text.contains(RELEASED)
}

/// What happened once on the board, each at its time, read from every task once.
#[derive(Default)]
struct Events {
    /// When each landing was written.
    landed: Vec<f64>,
    /// Each `changes` (true) or `approve` (false) verdict, by its time and the review round it ended.
    verdicts: Vec<(f64, bool, u32)>,
    /// Each wait, by its end.
    approved_to_landed: Vec<(f64, Wait)>,
    review_to_reviewer: Vec<(f64, Wait)>,
    changes_to_resumed: Vec<(f64, Wait)>,
    changes_to_review: Vec<(f64, Wait)>,
}

impl Events {
    fn of(replayed: &[Replayed], children: &HashMap<&str, Vec<&Replayed>>) -> Events {
        let mut events = Events::default();
        for task in replayed {
            let slug = &task.task.slug;
            let wait = |from: f64, to: f64| Wait {
                slug: slug.clone(),
                seconds: (to - from).max(0.0),
            };
            let mut approved: Option<f64> = None;
            for (index, line) in task.lines.iter().enumerate() {
                if line.kind != "verdict" {
                    continue;
                }
                let at = task.at(line);
                let changes = verdict_is(&line.text, "changes");
                if changes || verdict_is(&line.text, "approve") {
                    let round = count(task.review_segments().filter(|s| s.from <= at)).max(1);
                    events.verdicts.push((at, changes, round));
                }
                if verdict_is(&line.text, "approve") {
                    approved = Some(at);
                } else if line.text.starts_with(LANDED) {
                    events.landed.push(at);
                    if let Some(from) = approved.take() {
                        events.approved_to_landed.push((at, wait(from, at)));
                    }
                } else if changes {
                    // The next time in review after the one the verdict ended, by the stretches'
                    // order, so a move and a verdict in one second are read as written.
                    let segments = &task.stages.segments;
                    let ended = segments
                        .iter()
                        .rposition(|s| s.status == "review" && s.from <= at);
                    let next = segments[ended.map_or(0, |i| i + 1)..]
                        .iter()
                        .find(|s| s.status == "review" && s.from >= at);
                    if let Some(review) = next {
                        events
                            .changes_to_review
                            .push((review.from, wait(at, review.from)));
                    }
                    let resumed = task.lines[index + 1..].iter().find(|later| {
                        is_claim(later)
                            || (!later.sender.is_empty()
                                && !(later.kind == "system" && later.text.contains(RELEASED))
                                && task.held_by(&later.sender, "executor", task.at(later).max(at)))
                    });
                    if let Some(later) = resumed {
                        let to = task.at(later).max(at);
                        events.changes_to_resumed.push((to, wait(at, to)));
                    }
                }
            }
            let reviews = children.get(slug.as_str());
            for review in task.review_segments() {
                let (from, to) = (review.from, review.to.unwrap_or(f64::INFINITY));
                let held = task.holds.iter().any(|hold| {
                    hold.role == "reviewer" && hold.from <= from && hold.to.is_none_or(|t| t > from)
                });
                let staffed = if held {
                    Some(from)
                } else {
                    let joined = task
                        .holds
                        .iter()
                        .filter(|hold| hold.role == "reviewer")
                        .map(|hold| hold.from);
                    let claimed = reviews.into_iter().flatten().flat_map(|child| {
                        child
                            .holds
                            .iter()
                            .filter(|hold| hold.role == "executor")
                            .map(|hold| hold.from)
                    });
                    joined
                        .chain(claimed)
                        .filter(|at| from <= *at && *at < to)
                        .min_by(f64::total_cmp)
                };
                if let Some(at) = staffed {
                    events.review_to_reviewer.push((at, wait(from, at)));
                }
            }
        }
        events
    }

    /// The waits that ended at a time `within` says.
    fn waits_in(&self, within: &dyn Fn(f64) -> bool) -> Waits {
        let pick = |all: &[(f64, Wait)]| -> Vec<Wait> {
            all.iter()
                .filter(|(at, _)| within(*at))
                .map(|(_, wait)| wait.clone())
                .collect()
        };
        Waits {
            approved_to_landed: pick(&self.approved_to_landed),
            review_to_reviewer: pick(&self.review_to_reviewer),
            changes_to_resumed: pick(&self.changes_to_resumed),
            changes_to_review: pick(&self.changes_to_review),
        }
    }
}

fn add(map: &mut BTreeMap<String, u32>, status: &str) {
    *map.entry(status.into()).or_default() += 1;
}

/// The time `from`..`to` spends in `start`..`end`.
fn overlap(from: f64, to: f64, start: f64, end: f64) -> f64 {
    (to.min(end) - from.max(start)).max(0.0)
}

/// How `tasks` moved from `from` to `to`, their open stretches counted up to `now`.
fn flow<'a>(tasks: impl Iterator<Item = &'a Replayed<'a>>, from: f64, to: f64, now: f64) -> Flow {
    let mut flow = Flow::default();
    for task in tasks {
        let segments = &task.stages.segments;
        if let Some(held) = task.holding(to)
            && !CLOSED.contains(&held.status.as_str())
        {
            add(&mut flow.held, &held.status);
        }
        for segment in segments {
            if from <= segment.from && segment.from < to {
                add(&mut flow.entered, &segment.status);
            }
            if !CLOSED.contains(&segment.status.as_str()) {
                let spent = overlap(segment.from, segment.to.unwrap_or(now), from, to);
                if spent > 0.0 {
                    *flow.task_hours.entry(segment.status.clone()).or_default() += spent / HOUR;
                }
            }
        }
        let waited_until = (task.stages.first_claimed)
            .or(task.stages.closed)
            .unwrap_or(now);
        flow.unclaimed_hours += overlap(task.task.created, waited_until, from, to) / HOUR;
        flow.bounces += count(segments.windows(2).filter(|pair| {
            pair[0].status == "review"
                && pair[1].status == "in_progress"
                && from <= pair[1].from
                && pair[1].from < to
        }));
    }
    flow
}

fn spread(values: Vec<f64>) -> Option<Spread> {
    stages::stats::spread(values).map(|s| Spread {
        n: u32::try_from(s.n).unwrap_or(u32::MAX),
        median: s.median,
        p90: s.p90,
    })
}

/// What `tasks` did over the day from `start` to `until` that does not add up across hours.
fn day_flow<'a>(tasks: impl Iterator<Item = &'a Replayed<'a>>, start: f64, until: f64) -> DayFlow {
    let mut leads = Vec::new();
    let mut longest = Vec::new();
    for task in tasks {
        for segment in &task.stages.segments {
            if segment.status == "done" && start <= segment.from && segment.from < until {
                leads.push((segment.from - task.task.created).max(0.0));
            }
        }
        if let Some(held) = task.holding(until)
            && !CLOSED.contains(&held.status.as_str())
        {
            longest.push(Sitting {
                slug: task.task.slug.clone(),
                status: held.status.clone(),
                since: held.from,
            });
        }
    }
    longest.sort_by(|a, b| {
        a.since
            .total_cmp(&b.since)
            .then_with(|| a.slug.cmp(&b.slug))
    });
    longest.truncate(LONGEST);
    DayFlow {
        lead: spread(leads),
        longest,
    }
}

/// Each red open at some time from `start` to `until`: filed before `until` and not closed before
/// `start`, by when it was filed.
fn reds(replayed: &[Replayed], start: f64, until: f64) -> Vec<Red> {
    replayed
        .iter()
        .filter_map(|task| {
            let series = task.series?;
            let closed = task.stages.closed.filter(|closed| *closed < until);
            let open = task.task.created < until && closed.is_none_or(|closed| closed >= start);
            open.then(|| Red {
                slug: task.task.slug.clone(),
                series: series.into(),
                filed: task.task.created,
                closed,
                status: task
                    .holding(until)
                    .map_or_else(|| task.task.status.clone(), |s| s.status.clone()),
            })
        })
        .collect()
}

/// The sessions that registered from `start` to `until`, by kind and model, the most first.
fn sessions(read: &ReportRead, start: f64, until: f64) -> Vec<Started> {
    let mut counted: BTreeMap<(&str, &str), u32> = BTreeMap::new();
    for session in &read.sessions {
        if start <= session.connected && session.connected < until {
            *counted.entry((&session.kind, &session.model)).or_default() += 1;
        }
    }
    let mut started: Vec<Started> = counted
        .into_iter()
        .map(|((kind, model), count)| Started {
            kind: kind.into(),
            model: model.into(),
            count,
        })
        .collect();
    // Stable: equal counts stay in kind and model order.
    started.sort_by_key(|s| std::cmp::Reverse(s.count));
    started
}

/// Where the board `env` reaches keeps its day files: the folder [`FOLDER_VAR`] names, else
/// [`FOLDER`] in the git common dir of its working directory, `symphony/reports.<database>/` for a
/// database other than the live one. A board `TASKS_SERVER` names keeps none unless
/// [`FOLDER_VAR`] names it.
pub fn folder(env: &Env) -> Result<PathBuf, String> {
    if let Some(path) = env.var(FOLDER_VAR) {
        return Ok(PathBuf::from(path));
    }
    if env.var("TASKS_SERVER").is_some() {
        return Err(format!(
            "a board TASKS_SERVER names keeps no reports unless {FOLDER_VAR} names their folder"
        ));
    }
    let common = crate::git::common_dir(&env.cwd).map_err(|e| e.to_string())?;
    let name = match env.var("TASKS_DATABASE") {
        Some(database) if database != crate::DATABASE => format!("{FOLDER}.{database}"),
        _ => FOLDER.to_string(),
    };
    Ok(common.join(name))
}

/// The day file of `day.day` written in `folder`: a whole file in its place, written aside and
/// renamed, so a reader finds the last one or the one before it, never a part.
pub fn write(folder: &Path, day: &ReportDay) -> Result<(), String> {
    static WRITES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    let name = symphony_api::report_file(&day.day);
    let path = folder.join(&name);
    let partial = folder.join(format!(
        ".{name}.{}.{}",
        std::process::id(),
        WRITES.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let text = serde_json::to_string_pretty(day).map_err(|e| e.to_string())?;
    std::fs::write(&partial, text + "\n")
        .and_then(|()| std::fs::rename(&partial, &path))
        .map_err(|e| {
            let _ = std::fs::remove_file(&partial);
            format!("{}: {e}", path.display())
        })
}

/// The day file of `day` (`YYYY-MM-DD`) in `folder`; none when there is none.
pub fn read(folder: &Path, day: &str) -> Result<Option<ReportDay>, String> {
    let path = folder.join(symphony_api::report_file(day));
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text)
            .map(Some)
            .map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

#[cfg(test)]
mod tests;
