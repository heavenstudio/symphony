//! The Runs page's reads (docs/RUNS.md, *The Runs page*): the list of the project's runs
//! ([`symphony_api::RUNS_PATH`]) and one run with its steps ([`symphony_api::RUN_PATH`]), from the
//! day files the board's recorder appends beside its logs in `symphony/runs/`, folded a run at a
//! time ([`fold`]); never the board, so a board slow or down leaves them as they are.
//!
//! Each read takes the day files newest first, back [`RUNS_DAYS`] days at most, and stops once what
//! it answers is whole: the list once more runs that match were asked for since the start of the
//! day just read than a page holds (every run asked for from that day on has its start line and its
//! end there or later), a run once the day it was asked for is read. A file is read only up to
//! [`RUNS_FILE_BYTES`] and an answer at most [`RUNS_READ_BYTES`]; a file past either, a link,
//! anything but a regular file or one that does not read is named in the answer's `unread` and left
//! out. A line that does not parse, one a writer that died cut short among them, is passed over.
//!
//! What a read folds of a day file is kept in the project's [`Index`], by the file's length,
//! modification time and inode, so the next read parses it again only once one of them changed: a
//! closed day is parsed once, today's once each time the recorder wrote to it. It keeps the days of
//! the window alone.

use axum::http::StatusCode;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::Read as _;
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use symphony_api::{
    End, RUNS_DAYS, RUNS_FILE_BYTES, RUNS_PAGE, RUNS_READ_BYTES, RunLine, RunPage, RunsPage, fold,
    run_day, run_name,
};

const DAY: f64 = 86_400.0;

/// What the reads keep of the day files between them: each file's fold by the path read, with what
/// its status said when it was read. One is held by each project's hub ([`crate::live::Hub`]).
#[derive(Default)]
pub(crate) struct Index {
    days: Mutex<HashMap<PathBuf, Kept>>,
}

/// A day file's fold as kept, and the length, modification time and inode it was read at.
struct Kept {
    seen: Seen,
    day: Arc<Day>,
}

/// What a day file's status says of its bytes: its length, modification time (s, ns), inode and
/// device.
type Seen = (u64, i64, i64, u64, u64);

/// A day file's runs, each once ([`fold`]), and the pipelines they name.
#[derive(Default)]
struct Day {
    lines: Vec<Arc<RunLine>>,
    pipelines: BTreeSet<String>,
}

impl Index {
    fn days(&self) -> std::sync::MutexGuard<'_, HashMap<PathBuf, Kept>> {
        self.days.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The fold kept of `path` read at `seen`; none when it was read at another or never.
    fn kept(&self, path: &Path, seen: Seen) -> Option<Arc<Day>> {
        self.days()
            .get(path)
            .filter(|kept| kept.seen == seen)
            .map(|kept| kept.day.clone())
    }

    fn keep(&self, path: &Path, seen: Seen, day: Arc<Day>) {
        self.days().insert(path.to_owned(), Kept { seen, day });
    }

    fn forget(&self, path: &Path) {
        self.days().remove(path);
    }

    /// Keeps only the days of `runs` of the window ending on the day starting at `today`.
    fn window(&self, runs: &Path, today: f64) {
        let window: HashSet<PathBuf> = (0..RUNS_DAYS)
            .map(|back| day_path(runs, today - f64::from(back) * DAY))
            .collect();
        self.days().retain(|path, _| window.contains(path));
    }

    /// How many day files it keeps.
    #[cfg(test)]
    fn len(&self) -> usize {
        self.days().len()
    }
}

/// What a read of the list asks for: the runs of a pipeline, that ended a way (or have not ended,
/// `Some(None)`), for a task, on a branch, asked for at or before a time (the last of the page
/// before, so a run asked for in the same second is not lost between them). Each is matched whole.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Filters {
    pub(crate) pipeline: Option<String>,
    pub(crate) end: Option<Option<End>>,
    pub(crate) task: Option<String>,
    pub(crate) branch: Option<String>,
    pub(crate) before: Option<f64>,
}

