//! The board's part of a project's day files ([`symphony_api::reports`]): how tasks moved hour by
//! hour, the waits that ended, the review rounds and the reds, from the one read the hourly
//! roll-up makes of the board ([`crate::Store::report_records`]). Nothing here reads the board or
//! the clock: [`day`] is given the read and the times. Where the day files are kept ([`folder`]),
//! their write ([`write`]), whether one is final ([`closed`]) and their read ([`read`], a test's
//! and a page's), are here too.
//!
//! Each task is replayed as `tasks show` replays it ([`crate::stages::stages`]): its stretches in
//! one status are what the flow counts. Its lines are taken in the order the read gives them, the
//! order written ([`ReportTask::lines`]). A status held at an instant is the one in force just
//! before it, so an hour's `held` holds the tasks its `entered` and the hours before it brought in.
//! A line's time is its own, never before its task was made.
//!
//! On a board that runs a custom workflow the read carries each task's recorded workflow
//! transitions too ([`ReportRead::places`]), and each hour then says where its tasks spent it, by
//! phase and by state ([`placed`]): from those records alone, never from the thread's lines, and
//! apart from the statuses' task-hours. A board that runs none writes the file it always wrote.

use crate::cli::Env;
use crate::stages::{self, CLAIMED, CLOSED, Facts, Hold, Line, RELEASED, Segment, Stages};
use crate::{ReportPlace, ReportRead, ReportTask, red};
use std::path::{Path, PathBuf};
use symphony_api::reports::{
    ByStatus, DayFlow, Flow, Landed, Placed, PlacedHours, Red, ReportDay, ReportHour, Round,
    Sitting, Spent, Spread, Wait, Waits,
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
    let mut hours = Vec::new();
    // Each hour of the day up to the one `until` falls in; the first always, even when `until` is
    // the day's first second.
    for hour in 0..24u32 {
        let from = start + f64::from(hour) * HOUR;
        if hour > 0 && from >= until {
            break;
        }
        let to = (from + HOUR).min(until);
        hours.push(ReportHour {
            start: from,
            top: flow(&replayed, true, from, to, now),
            subtasks: flow(&replayed, false, from, to, now),
            workflow: read.places.as_ref().map(|places| Placed {
                top: placed(&read.tasks, places, true, from, to, now),
                subtasks: placed(&read.tasks, places, false, from, to, now),
            }),
            ..ReportHour::default()
        });
    }
    // What happened once, each to the hour it happened in.
    let rounds = happened(&replayed, &mut hours, start, until);
    for red in replayed.iter().filter(|r| r.series.is_some()) {
        if let Some(hour) = hour_at(&mut hours, until, red.task.created) {
            hour.reds_filed += 1;
        }
        for segment in &red.stages.segments {
            if CLOSED.contains(&segment.status.as_str())
                && let Some(hour) = hour_at(&mut hours, until, segment.from)
            {
                hour.reds_closed += 1;
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
        top: day_flow(&replayed, true, start, until),
        subtasks: day_flow(&replayed, false, start, until),
        rounds,
        reds: reds(&replayed, start, until),
        decisions: None,
        git: None,
        // The next roll-up's to take from: the roll-up fills the hours' ([`spend::fill`]).
        usage: None,
        landed: landed(&replayed, read, start, until),
    }
}

/// Each top-level task of `replayed` landed from `start` to `until`, once at its last landing
/// there, with the tokens `read` holds for it and its subtasks, oldest first.
fn landed(replayed: &[Replayed], read: &ReportRead, start: f64, until: f64) -> Vec<Landed> {
    let mut landed: Vec<Landed> = Vec::new();
    for task in replayed.iter().filter(|r| r.task.parent.is_none()) {
        let last = (task.task.lines.iter().rev())
            .filter(|line| line.kind == "verdict" && line.text.starts_with(LANDED))
            .map(|line| task.at(line))
            .find(|at| start <= *at && *at < until);
        if let Some(at) = last {
            let slug = &task.task.slug;
            let place = landed.partition_point(|kept| (kept.at, &kept.slug) <= (at, slug));
            landed.insert(
                place,
                Landed {
                    slug: slug.clone(),
                    at,
                    spent: (read.spent.iter())
                        .filter(|(of, _)| of == slug)
                        .map(|(_, sum)| sum.clone())
                        .collect(),
                },
            );
        }
    }
    landed
}

fn count<T>(items: impl Iterator<Item = T>) -> u32 {
    u32::try_from(items.count()).unwrap_or(u32::MAX)
}

/// The hour of `hours`, a day's from its first, that `at` falls in, when it falls before `until`;
/// none for a time before the day or from `until` on.
pub fn hour_at(hours: &mut [ReportHour], until: f64, at: f64) -> Option<&mut ReportHour> {
    if at >= until {
        return None;
    }
    hours.iter_mut().rev().find(|hour| hour.start <= at)
}

/// A task replayed for the report.
struct Replayed<'a> {
    task: &'a ReportTask,
    stages: Stages,
    /// Who held its executor and reviewer roles.
    holds: Vec<Hold>,
    /// The series of reds it is filed in, `main` or `release`, or `red` for one marked outside them;
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
    // The release run's reds are `release` in the series they were filed in before its rename too.
    let series = if let Some(series) = red::SERIES.iter().find(|s| red::filed_in(s, &task.slug)) {
        Some(if *series == red::SERIES[0] {
            "main"
        } else {
            "release"
        })
    } else {
        task.marked_red.then_some("red")
    };
    Replayed {
        task,
        stages: stages::stages(facts, &task.lines, now),
        holds: stages::holds(&task.lines, task.created),
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

/// What happened once on the board, read from every task of `replayed` once, each into the hour of
/// `hours` (the day's from `start`, up to `until`) it happened in: the landings and the waits, each
/// wait by its end. The review rounds the day's `changes` and `approve` verdicts ended are the
/// answer, by their number, kept in order as found. Each task's review subtasks are found among
/// `replayed` by a scan: a few hundred tasks an hour, where a map would cost the server's binary
/// its own (docs/workflow/CHECKS.md, size budgets).
fn happened(replayed: &[Replayed], hours: &mut [ReportHour], start: f64, until: f64) -> Vec<Round> {
    let mut rounds: Vec<Round> = Vec::new();
    for task in replayed {
        let slug = &task.task.slug;
        // The wait from `from` to `to`, into its kind's list (`kind`) of the hour `to` falls in.
        let wait = |hours: &mut [ReportHour],
                    kind: fn(&mut Waits) -> &mut Vec<Wait>,
                    from: f64,
                    to: f64| {
            if let Some(hour) = hour_at(hours, until, to) {
                kind(&mut hour.waits).push(Wait {
                    slug: slug.clone(),
                    seconds: (to - from).max(0.0),
                });
            }
        };
        let mut approved: Option<f64> = None;
        let lines = &task.task.lines;
        for (index, line) in lines.iter().enumerate() {
            if line.kind != "verdict" {
                continue;
            }
            let at = task.at(line);
            let changes = verdict_is(&line.text, "changes");
            if (changes || verdict_is(&line.text, "approve")) && start <= at && at < until {
                let number = count(task.review_segments().filter(|s| s.from <= at)).max(1);
                let place = rounds.partition_point(|r| r.number < number);
                if rounds.get(place).is_none_or(|r| r.number != number) {
                    rounds.insert(
                        place,
                        Round {
                            number,
                            ..Round::default()
                        },
                    );
                }
                let round = &mut rounds[place];
                if changes {
                    round.changes += 1;
                } else {
                    round.approved += 1;
                }
            }
            if verdict_is(&line.text, "approve") {
                approved = Some(at);
            } else if line.text.starts_with(LANDED) {
                if let Some(from) = approved.take() {
                    wait(hours, |w| &mut w.approved_to_landed, from, at);
                }
                if let Some(hour) = hour_at(hours, until, at) {
                    hour.landed += 1;
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
                    wait(hours, |w| &mut w.changes_to_review, at, review.from);
                }
                let resumed = lines[index + 1..].iter().find(|later| {
                    is_claim(later)
                        || (!later.sender.is_empty()
                            && !(later.kind == "system" && later.text.contains(RELEASED))
                            && task.held_by(&later.sender, "executor", task.at(later).max(at)))
                });
                if let Some(later) = resumed {
                    wait(
                        hours,
                        |w| &mut w.changes_to_resumed,
                        at,
                        task.at(later).max(at),
                    );
                }
            }
        }
        let reviews = || {
            replayed.iter().filter(|child| {
                child.task.instruction && child.task.parent.as_deref() == Some(slug.as_str())
            })
        };
        for review in task.review_segments() {
            let (from, to) = (review.from, review.to.unwrap_or(f64::INFINITY));
            // A reviewer that kept the role from a round before joins nothing: the board's lines
            // cannot tell when it came back (its first line is its verdict), so the stretch has a
            // wait only once a session joins as its reviewer or claims a review subtask in it.
            let joined = task
                .holds
                .iter()
                .filter(|hold| hold.role == "reviewer")
                .map(|hold| hold.from);
            let claimed = reviews().flat_map(|child| {
                child
                    .holds
                    .iter()
                    .filter(|hold| hold.role == "executor")
                    .map(|hold| hold.from)
            });
            let staffed = joined
                .chain(claimed)
                .filter(|at| from <= *at && *at < to)
                .min_by(f64::total_cmp);
            if let Some(at) = staffed {
                wait(hours, |w| &mut w.review_to_reviewer, from, at);
            }
        }
    }
    rounds
}

fn add(counts: &mut ByStatus<u32>, status: &str) {
    if let Some(count) = counts.get_mut(status) {
        *count += 1;
    }
}

/// The time `from`..`to` spends in `start`..`end`.
fn overlap(from: f64, to: f64, start: f64, end: f64) -> f64 {
    (to.min(end) - from.max(start)).max(0.0)
}

/// How the top-level tasks of `replayed` (`top`), or its subtasks, moved from `from` to `to`, their
/// open stretches counted up to `now`. The tasks done are those whose last close falls in it, done
/// at the read ([`Stages::done_at`](crate::stages::Stages::done_at)), as `tasks stats` counts them.
fn flow(replayed: &[Replayed], top: bool, from: f64, to: f64, now: f64) -> Flow {
    let mut flow = Flow::default();
    let mut done = 0;
    for task in replayed.iter().filter(|r| r.task.parent.is_none() == top) {
        if task
            .stages
            .done_at()
            .is_some_and(|at| from <= at && at < to)
        {
            done += 1;
        }
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
                if spent > 0.0
                    && let Some(hours) = flow.task_hours.get_mut(&segment.status)
                {
                    *hours += spent / HOUR;
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
    flow.done = Some(done);
    flow
}

/// The task-hours the top-level tasks of `tasks` (`top`), or its subtasks, spent from `from` to
/// `to` at each place of `places`, a list for each task in their order, by phase and by state: a
/// place held from its move to the task's next one, the last up to `now`, and none counted while
/// it projects to a closed status, as [`flow`] counts no closed stretch. Each task's time counts
/// once in its phase and once in its state, under the label its move recorded.
fn placed(
    tasks: &[ReportTask],
    places: &[Vec<ReportPlace>],
    top: bool,
    from: f64,
    to: f64,
    now: f64,
) -> PlacedHours {
    let mut hours = PlacedHours::default();
    for (task, places) in tasks.iter().zip(places) {
        if task.parent.is_none() != top {
            continue;
        }
        for (index, place) in places.iter().enumerate() {
            if CLOSED.contains(&place.status.as_str()) {
                continue;
            }
            let until = places.get(index + 1).map_or(now, |next| next.from);
            let spent = overlap(place.from, until, from, to);
            if spent > 0.0 {
                spend(&mut hours.phases, &place.phase, &place.phase_label, spent);
                spend(&mut hours.states, &place.state, &place.state_label, spent);
            }
        }
    }
    hours
}

/// `seconds` more under `id` and `label` in `spent`, kept in order of ID and then label: a list
/// searched rather than a map, which would cost the server's binary its own tree code
/// (docs/workflow/CHECKS.md, size budgets).
fn spend(spent: &mut Vec<Spent>, id: &str, label: &str, seconds: f64) {
    let at = spent.partition_point(|kept| (kept.id.as_str(), kept.label.as_str()) < (id, label));
    match spent.get_mut(at) {
        Some(kept) if kept.id == id && kept.label == label => kept.hours += seconds / HOUR,
        _ => spent.insert(
            at,
            Spent {
                id: id.into(),
                label: label.into(),
                hours: seconds / HOUR,
            },
        ),
    }
}

/// The median and p90 of `values`, as a day file keeps a spread; none of none.
pub(crate) fn spread(values: Vec<f64>) -> Option<Spread> {
    stages::stats::spread(values).map(|s| Spread {
        n: u32::try_from(s.n).unwrap_or(u32::MAX),
        median: s.median,
        p90: s.p90,
    })
}

/// What the top-level tasks of `replayed` (`top`), or its subtasks, did over the day from `start`
/// to `until` that does not add up across hours: the lead time of the tasks its hours count done,
/// each once at its last close. The longest in their status are kept in order as they are found,
/// ten at most, rather than all sorted: a sort costs the server's binary a sort of its own for each
/// kind of list (docs/workflow/CHECKS.md, size budgets).
fn day_flow(replayed: &[Replayed], top: bool, start: f64, until: f64) -> DayFlow {
    let mut leads = Vec::new();
    let mut longest: Vec<Sitting> = Vec::new();
    for task in replayed.iter().filter(|r| r.task.parent.is_none() == top) {
        if let Some(at) = task.stages.done_at()
            && start <= at
            && at < until
        {
            leads.push((at - task.task.created).max(0.0));
        }
        if let Some(held) = task.holding(until)
            && !CLOSED.contains(&held.status.as_str())
        {
            let key = (held.from, &task.task.slug);
            let at = longest.partition_point(|kept| (kept.since, &kept.slug) <= key);
            if at < LONGEST {
                longest.insert(
                    at,
                    Sitting {
                        slug: task.task.slug.clone(),
                        status: held.status.clone(),
                        since: held.from,
                    },
                );
                longest.truncate(LONGEST);
            }
        }
    }
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
                between: task.task.between.clone(),
                // The landings in that range are git's to say: the roll-up's.
                landed: None,
            })
        })
        .collect()
}

/// Where the board `env` reaches keeps its day files: the folder [`FOLDER_VAR`] names, else
/// [`FOLDER`] in the git common dir of its working directory, `symphony/reports.<database>/` for a
/// database other than the live one. A board `TASKS_SERVER` names keeps none unless
/// [`FOLDER_VAR`] names it.
pub fn folder(env: &Env) -> Result<PathBuf, String> {
    day_files(env, FOLDER, FOLDER_VAR, "reports")
}

/// Where the board `env` reaches keeps a family of day files: the folder `var` names, else `folder`
/// in the git common dir of its working directory, `folder.<database>` for a database other than
/// the live one. A board `TASKS_SERVER` names keeps none unless `var` names it, and the refusal
/// names the files as `what`.
pub fn day_files(env: &Env, folder: &str, var: &str, what: &str) -> Result<PathBuf, String> {
    if let Some(path) = env.var(var) {
        return Ok(PathBuf::from(path));
    }
    if env.var("TASKS_SERVER").is_some() {
        return Err(format!(
            "a board TASKS_SERVER names keeps no {what} unless {var} names their folder"
        ));
    }
    let common = crate::git::common_dir(&env.cwd).map_err(|e| e.to_string())?;
    let name = match env.var("TASKS_DATABASE") {
        Some(database) if database != crate::DATABASE => format!("{folder}.{database}"),
        _ => folder.to_string(),
    };
    Ok(common.join(name))
}

/// The day file of `day.day` written in `folder`, compact JSON on one line: a whole file in its
/// place, written aside and renamed ([`crate::board_health::write_whole`]), so a reader finds the
/// last one or the one before it, never a part.
pub fn write(folder: &Path, day: &ReportDay) -> Result<(), String> {
    // Compact, `jq .` shows one indented; written by hand, as serde writes it ([`json`]).
    let mut text = json::day(day);
    text.push(b'\n');
    let path = folder.join(symphony_api::report_file(&day.day));
    crate::board_health::write_whole(&path, &text)
}

/// Whether the day file of `day` (`YYYY-MM-DD`) in `folder` is final ([`ReportDay::closed`]): false
/// when there is none or it does not read. It is read as any JSON is, for its one field: a reader
/// of the day file's shape would cost the server's binary its own (docs/workflow/CHECKS.md, size budgets).
pub fn closed(folder: &Path, day: &str) -> bool {
    std::fs::read(folder.join(symphony_api::report_file(day)))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|file| file.get("closed")?.as_bool())
        .unwrap_or(false)
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

mod json;
pub mod spend;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod placed_tests;
