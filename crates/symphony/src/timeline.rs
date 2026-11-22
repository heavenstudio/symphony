//! What a task went through (`symphony_api::timeline`), for its page's `/api/task-timeline`: read
//! from the task and its bounded recorded ancestry, never the whole board.
//!
//! - Its stages are the board's replay of its thread (`symphony_board::stages`), each stretch in
//!   review a round: the round waits for a reviewer until one takes the role (or holds it already),
//!   and ends at its verdict, the board's own ([`crate::gates::review_rounds`]) or, where none
//!   closed it, the review gate's turning met. A stretch in review after that approval is the task
//!   waiting to land.
//! - Its claims are the executors' holds (`symphony_board::stages::holds`) while it was in progress.
//! - Its runs are the builds and tests the board's recorder kept for it (docs/workflow/MACHINE_WORK.md),
//!   a step inside one of them left to that one; none until one is kept.
//! - Its gates are the gates answer's states, past and current ([`crate::gates::answer`]), a lane
//!   each, by name, but none for a stamp left out (`gates::stamps::left_out`), which the gates
//!   list. The review gate's lane begins where the task first went to review.
//! - Its role and address metadata is a separate bounded projection (`history`), with explicit
//!   source completeness and thread cursors. It reads no current session or role holders.
//! - Its places are the custom workflow's, one for each recorded transition of the task
//!   ([`places`]), absent for a task no workflow held: time at a place, never the runs' time.
//!
//! [`build`] puts them together from what was read; [`answer`] reads the thread and the recorder's
//! history once, for those lanes and the gates beside them, and reads capped thread windows for
//! the role and address projection.

use std::path::Path;

use symphony_api::gates::REVIEW;
use symphony_api::timeline::{
    Claim, GateLane, GateSpan, Offset, Round, Run, RunResult, Stage, TIMELINE_EVERY, Verdict,
};
use symphony_api::{Gate, GateState, Pending, TaskTimeline};
use symphony_board::cli::machine_work::recorded::{self, Phase, Record, Step};
use symphony_board::stages::{self, Facts, Hold, Line, Segment};

mod history;
mod places;

/// The timeline of the task `slug`, which `facts` describe, from its `thread`, the `runs` kept for
/// it, its `gates` and the time `now`. Its clock ([`TaskTimeline::zone`]) and how often it is read
/// again are the caller's to set.
pub fn build(
    slug: &str,
    facts: Facts,
    thread: &[symphony_board::Message],
    runs: &[Record],
    gates: &[Gate],
    now: f64,
) -> TaskTimeline {
    let lines: Vec<Line> = thread.iter().map(Line::from).collect();
    let replay = stages::stages(facts, &lines, now);
    let holds = stages::holds(&lines, facts.created);
    let verdicts = crate::gates::review_rounds(thread);
    let approvals = approvals(gates);
    let mut stages = Vec::new();
    let mut rounds: Vec<Round> = Vec::new();
    for segment in &replay.segments {
        if segment.status != "review" {
            stages.push(stage(segment, 0));
            continue;
        }
        let number = u32::try_from(rounds.len() + 1).unwrap_or(u32::MAX);
        let (round, approved_at) = round(number, segment, &holds, &verdicts, &approvals, now);
        match approved_at {
            Some((at, by)) => {
                stages.push(Stage {
                    to: Some(at),
                    ..stage(segment, number)
                });
                stages.push(Stage {
                    from: at,
                    by,
                    approved: true,
                    ..stage(segment, number)
                });
            }
            None => stages.push(stage(segment, number)),
        }
        rounds.push(round);
    }
    let reviewed = replay
        .segments
        .iter()
        .find(|segment| segment.status == "review")
        .map(|segment| segment.from);
    TaskTimeline {
        slug: slug.to_string(),
        now,
        created: facts.created,
        closed: replay.closed,
        first_claimed: replay.first_claimed,
        bounces: replay.bounces,
        stages,
        claims: claims(&holds, &replay.segments),
        rounds,
        runs: own_runs(runs),
        gates: lanes(gates, facts.created, reviewed),
        ..TaskTimeline::default()
    }
}