impl Filters {
    /// Whether `line` is a run, not a step, that the filters take.
    fn take(&self, line: &RunLine) -> bool {
        line.parent.is_none()
            && self
                .pipeline
                .as_ref()
                .is_none_or(|pipeline| *pipeline == line.pipeline)
            && self.end.is_none_or(|end| end == line.end)
            && self
                .task
                .as_ref()
                .is_none_or(|task| line.task.as_ref() == Some(task))
            && self
                .branch
                .as_ref()
                .is_none_or(|branch| line.branch.as_ref() == Some(branch))
            && self.before.is_none_or(|before| line.queued <= before)
    }
}

/// The filters a request names (`pipeline`, `end`, `task`, `branch`, `before`), an empty one
/// none; refused, saying why, for a pipeline that is no pipeline's name, an end that is none of
/// [`End`]'s nor `started`, or a time that is no number.
pub(crate) fn asked(
    pipeline: Option<&str>,
    end: Option<&str>,
    task: Option<&str>,
    branch: Option<&str>,
    before: Option<&str>,
) -> Result<Filters, String> {
    let given = |value: Option<&str>| value.filter(|value| !value.is_empty()).map(str::to_owned);
    let pipeline = given(pipeline);
    if let Some(name) = &pipeline
        && !run_name(name)
    {
        return Err(format!("pipeline is a pipeline's name, not {name:?}"));
    }
    let end = match given(end) {
        None => None,
        Some(word) if word == "started" => Some(None),
        Some(word) => Some(Some(
            serde_json::from_value::<End>(serde_json::Value::String(word.clone())).map_err(
                |_| {
                    format!(
                        "end is green, red, timed_out, interrupted, incomplete or started, not {word:?}"
                    )
                },
            )?,
        )),
    };
    let before = match given(before) {
        None => None,
        Some(text) => Some(
            text.parse::<f64>()
                .ok()
                .filter(|at| at.is_finite())
                .ok_or_else(|| format!("before is a time in seconds, not {text:?}"))?,
        ),
    };
    Ok(Filters {
        pipeline,
        end,
        task: given(task),
        branch: given(branch),
        before,
    })
}

/// Whether `run` names a run: 32 lower-case hex digits, as the recorder writes an operation's id
/// (the day files hold no other, so an id in capitals would find none).
fn is_run(run: &str) -> bool {
    run.len() == 32
        && run
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

/// The start of the UTC day `at` falls in.
fn day_of(at: f64) -> f64 {
    (at / DAY).floor() * DAY
}

/// The day file of the UTC day starting at `day` in `runs`.
fn day_path(runs: &Path, day: f64) -> PathBuf {
    runs.join(format!("{}.jsonl", run_day(day)))
}

/// The runs of the day file of the UTC day starting at `day` in `runs`, each once ([`fold`]), its
/// bytes taken from `budget` whether `index` kept its fold or it is parsed now; none when there is
/// none. A link, a FIFO or anything but a regular file, one larger than [`RUNS_FILE_BYTES`] or than
/// what is left of `budget`, or one that cannot be read, is refused in a sentence naming it by its
/// name alone (the server's folders are not the page's to know).
fn day_file(index: &Index, runs: &Path, day: f64, budget: &mut u64) -> Result<Arc<Day>, String> {
    let path = day_path(runs, day);
    let name = format!("{}.jsonl", run_day(day));
    // Nothing is kept of a file gone, or refused but for the answer's budget.
    let gone = |why: String| {
        index.forget(&path);
        Err(why)
    };
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(&path)
    {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            index.forget(&path);
            return Ok(Arc::default());
        }
        Err(e) => return gone(format!("{name}: {e}")),
    };
    let meta = match file.metadata() {
        Ok(meta) => meta,
        Err(e) => return gone(format!("{name}: {e}")),
    };
    if !meta.is_file() {
        return gone(format!("{name}: not a regular file"));
    }
    let size = meta.len();
    if size > RUNS_FILE_BYTES {
        return gone(format!(
            "{name}: {size} bytes, over the {RUNS_FILE_BYTES} a day file is read to"
        ));
    }
    if size > *budget {
        return Err(format!(
            "{name}: not read, as the {RUNS_READ_BYTES} bytes one answer reads were read"
        ));
    }
    *budget -= size;
    let seen = (
        size,
        meta.mtime(),
        meta.mtime_nsec(),
        meta.ino(),
        meta.dev(),
    );
    if let Some(day) = index.kept(&path, seen) {
        return Ok(day);
    }
    // The bytes the status counted, no more: a line the recorder appends meanwhile changes the
    // length, so the next read parses the file again with it.
    let mut bytes = Vec::new();
    if let Err(e) = file.take(size).read_to_end(&mut bytes) {
        return gone(format!("{name}: {e}"));
    }
    let lines = fold(
        bytes
            .split(|byte| *byte == b'\n')
            .filter_map(|line| serde_json::from_slice::<RunLine>(line).ok()),
    );
    let day = Arc::new(Day {
        pipelines: lines.iter().map(|line| line.pipeline.clone()).collect(),
        lines: lines.into_iter().map(Arc::new).collect(),
    });
    index.keep(&path, seen, day.clone());
    Ok(day)
}

