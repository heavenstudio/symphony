//! The agents' activity as the machine's one server sees it (docs/workflow/AGENT_ACTIVITY.md): each
//! session's health as the board snapshot of each tick works it out, kept from tick to tick, so a
//! session that went stale or lost and came back, or left, is said once it does, and once an hour
//! what the hour held, by kind and model: the sessions started, gone stale and gone lost, the time
//! they spent so, the sessions running work, and the machine's load beside them.
//!
//! Nothing here reads or writes the board: [`Recorder::observe`] takes the snapshot the server's
//! tick made already, and [`Recorder::record`] appends the lines it says to the day files
//! `symphony/agents/YYYY-MM-DD.jsonl` in the git common dir ([`folder`]). Only the machine's one
//! server keeps a recorder (`startup.rs`); a private server keeps none and writes nothing.

use crate::lock::Held as _;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use symphony_board::recording::{Gate, State};
use symphony_board::runs::{ends_a_line, regular};
use symphony_board::{Board, Health, Session};

/// The day files' folder in the git common dir, in the one folder of Symphony's there, as the runs'
/// (docs/workflow/RUNS.md).
pub(crate) const FOLDER: &str = "symphony/agents";
/// Names the day files' folder in place of the common dir's: a test's own.
pub(crate) const FOLDER_VAR: &str = "SYMPHONY_AGENT_ACTIVITY";
/// The version of the lines' shape, in every line.
pub(crate) const SCHEMA: u32 = symphony_api::AGENT_ACTIVITY_SCHEMA;
/// How many days a day file is kept after its day.
pub(crate) const KEPT_DAYS: u32 = 90;
/// The longest time between two snapshots that counts as watched, in seconds: past it (a board
/// that did not answer, a server that was busy or stopped) the server saw nothing in between, and
/// that time is not counted.
pub(crate) const GAP: f64 = 120.0;
/// The roles whose holders are counted running.
pub(crate) const RUNNING_ROLES: [&str; 3] = ["executor", "reviewer", "orchestrator"];
/// The statuses whose tasks' live role holders are counted running.
pub(crate) const RUNNING_STATUSES: [&str; 2] = ["in_progress", "review"];
const HOUR: f64 = 3600.0;
const DAY: f64 = 86_400.0;

/// Where the board `env` reaches keeps its agents' day files: the folder [`FOLDER_VAR`] names; none
/// for a board `TASKS_SERVER` names (a test's) without it; else [`FOLDER`] in the git common dir of
/// its working directory, `symphony/agents.<database>/` for a board database other than the live
/// one.
pub(crate) fn folder(env: &symphony_board::cli::Env) -> Result<PathBuf, String> {
    symphony_board::reports::day_files(env, FOLDER, FOLDER_VAR, "agents' day files")
}

/// The lines' types are the wire's ([`symphony_api::agent_activity`]), which the Reports page reads.
pub(crate) use symphony_api::agent_activity::{
    AgentChange as Change, AgentHour as Hour, AgentLine as Line, AgentSeen as Seen,
    AgentStart as Start, Gauge, KindLevel, Level, RoleLevel, RoleModel, SessionHour,
};

/// The line `text` says; none when it does not read as one, as a reader of the files reads it:
/// by its `type`, one struct at a time ([`symphony_api::agent_activity`]). The server writes these
/// lines and reads only their hours (`api/reports.rs`); the tests read every line back.
#[cfg(test)]
pub(crate) fn read_line(text: &str) -> Option<Line> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let line = match value.get("type")?.as_str()? {
        "start" => Line::Start(serde_json::from_value(value).ok()?),
        "seen" => Line::Seen(serde_json::from_value(value).ok()?),
        "change" => Line::Change(serde_json::from_value(value).ok()?),
        "hour" => Line::Hour(serde_json::from_value(value).ok()?),
        _ => return None,
    };
    Some(line)
}

/// The whole hour `at` falls in: its start.
fn hour_of(at: f64) -> f64 {
    (at / HOUR).floor() * HOUR
}