/// Each of `gates`' lanes on a task made at `created`, by name: a lane keeps its row while its gate
/// turns met or missing, as the answer's order, what the task waits on first, would not. The
/// review gate's lane begins at the task's first move to review (`reviewed`), missing until its
/// first verdict, and holds nothing before it.
fn lanes(gates: &[Gate], created: f64, reviewed: Option<f64>) -> Vec<GateLane> {
    let mut lanes: Vec<GateLane> = gates.iter().map(|gate| lane(gate, created)).collect();
    lanes.sort_unstable_by(|a, b| a.name.cmp(&b.name));
    if let Some(review) = lanes.iter_mut().find(|lane| lane.name == REVIEW) {
        review.states = from_review(std::mem::take(&mut review.states), reviewed);
    }
    // A task never in review has no review lane yet.
    lanes.retain(|lane| !lane.states.is_empty());
    lanes
}

/// The review gate's `states` from `reviewed` on: a state taken before it starts there, the last
/// of several that would start together kept, and the wait for a first verdict missing.
fn from_review(states: Vec<GateSpan>, reviewed: Option<f64>) -> Vec<GateSpan> {
    let Some(reviewed) = reviewed else {
        return Vec::new();
    };
    let mut kept: Vec<GateSpan> = Vec::new();
    for mut span in states {
        span.from = span.from.max(reviewed);
        if kept.last().is_some_and(|last| last.from >= span.from) {
            kept.pop();
        }
        kept.push(span);
    }
    if kept.first().is_none_or(|first| first.from > reviewed) {
        kept.insert(
            0,
            GateSpan {
                state: GateState::Missing,
                from: reviewed,
                by: None,
                detail: "Not reviewed yet".into(),
            },
        );
    }
    kept
}

fn stage(segment: &Segment, round: u32) -> Stage {
    Stage {
        status: segment.status.clone(),
        from: segment.from,
        to: segment.to,
        by: segment.by.clone(),
        round,
        approved: false,
    }
}

/// When the review gate turned met, and by whom: each of its states that is met, oldest first.
fn approvals(gates: &[Gate]) -> Vec<(f64, Option<String>)> {
    let Some(review) = gates.iter().find(|gate| gate.name == REVIEW) else {
        return Vec::new();
    };
    let past = review
        .past
        .iter()
        .filter(|past| past.state == GateState::Met)
        .map(|past| (past.at, past.by.clone()));
    let current = review
        .at
        .filter(|_| review.state == GateState::Met)
        .map(|at| (at, review.by.clone()));
    past.chain(current).collect()
}

/// The round `number`, which the stretch in review `segment` is: when a reviewer took it up, who,
/// and its verdict. Where no verdict of the board's ended it but the review gate turned met while it
/// went on, that is its approval, returned apart when the stretch goes on after it.
fn round(
    number: u32,
    segment: &Segment,
    holds: &[Hold],
    verdicts: &[crate::gates::Round],
    approvals: &[(f64, Option<String>)],
    now: f64,
) -> (Round, Option<(f64, Option<String>)>) {
    let end = segment.to.unwrap_or(now);
    let within = |at: f64| at >= segment.from && at <= end;
    // The reviewer who took it up first: one holding the role as it came, else the first to join.
    let reviewer = holds
        .iter()
        .filter(|hold| hold.role == "reviewer")
        .filter_map(|hold| {
            let held_on = hold.to.is_none_or(|to| to > segment.from);
            if hold.from <= segment.from && held_on {
                Some((segment.from, hold))
            } else {
                within(hold.from).then_some((hold.from, hold))
            }
        })
        .min_by(|a, b| a.0.total_cmp(&b.0));
    let taken = reviewer.map(|(at, _)| at);
    let reviewer = reviewer.map(|(_, hold)| hold.handle.clone());
    let verdict = verdicts.iter().rfind(|verdict| within(verdict.at));
    let approval = if verdict.is_some() {
        None
    } else {
        approvals.iter().find(|(at, _)| within(*at)).cloned()
    };
    let (to, by, verdict) = match (verdict, &approval) {
        (Some(verdict), _) => (
            Some(verdict.at),
            Some(verdict.by.clone()),
            Some(if verdict.approved {
                Verdict::Approved
            } else {
                Verdict::Changes
            }),
        ),
        (None, Some((at, by))) => (Some(*at), by.clone().or(reviewer), Some(Verdict::Approved)),
        (None, None) => (segment.to, reviewer, None),
    };
    let round = Round {
        number,
        from: segment.from,
        taken: taken.map(|at| to.map_or(at, |to| at.min(to))),
        to,
        by,
        verdict,
    };
    // An approval at the stretch's very end is its close: nothing waits after it.
    let waits = approval.filter(|(at, _)| segment.to.is_none_or(|to| *at < to));
    (round, waits)
}