/// The runs of an older day taken into `newer`'s, as a fold of every line in order would take
/// them: a run's first end line, else its newest start line, so an older day's line stands only
/// where it ended the run.
fn merge<'a>(
    newer: &mut HashMap<String, Arc<RunLine>>,
    older: impl IntoIterator<Item = &'a Arc<RunLine>>,
) {
    for line in older {
        if !line.is_start() || !newer.contains_key(&line.run) {
            newer.insert(line.run.clone(), line.clone());
        }
    }
}

/// The page of the runs `filters` take in the day files of `runs`, read at `now` through `index`.
pub(crate) fn page(index: &Index, runs: &Path, filters: &Filters, now: f64) -> RunsPage {
    let today = day_of(now);
    let first = today - f64::from(RUNS_DAYS - 1) * DAY;
    // A run ends on the day it was asked for or the next as a rule, so the page after `before`
    // finds its runs' end lines from the day after that time's.
    let mut day = filters
        .before
        .map_or(today, |before| (day_of(before) + DAY).min(today));
    let mut budget = RUNS_READ_BYTES;
    let mut seen: HashMap<String, Arc<RunLine>> = HashMap::new();
    let mut pipelines = BTreeSet::new();
    let mut unread = Vec::new();
    while day >= first {
        match day_file(index, runs, day, &mut budget) {
            Ok(read) => {
                pipelines.extend(read.pipelines.iter().cloned());
                merge(&mut seen, &read.lines);
            }
            Err(why) => unread.push(why),
        }
        let whole = seen
            .values()
            .filter(|line| line.queued >= day && filters.take(line))
            .count();
        if whole > RUNS_PAGE {
            break;
        }
        day -= DAY;
    }
    index.window(runs, today);
    if let Some(pipeline) = &filters.pipeline {
        pipelines.insert(pipeline.clone());
    }
    let mut taken: Vec<Arc<RunLine>> = seen
        .into_values()
        .filter(|line| filters.take(line))
        .collect();
    taken.sort_by(|a, b| {
        b.queued
            .total_cmp(&a.queued)
            .then_with(|| a.run.cmp(&b.run))
    });
    let more = taken.len() > RUNS_PAGE;
    RunsPage {
        runs: taken
            .into_iter()
            .take(RUNS_PAGE)
            .map(|line| RunLine::clone(&line))
            .collect(),
        more,
        pipelines: pipelines.into_iter().collect(),
        unread,
    }
}