/// The UTC day `at` falls in, `YYYY-MM-DD`, as a day file is named: what the tests name one by.
#[cfg(test)]
pub(crate) use symphony_api::run_day as day;

/// `value` to the thousandth: a line says no more than that.
fn rounded(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

/// The machine's load average over 1 and over 15 minutes; none when it cannot be read.
pub(crate) fn load_average() -> Option<[f64; 2]> {
    symphony_board::sys::load_average().map(|[one, _, fifteen]| [one, fifteen])
}

/// Who writes a line ([`Start`]).
#[derive(Debug, Clone, Copy)]
struct Writer {
    pid: u32,
    generation: f64,
}

/// A session as the last snapshot showed it.
#[derive(Debug, Clone)]
struct Tracked {
    handle: String,
    kind: String,
    model: String,
    connected: f64,
    health: Health,
}

impl Tracked {
    fn of(session: &Session) -> Tracked {
        let row = &session.row;
        Tracked {
            handle: row.handle.clone(),
            kind: row.kind.clone(),
            model: row.model.clone(),
            connected: row.connected,
            health: session.health,
        }
    }

    /// As `session` shows it now: its handle the same, nothing copied that did not change.
    fn update(&mut self, session: &Session) {
        let row = &session.row;
        if self.kind != row.kind {
            self.kind.clone_from(&row.kind);
        }
        if self.model != row.model {
            self.model.clone_from(&row.model);
        }
        self.connected = row.connected;
        self.health = session.health;
    }
}

/// `change` made to the value `map` holds at `key`, a default one put there first when it holds
/// none; the key is copied only then.
fn update<K: Ord + Clone, V: Default>(
    map: &mut BTreeMap<K, V>,
    key: &K,
    change: impl FnOnce(&mut V),
) {
    match map.get_mut(key) {
        Some(value) => change(value),
        None => change(map.entry(key.clone()).or_default()),
    }
}

/// What one snapshot counted of the sessions holding an executor, reviewer or orchestrator role on
/// a task in progress or in review: by role and model, by role (each once for each such role it
/// holds), and the sessions each once.
#[derive(Debug, Clone, Default)]
struct Holders {
    by_model: BTreeMap<(String, String), u64>,
    by_role: BTreeMap<String, u64>,
    agents: u64,
}

/// What one snapshot counted: the sessions running and holding work, the live sessions by kind.
#[derive(Debug, Clone, Default)]
struct Counted {
    running: Holders,
    holding: Holders,
    live: BTreeMap<String, u64>,
}

/// A value summed over the hour: its peak, the value times the seconds it held, and the sum of
/// the samples (the mean of an hour with no time watched).
#[derive(Debug, Clone, Copy, Default)]
struct Sum {
    peak: f64,
    timed: f64,
    sampled: f64,
}

impl Sum {
    fn held(&mut self, value: f64, seconds: f64) {
        self.peak = self.peak.max(value);
        self.timed += value * seconds;
    }

    fn sampled(&mut self, value: f64) {
        self.peak = self.peak.max(value);
        self.sampled += value;
    }

    fn gauge(&self, observed: f64, samples: u64) -> Gauge {
        let mean = if observed > 0.0 {
            self.timed / observed
        } else if samples > 0 {
            self.sampled / samples as f64
        } else {
            0.0
        };
        Gauge {
            peak: rounded(self.peak),
            mean: rounded(mean),
        }
    }

    fn level(&self, observed: f64, samples: u64) -> Level {
        let gauge = self.gauge(observed, samples);
        Level {
            peak: gauge.peak as u64,
            mean: gauge.mean,
        }
    }
}

/// [`Holders`] summed over the hour.
#[derive(Debug, Clone, Default)]
struct HoldersSum {
    by_model: BTreeMap<(String, String), Sum>,
    by_role: BTreeMap<String, Sum>,
    agents: Sum,
}

impl HoldersSum {
    /// `holders` added to each sum by `add`, a count at a time.
    fn add(&mut self, holders: &Holders, add: impl Fn(&mut Sum, f64)) {
        for (key, count) in &holders.by_model {
            update(&mut self.by_model, key, |sum| add(sum, *count as f64));
        }
        for (role, count) in &holders.by_role {
            update(&mut self.by_role, role, |sum| add(sum, *count as f64));
        }
        add(&mut self.agents, holders.agents as f64);
    }

    fn levels(&self, observed: f64, samples: u64) -> (Vec<RoleModel>, Vec<RoleLevel>, Level) {
        let by_model = self
            .by_model
            .iter()
            .map(|((role, model), sum)| {
                let level = sum.level(observed, samples);
                RoleModel {
                    role: role.clone(),
                    model: model.clone(),
                    peak: level.peak,
                    mean: level.mean,
                }
            })
            .collect();
        let by_role = self
            .by_role
            .iter()
            .map(|(role, sum)| {
                let level = sum.level(observed, samples);
                RoleLevel {
                    role: role.clone(),
                    peak: level.peak,
                    mean: level.mean,
                }
            })
            .collect();
        (by_model, by_role, self.agents.level(observed, samples))
    }
}

/// A kind and model's health over the hour.
#[derive(Debug, Clone, Copy, Default)]
struct Quiet {
    went_stale: u64,
    went_lost: u64,
    stale_s: f64,
    lost_s: f64,
}

/// The hour being summed.
#[derive(Debug, Clone)]
struct Summing {
    hour: f64,
    from: f64,
    observed: f64,
    samples: u64,
    quiet: BTreeMap<(String, String), Quiet>,
    running: HoldersSum,
    holding: HoldersSum,
    live: BTreeMap<String, Sum>,
    /// The load over 1 and over 15 minutes, once one was read.
    load: Option<[Sum; 2]>,
}

impl Summing {
    fn new(hour: f64, from: f64) -> Summing {
        Summing {
            hour,
            from,
            observed: 0.0,
            samples: 0,
            quiet: BTreeMap::new(),
            running: HoldersSum::default(),
            holding: HoldersSum::default(),
            live: BTreeMap::new(),
            load: None,
        }
    }

    /// `change` made to the health of `kind` and `model`, their names copied only the first time
    /// (the hour holds a few of them).
    fn quiet(&mut self, kind: &str, model: &str, change: impl FnOnce(&mut Quiet)) {
        match self
            .quiet
            .iter_mut()
            .find(|((k, m), _)| k == kind && m == model)
        {
            Some((_, quiet)) => change(quiet),
            None => change(
                self.quiet
                    .entry((kind.to_string(), model.to_string()))
                    .or_default(),
            ),
        }
    }

    /// What a snapshot `counted`, and the load read then, added to each sum by `add`.
    fn add(&mut self, counted: &Counted, load: Option<[f64; 2]>, add: impl Fn(&mut Sum, f64)) {
        self.running.add(&counted.running, &add);
        self.holding.add(&counted.holding, &add);
        for (kind, count) in &counted.live {
            update(&mut self.live, kind, |sum| add(sum, *count as f64));
        }
        if let Some(load) = load {
            let sums = self.load.get_or_insert_default();
            for (sum, value) in sums.iter_mut().zip(load) {
                add(sum, value);
            }
        }
    }

    /// `seconds` of the hour, during which the sessions, the counts and the load were as the last
    /// snapshot left them.
    fn held(
        &mut self,
        seconds: f64,
        sessions: &[Tracked],
        counted: &Counted,
        load: Option<[f64; 2]>,
    ) {
        self.observed += seconds;
        for session in sessions {
            match session.health {
                Health::Stale => {
                    self.quiet(&session.kind, &session.model, |q| q.stale_s += seconds);
                }
                Health::Lost => {
                    self.quiet(&session.kind, &session.model, |q| q.lost_s += seconds);
                }
                _ => {}
            }
        }
        self.add(counted, load, |sum, value| sum.held(value, seconds));
    }

    /// A snapshot taken in the hour.
    fn sampled(&mut self, counted: &Counted, load: Option<[f64; 2]>) {
        self.samples += 1;
        self.add(counted, load, Sum::sampled);
    }

    /// Its line, written by `writer` at `at` and ending at `to`; `started` the sessions that
    /// connected in it by kind and model.
    fn line(
        &self,
        writer: Writer,
        at: f64,
        to: f64,
        started: BTreeMap<(String, String), u64>,
    ) -> Hour {
        let mut kinds: BTreeSet<&(String, String)> = self.quiet.keys().collect();
        kinds.extend(started.keys());
        let sessions = kinds
            .into_iter()
            .map(|key| {
                let quiet = self.quiet.get(key).copied().unwrap_or_default();
                SessionHour {
                    kind: key.0.clone(),
                    model: key.1.clone(),
                    started: started.get(key).copied().unwrap_or(0),
                    went_stale: quiet.went_stale,
                    went_lost: quiet.went_lost,
                    stale_s: rounded(quiet.stale_s),
                    lost_s: rounded(quiet.lost_s),
                }
            })
            .collect();
        let (observed, samples) = (self.observed, self.samples);
        let (running, running_roles, running_agents) = self.running.levels(observed, samples);
        let (holding, holding_roles, holding_agents) = self.holding.levels(observed, samples);
        Hour {
            schema: SCHEMA,
            at,
            pid: writer.pid,
            generation: writer.generation,
            start: self.hour,
            from: self.from,
            to,
            observed_s: rounded(observed),
            samples,
            sessions,
            running,
            running_roles,
            running_agents,
            holding,
            holding_roles,
            holding_agents,
            live: self
                .live
                .iter()
                .map(|(kind, sum)| {
                    let level = sum.level(observed, samples);
                    KindLevel {
                        kind: kind.clone(),
                        peak: level.peak,
                        mean: level.mean,
                    }
                })
                .collect(),
            load_1m: self.load.map(|load| load[0].gauge(observed, samples)),
            load_15m: self.load.map(|load| load[1].gauge(observed, samples)),
        }
    }
}

/// The task `handle` holds a role on that a line names, and that role: of its executor, reviewer
/// and orchestrator roles, one on an unfinished task first, then executing before reviewing before
/// orchestrating, then the newest; none when it holds none of them.
fn held(board: &Board, handle: &str) -> Option<(String, String)> {
    let rank = |role: &str| RUNNING_ROLES.iter().position(|r| *r == role);
    let mut best: Option<((bool, usize, f64), &str, &str)> = None;
    for task in &board.tasks {
        let finished = symphony_board::health::FINISHED.contains(&task.status.as_str());
        for role in task.roles.iter().filter(|role| role.handle == handle) {
            let Some(rank) = rank(&role.role) else {
                continue;
            };
            let key = (finished, rank, -role.since);
            let better = best.as_ref().is_none_or(|(before, _, _)| {
                (key.0, key.1)
                    .cmp(&(before.0, before.1))
                    .then(key.2.total_cmp(&before.2))
                    .is_lt()
            });
            if better {
                best = Some((key, task.slug.as_str(), role.role.as_str()));
            }
        }
    }
    best.map(|(_, task, role)| (task.to_string(), role.to_string()))
}

/// What `board` counts: of the sessions holding an executor, reviewer or orchestrator role on a
/// task in progress or in review, those live (running) and those live, stale or unknown (holding),
/// each once for each such role, by its model; and the live sessions by kind, whatever they hold.
fn counted(board: &Board) -> Counted {
    let mut held: BTreeMap<(&str, &str), bool> = BTreeMap::new();
    for task in board
        .tasks
        .iter()
        .filter(|task| RUNNING_STATUSES.contains(&task.status.as_str()))
    {
        for role in &task.roles {
            if RUNNING_ROLES.contains(&role.role.as_str())
                && matches!(role.health, Health::Live | Health::Stale | Health::Unknown)
            {
                let key = (role.role.as_str(), role.handle.as_str());
                held.insert(key, role.health == Health::Live);
            }
        }
    }
    let mut models: BTreeMap<&str, &str> = held.keys().map(|(_, handle)| (*handle, "")).collect();
    let mut counted = Counted::default();
    for session in &board.sessions {
        if let Some(model) = models.get_mut(session.row.handle.as_str()) {
            *model = session.row.model.as_str();
        }
        if session.health == Health::Live {
            update(&mut counted.live, &session.row.kind, |count| *count += 1);
        }
    }
    let holders = |live_only: bool| {
        let mut holders = Holders::default();
        let mut agents = BTreeSet::new();
        for ((role, handle), _) in held.iter().filter(|(_, live)| **live || !live_only) {
            let model = models.get(handle).copied().unwrap_or_default();
            *holders
                .by_model
                .entry((role.to_string(), model.to_string()))
                .or_default() += 1;
            *holders.by_role.entry(role.to_string()).or_default() += 1;
            agents.insert(*handle);
        }
        holders.agents = agents.len() as u64;
        holders
    };
    counted.running = holders(true);
    counted.holding = holders(false);
    counted
}

/// The line `session` of `board` says at `now`, its health in the snapshot before `before` (none
/// when it is new on the board): at the `first` snapshot, seen unless it left; after it, a change
/// when its health changed, counted in the hour `summing` when it went stale or lost.
fn say(
    board: &Board,
    session: &Session,
    before: Option<Health>,
    first: bool,
    now: f64,
    writer: Writer,
    summing: &mut Option<Summing>,
) -> Option<Line> {
    let row = &session.row;
    let said = if first {
        session.health != Health::Left
    } else {
        before != Some(session.health)
    };
    if !said {
        return None;
    }
    let (task, role) = held(board, &row.handle).unzip();
    if first {
        return Some(Line::Seen(Seen {
            schema: SCHEMA,
            at: now,
            pid: writer.pid,
            generation: writer.generation,
            handle: row.handle.clone(),
            kind: row.kind.clone(),
            model: row.model.clone(),
            task,
            role,
            health: session.health.as_str().to_string(),
        }));
    }
    if let Some(summing) = summing {
        match session.health {
            Health::Stale => summing.quiet(&row.kind, &row.model, |q| q.went_stale += 1),
            Health::Lost => summing.quiet(&row.kind, &row.model, |q| q.went_lost += 1),
            _ => {}
        }
    }
    Some(Line::Change(Change {
        schema: SCHEMA,
        at: now,
        pid: writer.pid,
        generation: writer.generation,
        handle: row.handle.clone(),
        kind: row.kind.clone(),
        model: row.model.clone(),
        task,
        role,
        from: before.map(|health| health.as_str().to_string()),
        to: Some(session.health.as_str().to_string()),
    }))
}

/// Where the sessions an hour started are counted from: the snapshot just taken, or, as the server
/// stops, the sessions of the last one.
#[derive(Clone, Copy)]
enum Rows<'a> {
    Board(&'a Board),
    Tracked,
}

/// The sessions of `sessions` (kind, model, when connected) that connected from `hour` up to `to`,
/// by kind and model.
fn started<'a>(
    sessions: impl Iterator<Item = (&'a str, &'a str, f64)>,
    hour: f64,
    to: f64,
) -> BTreeMap<(String, String), u64> {
    let mut started = BTreeMap::new();
    for (kind, model, connected) in sessions {
        if connected >= hour && connected < to {
            *started
                .entry((kind.to_string(), model.to_string()))
                .or_default() += 1;
        }
    }
    started
}