/// The executors' holds while the task was in progress, oldest first.
fn claims(holds: &[Hold], segments: &[Segment]) -> Vec<Claim> {
    let mut found = Vec::new();
    for hold in holds.iter().filter(|hold| hold.role == "executor") {
        for segment in segments
            .iter()
            .filter(|segment| segment.status == "in_progress")
        {
            let from = hold.from.max(segment.from);
            let to = match (hold.to, segment.to) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (Some(a), None) | (None, Some(a)) => Some(a),
                (None, None) => None,
            };
            if to.is_none_or(|to| to > from) {
                found.push(Claim {
                    by: hold.handle.clone(),
                    from,
                    to,
                });
            }
        }
    }
    found.sort_by(|a, b| a.from.total_cmp(&b.from));
    found
}

/// The runs of `records`, all kept for the task, by when they were queued: a step of another of
/// them (its parent among them) is left to that one, so no time counts twice.
fn own_runs(records: &[Record]) -> Vec<Run> {
    let mut runs: Vec<&Record> = records
        .iter()
        .filter(|record| {
            record
                .parent_id
                .as_deref()
                .is_none_or(|parent| !records.iter().any(|other| other.operation_id == parent))
        })
        .collect();
    runs.sort_by(|a, b| a.queued_at.total_cmp(&b.queued_at));
    runs.into_iter()
        .map(|record| Run {
            phase: match record.phase {
                Phase::Build => "build",
                Phase::Test => "test",
            }
            .into(),
            label: record.label.clone(),
            by: record.handle.clone(),
            commit: record.commit.clone(),
            host: record.host.clone(),
            queued: record.queued_at,
            started: record.started_at,
            ended: record.ended_at,
            result: match (record.state, record.exit_code, record.signal) {
                (Step::Queued, ..) => RunResult::Queued,
                (Step::Running, ..) => RunResult::Running,
                (Step::Finished, Some(0), _) => RunResult::Green,
                (Step::Finished, Some(_), _) | (Step::Finished, None, Some(_)) => RunResult::Red,
                (Step::Finished, None, None) => RunResult::Unknown,
            },
        })
        .collect()
}

/// `gate`'s states over time, oldest first, on a task made at `created`: a state taken before it
/// from then, one with no time where the one before it ended (from `created` when it is the only
/// one), each no earlier than the one before, and one another followed at once left out.
fn lane(gate: &Gate, created: f64) -> GateLane {
    let mut spans: Vec<GateSpan> = gate
        .past
        .iter()
        .map(|past| GateSpan {
            state: past.state,
            from: past.at,
            by: past.by.clone(),
            detail: past.detail.clone(),
        })
        .collect();
    let since = gate
        .at
        .or_else(|| spans.last().map(|last| last.from))
        .unwrap_or(created);
    spans.push(GateSpan {
        state: gate.state,
        from: since,
        by: gate.by.clone(),
        detail: gate.detail.clone(),
    });
    let mut floor = created;
    for span in &mut spans {
        span.from = span.from.max(floor);
        floor = span.from;
    }
    let mut states: Vec<GateSpan> = Vec::new();
    for span in spans {
        if states.last().is_some_and(|last| last.from >= span.from) {
            states.pop();
        }
        states.push(span);
    }
    GateLane {
        name: gate.name.clone(),
        states,
    }
}