/// Every step of `steps` under `run`, each after the one it ran inside, the steps of one in the
/// order they were asked for. A step met twice (a loop of parents) is taken once.
fn under(run: &str, steps: &[Arc<RunLine>]) -> Vec<RunLine> {
    let mut ordered = Vec::new();
    let mut taken: HashSet<&str> = HashSet::from([run]);
    // The parents still to visit, the next on top: a step's own steps come before its next sibling.
    let mut stack: Vec<&RunLine> = Vec::new();
    let children = |parent: &str| -> Vec<&RunLine> {
        let mut found: Vec<&RunLine> = steps
            .iter()
            .map(|step| &**step)
            .filter(|step| step.parent.as_deref() == Some(parent))
            .collect();
        found.sort_by(|a, b| {
            a.queued
                .total_cmp(&b.queued)
                .then_with(|| a.run.cmp(&b.run))
        });
        found
    };
    stack.extend(children(run).into_iter().rev());
    while let Some(step) = stack.pop() {
        if !taken.insert(step.run.as_str()) {
            continue;
        }
        ordered.push(step.clone());
        stack.extend(children(&step.run).into_iter().rev());
    }
    ordered
}

/// The run or step `id` and its steps, from the day files of `runs` read at `now` through `index`;
/// why not when none of the days read holds it.
pub(crate) fn run(index: &Index, runs: &Path, id: &str, now: f64) -> Result<RunPage, String> {
    let today = day_of(now);
    let first = today - f64::from(RUNS_DAYS - 1) * DAY;
    let mut budget = RUNS_READ_BYTES;
    // The run's lines and every step's, folded across the days read.
    let mut lines: HashMap<String, Arc<RunLine>> = HashMap::new();
    let mut unread = Vec::new();
    // Once found, the day it was asked for: its steps were asked for that day or later.
    let mut asked: Option<f64> = None;
    let mut day = today;
    while day >= first && asked.is_none_or(|asked| day >= asked) {
        match day_file(index, runs, day, &mut budget) {
            Ok(found) => merge(
                &mut lines,
                found
                    .lines
                    .iter()
                    .filter(|line| line.run == id || line.parent.is_some()),
            ),
            Err(why) => unread.push(why),
        }
        if let Some(found) = lines.get(id) {
            asked = Some(day_of(found.queued));
        }
        day -= DAY;
    }
    index.window(runs, today);
    let Some(run) = lines.remove(id).map(|run| RunLine::clone(&run)) else {
        let unread = match unread.len() {
            0 => String::new(),
            n => format!(", {n} of them not read"),
        };
        return Err(format!(
            "no run {id} in the last {RUNS_DAYS} days' files{unread}"
        ));
    };
    let steps: Vec<Arc<RunLine>> = lines.into_values().collect();
    Ok(RunPage {
        steps: under(id, &steps),
        run,
        unread,
    })
}

/// The day files' folder of the project `hub` serves, as the reports find it, or the status and
/// why it has none.
fn folder(hub: &crate::live::Hub) -> Result<std::path::PathBuf, (StatusCode, String)> {
    hub.report_folders
        .get_or_init(|| super::reports::Folders::of(&hub.root))
        .runs
        .clone()
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                "this project keeps no runs where this server reads".into(),
            )
        })
}

/// The page of runs `filters` take, of the project `hub` serves, at its clock.
pub(crate) fn answer(
    hub: &crate::live::Hub,
    filters: &Filters,
) -> Result<RunsPage, (StatusCode, String)> {
    Ok(page(&hub.runs_index, &folder(hub)?, filters, hub.now()))
}

/// The run `id` of the project `hub` serves and its steps, at its clock, or the status and why
/// not: refused for an id that is no run's, none for a run no day file read holds.
pub(crate) fn answer_run(
    hub: &crate::live::Hub,
    id: &str,
) -> Result<RunPage, (StatusCode, String)> {
    if !is_run(id) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("run is a run's id, 32 lower-case hex digits, not {id:?}"),
        ));
    }
    run(&hub.runs_index, &folder(hub)?, id, hub.now()).map_err(|why| (StatusCode::NOT_FOUND, why))
}

#[cfg(test)]
mod tests;