/// The one server's watch of its agents, and the folder it writes their day files to.
#[derive(Debug)]
pub(crate) struct Recorder {
    folder: PathBuf,
    /// Every session of the last snapshot, in the board's order (by when they connected, then
    /// handle); empty before the first.
    sessions: Vec<Tracked>,
    /// Whether a snapshot was observed since this recorder began: the first says what it finds as
    /// seen, no change.
    begun: bool,
    /// When the last snapshot was observed.
    last: Option<f64>,
    /// When its first snapshot was taken: the generation its lines name.
    generation: Option<f64>,
    /// What the last snapshot counted, and the load read then.
    counted: Counted,
    load: Option<[f64; 2]>,
    summing: Option<Summing>,
    /// The day (since the epoch) whose old files were last pruned.
    pruned: Option<i64>,
    /// Why the last write failed, while it fails.
    unwritten: Option<String>,
}

impl Recorder {
    /// A recorder that appends to the day files in `folder`.
    pub(crate) fn new(folder: PathBuf) -> Recorder {
        Recorder {
            folder,
            sessions: Vec::new(),
            begun: false,
            last: None,
            generation: None,
            counted: Counted::default(),
            load: None,
            summing: None,
            pruned: None,
            unwritten: None,
        }
    }

    fn writer(&self) -> Writer {
        Writer {
            pid: std::process::id(),
            generation: self.generation.unwrap_or_default(),
        }
    }