/// Seconds to add to `at` for this machine's local clock, as `clock::stamp_at` reads it.
#[expect(
    clippy::cast_possible_truncation,
    reason = "the board's times are Unix seconds as f64, read here to the minute"
)]
pub fn offset(at: f64) -> i64 {
    let minute = (at / 60.0).floor() as i64;
    crate::clock::stamp_minutes(&crate::clock::stamp_at((minute * 60) as libc::time_t))
        .map_or(0, |local| (local - minute) * 60)
}

/// This machine's local clock from `from` to `to` ([`TaskTimeline::zone`]).
pub fn zone(from: f64, to: f64) -> Vec<Offset> {
    changes(from, to, offset)
}

/// The offsets `offset_at` gives from `from` to `to`: the one at `from`, then each change of it to
/// the minute. It is looked at once a day, and a change found between two looks is narrowed down:
/// a clock that changes twice within a day is taken to change once.
fn changes(from: f64, to: f64, offset_at: impl Fn(f64) -> i64) -> Vec<Offset> {
    const DAY: f64 = 86_400.0;
    let mut zone = vec![Offset {
        from,
        seconds: offset_at(from),
    }];
    let mut at = from;
    while at < to {
        let next = (at + DAY).min(to);
        let before = zone.last().map_or(0, |last| last.seconds);
        let after = offset_at(next);
        if after != before {
            let (mut early, mut late) = (at, next);
            while late - early > 60.0 {
                let middle = f64::midpoint(early, late);
                if offset_at(middle) == before {
                    early = middle;
                } else {
                    late = middle;
                }
            }
            zone.push(Offset {
                from: late,
                seconds: after,
            });
        }
        at = next;
    }
    zone
}

/// The timeline of `record` in the project at `root`: its thread, the runs the board's recorder
/// kept for it and its gates, `pending` the cards waiting on the owner and `kept` the plugins'
/// answers. The thread and the recorder's runs are read once, for the timeline and its gates
/// both, the runs as `kept` holds them ([`crate::gates::Kept::runs`]). Gates that cannot be read
/// leave their lanes out, never the timeline.
pub fn answer(
    store: &symphony_board::Store,
    record: &symphony_board::Task,
    root: &Path,
    pending: &Pending,
    kept: &crate::gates::Kept,
) -> Result<TaskTimeline, String> {
    let thread = store.thread(&record.slug).map_err(|e| e.to_string())?;
    let folder = crate::gates::work_folder(root);
    let all = kept.runs(root);
    let mut gates = crate::gates::answer_from(store, record, root, pending, kept, &thread, &all)
        .map(|answer| answer.gates)
        .unwrap_or_default();
    // A stamp left out has no lane: its gate, listed beside the timeline, says why.
    gates.retain(|gate| !crate::gates::stamps::left_out(gate));
    let rotated = folder
        .as_deref()
        .is_some_and(|folder| folder.join(recorded::ROTATED).exists());
    let runs_from = kept_from(rotated, &all, record.created);
    let runs: Vec<Record> = all
        .iter()
        .filter(|run| run.task.as_deref() == Some(record.slug.as_str()))
        .cloned()
        .collect();
    let now = store.now();
    let facts = Facts {
        status: &record.status,
        created: record.created,
        updated: record.updated,
    };
    Ok(TaskTimeline {
        zone: zone(record.created, now),
        every: TIMELINE_EVERY,
        runs_from,
        history: Some(history::answer(store, record)),
        places: places::read(store, &record.slug),
        ..build(&record.slug, facts, &thread, &runs, &gates, now)
    })
}

/// Since when the recorder's history keeps every run, for a task made at `created`: a history
/// that has `rotated` has let its oldest runs go, and what it keeps (`all`, every task's) begins
/// at its oldest run. `None` when nothing went, or when what went came before the task.
fn kept_from(rotated: bool, all: &[Record], created: f64) -> Option<f64> {
    let oldest = all
        .iter()
        .map(|run| run.queued_at)
        .fold(f64::INFINITY, f64::min);
    (rotated && oldest.is_finite() && oldest > created).then_some(oldest)
}

#[cfg(test)]
#[path = "timeline/tests.rs"]
mod tests;