    /// The snapshot `board` taken at `now` by the server's clock, the load read then: its lines
    /// appended, and the old day files pruned once a day. A write that fails is said on stderr,
    /// once for each change of what it is, and its lines are not kept.
    pub(crate) fn record(&mut self, board: &Board, now: f64, load: Option<[f64; 2]>) {
        let lines = self.observe(board, now, load);
        self.write(&lines, now);
    }

    /// Stopping at `now`: what the hour being summed held so far, as its line
    /// ([`Recorder::flush`]), written.
    pub(crate) fn stop(&mut self, now: f64) {
        let lines = self.flush(now);
        self.write(&lines, now);
    }

    fn write(&mut self, lines: &[Line], now: f64) {
        if !lines.is_empty() {
            let written =
                append(&self.folder, lines).map_err(|e| format!("{}: {e}", self.folder.display()));
            match written {
                Ok(()) => self.unwritten = None,
                Err(why) => {
                    if self.unwritten.as_ref() != Some(&why) {
                        crate::front::tell(format_args!("· agent activity: {why}"));
                    }
                    self.unwritten = Some(why);
                }
            }
        }
        let today = (now / DAY).floor() as i64;
        if self.pruned != Some(today) && self.folder.is_dir() {
            prune(&self.folder, now);
            self.pruned = Some(today);
        }
    }

    /// The lines the snapshot `board` taken at `now` says: the hours it ended, each summed; at the
    /// first snapshot, a start line and each session not left as seen; after it, each session whose
    /// health changed since the snapshot before.
    pub(crate) fn observe(&mut self, board: &Board, now: f64, load: Option<[f64; 2]>) -> Vec<Line> {
        let mut lines = Vec::new();
        self.generation.get_or_insert(now);
        let load = load.or(self.load);
        self.advance(now, Rows::Board(board), &mut lines);
        self.roll(hour_of(now), now, now, Rows::Board(board), &mut lines);
        self.changes(board, now, &mut lines);
        self.counted = counted(board);
        if let Some(summing) = &mut self.summing {
            summing.sampled(&self.counted, load);
        }
        self.last = Some(now);
        self.load = load;
        lines
    }

    /// The time from the last snapshot to `now`, when it is at most [`GAP`], counted as the last
    /// snapshot left things, in each hour it spans; each hour it ends, summed as a line.
    fn advance(&mut self, now: f64, rows: Rows<'_>, lines: &mut Vec<Line>) {
        let Some(last) = self.last.filter(|last| now > *last && now - last <= GAP) else {
            return;
        };
        let mut from = last;
        while from < now {
            let hour = hour_of(from);
            let until = now.min(hour + HOUR);
            self.roll(hour, from, now, rows, lines);
            if let Some(summing) = &mut self.summing {
                summing.held(until - from, &self.sessions, &self.counted, self.load);
            }
            from = until;
        }
    }

    /// Sum `hour` from `from` on: an earlier hour being summed is ended, as a line written at `at`;
    /// a later one (a clock set back) goes on being summed.
    fn roll(&mut self, hour: f64, from: f64, at: f64, rows: Rows<'_>, lines: &mut Vec<Line>) {
        if self.summing.as_ref().is_some_and(|s| s.hour >= hour) {
            return;
        }
        if let Some(done) = self.summing.take() {
            let to = done.hour + HOUR;
            let started = self.started(rows, done.hour, to);
            lines.push(Line::Hour(done.line(self.writer(), at, to, started)));
        }
        self.summing = Some(Summing::new(hour, from));
    }

    /// The sessions `rows` holds that connected from `hour` up to `to`, by kind and model.
    fn started(&self, rows: Rows<'_>, hour: f64, to: f64) -> BTreeMap<(String, String), u64> {
        match rows {
            Rows::Board(board) => started(
                board
                    .sessions
                    .iter()
                    .map(|s| (s.row.kind.as_str(), s.row.model.as_str(), s.row.connected)),
                hour,
                to,
            ),
            Rows::Tracked => started(
                self.sessions
                    .iter()
                    .map(|t| (t.kind.as_str(), t.model.as_str(), t.connected)),
                hour,
                to,
            ),
        }
    }

    /// Each session of `board` as against the snapshot before, at `now`.
    fn changes(&mut self, board: &Board, now: f64, lines: &mut Vec<Line>) {
        let writer = self.writer();
        let first = !self.begun;
        self.begun = true;
        if first {
            lines.push(Line::Start(Start {
                schema: SCHEMA,
                at: now,
                pid: writer.pid,
                generation: writer.generation,
                sessions: board.sessions.len(),
                left: board
                    .sessions
                    .iter()
                    .filter(|s| s.health == Health::Left)
                    .count(),
            }));
        }
        let said = |session: &Session, before: Option<Health>, summing: &mut Option<Summing>| {
            say(board, session, before, first, now, writer, summing)
        };
        // The board keeps its sessions by when they connected, then handle, so a tick that adds
        // none or adds new ones finds those before in their order: only their health is compared.
        let kept = self.sessions.len() <= board.sessions.len()
            && self
                .sessions
                .iter()
                .zip(&board.sessions)
                .all(|(tracked, session)| tracked.handle == session.row.handle);
        if kept {
            for (n, session) in board.sessions.iter().enumerate() {
                let before = self.sessions.get(n).map(|tracked| tracked.health);
                lines.extend(said(session, before, &mut self.summing));
                match self.sessions.get_mut(n) {
                    Some(tracked) => tracked.update(session),
                    None => self.sessions.push(Tracked::of(session)),
                }
            }
            return;
        }
        let before = std::mem::take(&mut self.sessions);
        let at: std::collections::HashMap<&str, usize> = before
            .iter()
            .enumerate()
            .map(|(n, tracked)| (tracked.handle.as_str(), n))
            .collect();
        let mut on_board = vec![false; before.len()];
        for session in &board.sessions {
            let was = at.get(session.row.handle.as_str()).map(|n| {
                on_board[*n] = true;
                before[*n].health
            });
            lines.extend(said(session, was, &mut self.summing));
            self.sessions.push(Tracked::of(session));
        }
        for (session, _) in before.into_iter().zip(on_board).filter(|(_, on)| !on) {
            lines.push(Line::Change(Change {
                schema: SCHEMA,
                at: now,
                pid: writer.pid,
                generation: writer.generation,
                handle: session.handle,
                kind: session.kind,
                model: session.model,
                task: None,
                role: None,
                from: Some(session.health.as_str().to_string()),
                to: None,
            }));
        }
    }

    /// The lines a stop at `now` says: the time since the last snapshot counted, as
    /// [`Recorder::observe`] counts it, and the hour being summed ended at `now`, its sessions
    /// those of the last snapshot. No hour is summed after.
    pub(crate) fn flush(&mut self, now: f64) -> Vec<Line> {
        let mut lines = Vec::new();
        self.advance(now, Rows::Tracked, &mut lines);
        if let Some(done) = self.summing.take() {
            let to = now.clamp(done.from, done.hour + HOUR);
            let started = self.started(Rows::Tracked, done.hour, to);
            lines.push(Line::Hour(done.line(self.writer(), now, to, started)));
        }
        self.last = None;
        lines
    }
}

/// `lines` appended to the files of their days in `folder`, made as needed: each day's lines in one
/// write, opened to append, after a newline when a writer before was cut short in its line, so each
/// line stays its own. No lock is taken: one server records at a time ([`Gate`], its generation's).
pub(crate) fn append(folder: &Path, lines: &[Line]) -> io::Result<()> {
    let mut by_day: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for line in lines {
        let text = by_day.entry(line.day()).or_default();
        serde_json::to_writer(&mut *text, line).map_err(io::Error::other)?;
        text.push(b'\n');
    }
    if by_day.is_empty() {
        return Ok(());
    }
    fs::create_dir_all(folder)?;
    for (day, mut text) in by_day {
        let mut file = regular(
            &folder.join(format!("{day}.jsonl")),
            OpenOptions::new().read(true).append(true).create(true),
        )?;
        if !ends_a_line(&file)? {
            text.insert(0, b'\n');
        }
        file.write_all(&text)?;
    }
    Ok(())
}

/// Each day file in `folder` of a day more than [`KEPT_DAYS`] days before `now`'s removed; nothing
/// else there, and no link, folder or FIFO under such a name: the rule the runs' day files are
/// kept by.
pub(crate) fn prune(folder: &Path, now: f64) {
    symphony_board::runs::prune(folder, now, KEPT_DAYS);
}

/// The lines of `day`'s file in `folder`, in order; a line that does not read (cut short, an empty
/// one, one of a shape none of these is) is passed over, as [`read_line`] reads them.
#[cfg(test)]
pub(crate) fn read_day(folder: &Path, day: &str) -> Vec<Line> {
    let Ok(file) = regular(
        &folder.join(format!("{day}.jsonl")),
        OpenOptions::new().read(true),
    ) else {
        return Vec::new();
    };
    let mut text = String::new();
    let _ = io::Read::read_to_string(&mut &file, &mut text);
    text.lines().filter_map(read_line).collect()
}

/// The snapshot `board` of a tick recorded by `recorder` as its server generation's `gate`
/// ([`symphony_board::recording`]) lets it, at `now`, read while the gate is held so that a close
/// falls between two ticks: open, recorded; held, not; closed, the hour so far written once,
/// ending at the close.
pub(crate) fn gated_record(
    gate: &Gate,
    recorder: &mut Recorder,
    board: &Board,
    now: impl FnOnce() -> f64,
) {
    gate.with(|state| match state {
        State::Open { .. } => recorder.record(board, now(), load_average()),
        State::Held => {}
        State::Closed(at) => recorder.stop(at),
    });
}

/// `recorder`'s server stopping at `now`: the hour so far written, ending at the close if `gate`
/// closed.
pub(crate) fn gated_stop(gate: &Gate, recorder: &mut Recorder, now: impl FnOnce() -> f64) {
    gate.with(|state| match state {
        State::Open { .. } => recorder.stop(now()),
        State::Held => {}
        State::Closed(at) => recorder.stop(at),
    });
}

impl super::Hub {
    /// The gate of this hub's server generation, which both of its recorders keep to: the
    /// agents' here and the board's health hours ([`symphony_board::board_health::Recorder::gated`]),
    /// and so does the hourly roll-up's write of the day files (`live/reports.rs`). Until it is
    /// given, none of them records.
    pub(crate) fn gate_recorders(&self, gate: Arc<Gate>) {
        self.health.gated(Arc::clone(&gate));
        let _ = self.recording_gate.set(gate);
    }

    /// The snapshot `board` this tick took, recorded by the one server's recorder, at the server's
    /// time, as its gate lets it; a private server keeps none and does nothing.
    pub(super) fn record_agents(&self, board: &Board) {
        let Some(gate) = self.recording_gate.get() else {
            return;
        };
        let mut recorder = self.agent_activity.lock().held();
        if let Some(recorder) = recorder.as_mut() {
            super::measured("agent activity", || {
                gated_record(gate, recorder, board, || self.now());
            });
        }
    }

    /// The hour so far written as the server stops.
    pub(super) fn stop_agents(&self) {
        let Some(gate) = self.recording_gate.get() else {
            return;
        };
        let mut recorder = self.agent_activity.lock().held();
        if let Some(recorder) = recorder.as_mut() {
            gated_stop(gate, recorder, || self.now());
        }
    }
}

#[cfg(test)]
#[path = "agent_activity_tests.rs"]
mod tests;
