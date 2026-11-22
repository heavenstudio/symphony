//! The Runs page's reads (docs/workflow/RUNS.md, *The Runs page*): the list of the project's runs
//! ([`symphony_api::RUNS_PATH`]) and one run with its steps ([`symphony_api::RUN_PATH`]), from the
//! day files the board's recorder appends beside its logs in `symphony/runs/`, folded a run at a
//! time ([`symphony_api::fold`]); never the board, so a board slow or down leaves them as they are.
//!
//! Each read takes the day files newest first, back [`RUNS_DAYS`] days at most, and stops once what
//! it answers is whole: the list once more runs that match were asked for since the start of the
//! day just read than a page holds (every run asked for from that day on has its start line and its
//! end there or later), a run once the day it was asked for is read. A file is read only up to
//! [`RUNS_FILE_BYTES`] and an answer at most [`RUNS_READ_BYTES`]; a file past either, a link,
//! anything but a regular file or one that does not read is named in the answer's `unread` and left
//! out. A line that does not parse, one a writer that died cut short among them, is passed over.
//!
//! What a read folds of a day file is kept in the project's [`Index`]: of each run, the keys the
//! filters, the fold and the steps' order read, and where in the file its line is, by the file's
//! length, modification time and inode. An answer's lines (a page of runs, or one run and its
//! steps) are read again from the file, through the descriptor its status was taken on, and each
//! must still be the run kept there; a line changed under its offset has its day parsed again, and
//! the answer made again once. A file that only grew since, on the same inode, still holding the
//! last line read where it was, is read on from there: a closed day is parsed once, today's a line
//! at a time as the recorder writes them; any other change parses the file again. An earlier line
//! changed in place as the file grew, the last line read left where it was, is seen only once an
//! answer reads it back or the file is parsed again: the recorder only appends, so only a hand edit
//! makes one, and seeing it on each growth would read the whole file each time. A day is read, and
//! its runs folded into the answer, under the index's lock: an answer that asks for a day another
//! is reading waits for it and takes what it kept, so each byte appended is parsed once however
//! many answers ask. It keeps the days of the window alone.

use axum::http::StatusCode;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::hash::{DefaultHasher, Hasher as _};
use std::io::{Read as _, Seek as _, SeekFrom};
use std::os::unix::fs::{FileExt as _, MetadataExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use symphony_api::{
    End, RUNS_DAYS, RUNS_FILE_BYTES, RUNS_PAGE, RUNS_READ_BYTES, RunLine, RunPage, RunsPage,
    run_day, run_name,
};
use symphony_board::lock::Recovered as _;

const DAY: f64 = 86_400.0;

/// What the reads keep of the day files between them: each file's runs by the path read, with what
/// its status said when it was read, read and folded into an answer under its lock alone
/// ([`day_file`]). One is held by each project's hub ([`crate::live::Hub`]).
#[derive(Default)]
pub(crate) struct Index {
    days: Mutex<HashMap<PathBuf, Kept>>,
    /// How many bytes of day files it parsed.
    #[cfg(test)]
    parsed: std::sync::atomic::AtomicU64,
}

/// A day file's runs as kept, and the length, modification time and inode it was read at.
#[derive(Default)]
struct Kept {
    seen: Seen,
    day: Day,
}

/// What a day file's status says of its bytes: its length, modification time (s, ns), inode and
/// device.
type Seen = (u64, i64, i64, u64, u64);

/// A day file's runs, each once as [`symphony_api::fold`] takes them, by id; the pipelines they
/// name; the names their keys share, each held once; and where the last line read starts, with a
/// hash of its bytes, its newline among them: none when the bytes read did not end a line.
#[derive(Default)]
struct Day {
    runs: HashMap<Arc<str>, Key>,
    pipelines: BTreeSet<String>,
    names: HashSet<Arc<str>>,
    last: Option<(u64, u64)>,
}

/// A run's line as a day's fold keeps it: what the filters, the fold and [`under`] read of it, and
/// where it is in its file, `len` bytes from `at`, its newline left out.
#[derive(Clone, Debug)]
struct Key {
    parent: Option<Arc<str>>,
    queued: f64,
    end: Option<End>,
    pipeline: Arc<str>,
    task: Option<Arc<str>>,
    branch: Option<Arc<str>>,
    at: u64,
    len: u64,
}

impl Key {
    /// Whether `line`, read again, is still the line of `run` this was kept of.
    fn is(&self, run: &str, line: &RunLine) -> bool {
        line.run == run
            && line.parent.as_deref() == self.parent.as_deref()
            && line.queued.to_bits() == self.queued.to_bits()
            && line.end == self.end
            && line.pipeline == *self.pipeline
            && line.task.as_deref() == self.task.as_deref()
            && line.branch.as_deref() == self.branch.as_deref()
    }
}

/// The hash of a line's bytes that tells a longer file still holds it.
fn hash(bytes: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    hasher.write(bytes);
    hasher.finish()
}

impl Day {
    /// `text`, held once among the names of the day's keys.
    fn name(&mut self, text: String) -> Arc<str> {
        if let Some(name) = self.names.get(text.as_str()) {
            return name.clone();
        }
        let name: Arc<str> = text.into();
        self.names.insert(name.clone());
        name
    }

    /// Folds in the lines of `bytes`, the file's from `from` on, after the lines folded before, as
    /// [`symphony_api::fold`] takes them: a run's first end line, else its newest start line. A line
    /// that does not parse as a whole [`RunLine`] is passed over.
    #[expect(
        clippy::iter_over_hash_type,
        reason = "the pipelines are a set, the same in any order"
    )]
    fn fold_in(&mut self, bytes: &[u8], from: u64) {
        let mut at = from;
        let mut last = None;
        for piece in bytes.split_inclusive(|byte| *byte == b'\n') {
            let text = piece.strip_suffix(b"\n").unwrap_or(piece);
            if let Ok(line) = serde_json::from_slice::<RunLine>(text) {
                let key = Key {
                    parent: line.parent.map(|parent| self.name(parent)),
                    queued: line.queued,
                    end: line.end,
                    pipeline: self.name(line.pipeline),
                    task: line.task.map(|task| self.name(task)),
                    branch: line.branch.map(|branch| self.name(branch)),
                    at,
                    len: text.len() as u64,
                };
                match self.runs.get_mut(line.run.as_str()) {
                    None => {
                        self.runs.insert(line.run.into(), key);
                    }
                    Some(kept) if kept.end.is_none() => *kept = key,
                    Some(_) => {}
                }
            }
            last = Some((at, piece));
            at += piece.len() as u64;
        }
        self.last = last
            .filter(|(_, piece)| piece.ends_with(b"\n"))
            .map(|(at, piece)| (at, hash(piece)));
        self.pipelines.clear();
        for key in self.runs.values() {
            if !self.pipelines.contains(&*key.pipeline) {
                self.pipelines.insert(key.pipeline.to_string());
            }
        }
    }
}

impl Index {
    fn days(&self) -> std::sync::MutexGuard<'_, HashMap<PathBuf, Kept>> {
        #[cfg(test)]
        tests::before_day_lock();
        self.days.lock().recover()
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

    /// How many bytes of day files it parsed.
    #[cfg(test)]
    fn parsed(&self) -> u64 {
        self.parsed.load(std::sync::atomic::Ordering::Relaxed)
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
    /// Whether the line kept as `key` is a run, not a step, that the filters take.
    fn take(&self, key: &Key) -> bool {
        key.parent.is_none()
            && self
                .pipeline
                .as_deref()
                .is_none_or(|pipeline| pipeline == &*key.pipeline)
            && self.end.is_none_or(|end| end == key.end)
            && self
                .task
                .as_deref()
                .is_none_or(|task| key.task.as_deref() == Some(task))
            && self
                .branch
                .as_deref()
                .is_none_or(|branch| key.branch.as_deref() == Some(branch))
            && self.before.is_none_or(|before| key.queued <= before)
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

/// A day file as one answer read it: its path, and the descriptor its status was taken on, which
/// its answered lines are read again through.
struct Opened {
    path: PathBuf,
    file: File,
}

/// The day file of the UTC day starting at `day` in `runs`, open, its runs, each once, handed to
/// `answer` under `index`'s lock, and its bytes taken from `budget` whether `index` kept its runs,
/// they are read on or parsed now; none when there is none. A link, a FIFO or anything but a
/// regular file, one larger than [`RUNS_FILE_BYTES`] or than what is left of `budget`, or one that
/// cannot be read, is refused in a sentence naming it by its name alone (the server's folders are
/// not the page's to know).
fn day_file(
    index: &Index,
    runs: &Path,
    day: f64,
    budget: &mut u64,
    answer: &mut dyn FnMut(&Day),
) -> Result<Option<Opened>, String> {
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
            return Ok(None);
        }
        Err(e) => return gone(format!("{name}: {e}")),
    };
    // Take the status under the lock too: another reader may have stored a longer day while
    // this reader waited. Its descriptor sees the append, so it takes that reader's fold.
    let mut days = index.days();
    let mut gone = |why: String| {
        days.remove(&path);
        Err(why)
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
    // Under the lock to the end: an answer that asks for the day meanwhile waits, and takes what
    // this one kept.
    if let Some(kept) = days.get(&path)
        && kept.seen == seen
    {
        answer(&kept.day);
        return Ok(Some(Opened { path, file }));
    }
    // Out of the map until it is read, so nothing is kept of a file that does not read.
    let mut kept = days.remove(&path).unwrap_or_default();
    let from = grown(&kept, &file, seen).unwrap_or_else(|| {
        kept.day = Day::default();
        0
    });
    // The bytes the status counted, no more: a line the recorder appends meanwhile changes the
    // length, so the next read reads on to it.
    let mut bytes = Vec::new();
    let mut reader = &file;
    if let Err(e) = reader
        .seek(SeekFrom::Start(from))
        .and_then(|_| reader.take(size - from).read_to_end(&mut bytes))
    {
        return Err(format!("{name}: {e}"));
    }
    #[cfg(test)]
    index
        .parsed
        .fetch_add(bytes.len() as u64, std::sync::atomic::Ordering::Relaxed);
    kept.day.fold_in(&bytes, from);
    kept.seen = seen;
    answer(&kept.day);
    days.insert(path.clone(), kept);
    Ok(Some(Opened { path, file }))
}

/// Where the runs kept `before` of the file `file`, whose status is now `seen`, are read on from:
/// where they were read to, when the file only grew since, on the same inode and device, and still
/// holds the last line read where it was. None when it is to be parsed whole.
fn grown(before: &Kept, file: &File, seen: Seen) -> Option<u64> {
    let (size, .., inode, device) = seen;
    let (read, .., was_inode, was_device) = before.seen;
    let (at, kept) = before.day.last?;
    if size <= read || inode != was_inode || device != was_device {
        return None;
    }
    let mut last = vec![0; usize::try_from(read - at).ok()?];
    file.read_exact_at(&mut last, at).ok()?;
    (hash(&last) == kept).then_some(read)
}

/// A run as one answer found it: its id, its key, and the day it was read from, by its place among
/// the answer's [`Opened`] days.
#[derive(Clone)]
struct Found {
    run: Arc<str>,
    key: Key,
    day: usize,
}

/// The runs of an older day, the one at `day` among the answer's, that `keep` takes, taken into
/// `newer`'s, as a fold of every line in order would take them: a run's first end line, else its
/// newest start line, so an older day's line stands only where it ended the run.
#[expect(
    clippy::iter_over_hash_type,
    reason = "each run of a day is taken or not by itself, so in any order the same"
)]
fn merge(
    newer: &mut HashMap<Arc<str>, Found>,
    day: usize,
    older: &Day,
    keep: impl Fn(&str, &Key) -> bool,
) {
    for (run, key) in &older.runs {
        if keep(run, key) && (key.end.is_some() || !newer.contains_key(run)) {
            newer.insert(
                run.clone(),
                Found {
                    run: run.clone(),
                    key: key.clone(),
                    day,
                },
            );
        }
    }
}

/// The line `found` names, read again through its day's descriptor; none when the bytes there no
/// longer hold the line kept of that run.
fn line_of(days: &[Opened], found: &Found) -> Option<RunLine> {
    let mut bytes = vec![0; usize::try_from(found.key.len).ok()?];
    days.get(found.day)?
        .file
        .read_exact_at(&mut bytes, found.key.at)
        .ok()?;
    serde_json::from_slice::<RunLine>(&bytes)
        .ok()
        .filter(|line| found.key.is(&found.run, line))
}

/// The lines `found` name, read again ([`line_of`]), and the paths of the days that no longer hold
/// one, whose line is left out.
fn lines_of(days: &[Opened], found: &[Found]) -> (Vec<RunLine>, Vec<PathBuf>) {
    let mut changed = Vec::new();
    let lines = found
        .iter()
        .filter_map(|found| {
            let line = line_of(days, found);
            if line.is_none() {
                changed.extend(days.get(found.day).map(|day| day.path.clone()));
            }
            line
        })
        .collect();
    (lines, changed)
}

/// An answer made through `index` by `answer`, which says the days that no longer held a line it
/// answered: those are parsed again and the answer made again, once; a line changed under its
/// offset again meanwhile is left out.
fn read_twice<T>(index: &Index, answer: impl Fn() -> (T, Vec<PathBuf>)) -> T {
    let (first, changed) = answer();
    if changed.is_empty() {
        return first;
    }
    for path in &changed {
        index.forget(path);
    }
    answer().0
}

/// The page of the runs `filters` take in the day files of `runs`, read at `now` through `index`.
pub(crate) fn page(index: &Index, runs: &Path, filters: &Filters, now: f64) -> RunsPage {
    read_twice(index, || page_once(index, runs, filters, now))
}

/// [`page`], and the days that no longer held a line it answered.
fn page_once(index: &Index, runs: &Path, filters: &Filters, now: f64) -> (RunsPage, Vec<PathBuf>) {
    let today = day_of(now);
    let first = today - f64::from(RUNS_DAYS - 1) * DAY;
    // A run ends on the day it was asked for or the next as a rule, so the page after `before`
    // finds its runs' end lines from the day after that time's.
    let mut day = filters
        .before
        .map_or(today, |before| (day_of(before) + DAY).min(today));
    let mut budget = RUNS_READ_BYTES;
    let mut days: Vec<Opened> = Vec::new();
    let mut seen: HashMap<Arc<str>, Found> = HashMap::new();
    let mut pipelines = BTreeSet::new();
    let mut unread = Vec::new();
    while day >= first {
        let at = days.len();
        let read = day_file(index, runs, day, &mut budget, &mut |read| {
            pipelines.extend(read.pipelines.iter().cloned());
            merge(&mut seen, at, read, |_, _| true);
        });
        match read {
            Ok(Some(opened)) => days.push(opened),
            Ok(None) => {}
            Err(why) => unread.push(why),
        }
        let whole = seen
            .values()
            .filter(|found| found.key.queued >= day && filters.take(&found.key))
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
    let mut taken: Vec<Found> = seen
        .into_values()
        .filter(|found| filters.take(&found.key))
        .collect();
    symphony_board::order::sort_by(&mut taken, &|a, b| {
        b.key
            .queued
            .total_cmp(&a.key.queued)
            .then_with(|| a.run.cmp(&b.run))
    });
    let more = taken.len() > RUNS_PAGE;
    taken.truncate(RUNS_PAGE);
    let (runs, changed) = lines_of(&days, &taken);
    let page = RunsPage {
        runs,
        more,
        pipelines: pipelines.into_iter().collect(),
        unread,
    };
    (page, changed)
}

/// What [`under`] reads of a step: its id, the id of the run or step it ran inside, and when it was
/// asked for.
trait Step: Clone {
    fn id(&self) -> &str;
    fn parent(&self) -> Option<&str>;
    fn queued(&self) -> f64;
}

impl Step for Found {
    fn id(&self) -> &str {
        &self.run
    }

    fn parent(&self) -> Option<&str> {
        self.key.parent.as_deref()
    }

    fn queued(&self) -> f64 {
        self.key.queued
    }
}

#[cfg(test)]
impl Step for RunLine {
    fn id(&self) -> &str {
        &self.run
    }

    fn parent(&self) -> Option<&str> {
        self.parent.as_deref()
    }

    fn queued(&self) -> f64 {
        self.queued
    }
}

/// Every step of `steps` under `run`, each after the one it ran inside, the steps of one in the
/// order they were asked for. A step met twice (a loop of parents) is taken once.
fn under<S: Step>(run: &str, steps: &[impl std::ops::Deref<Target = S>]) -> Vec<S> {
    let mut ordered = Vec::new();
    let mut taken: HashSet<&str> = HashSet::from([run]);
    // The parents still to visit, the next on top: a step's own steps come before its next sibling.
    let mut stack: Vec<&S> = Vec::new();
    let children = |parent: &str| -> Vec<&S> {
        let mut found: Vec<&S> = steps
            .iter()
            .map(|step| &**step)
            .filter(|step| step.parent() == Some(parent))
            .collect();
        symphony_board::order::sort_by(&mut found, &|a, b| {
            a.queued()
                .total_cmp(&b.queued())
                .then_with(|| a.id().cmp(b.id()))
        });
        found
    };
    stack.extend(children(run).into_iter().rev());
    while let Some(step) = stack.pop() {
        if !taken.insert(step.id()) {
            continue;
        }
        ordered.push(step.clone());
        stack.extend(children(step.id()).into_iter().rev());
    }
    ordered
}

/// The run or step `id` and its steps, from the day files of `runs` read at `now` through `index`;
/// why not when none of the days read holds it.
pub(crate) fn run(index: &Index, runs: &Path, id: &str, now: f64) -> Result<RunPage, String> {
    read_twice(index, || run_once(index, runs, id, now))
}

/// [`run`], and the days that no longer held a line it answered.
fn run_once(
    index: &Index,
    runs: &Path,
    id: &str,
    now: f64,
) -> (Result<RunPage, String>, Vec<PathBuf>) {
    let today = day_of(now);
    let first = today - f64::from(RUNS_DAYS - 1) * DAY;
    let mut budget = RUNS_READ_BYTES;
    let mut days: Vec<Opened> = Vec::new();
    // The run's lines and every step's, folded across the days read.
    let mut lines: HashMap<Arc<str>, Found> = HashMap::new();
    let mut unread = Vec::new();
    // Once found, the day it was asked for: its steps were asked for that day or later.
    let mut asked: Option<f64> = None;
    let mut day = today;
    while day >= first && asked.is_none_or(|asked| day >= asked) {
        let at = days.len();
        let read = day_file(index, runs, day, &mut budget, &mut |read| {
            merge(&mut lines, at, read, |run, key| {
                run == id || key.parent.is_some()
            });
        });
        match read {
            Ok(Some(opened)) => days.push(opened),
            Ok(None) => {}
            Err(why) => unread.push(why),
        }
        if let Some(found) = lines.get(id) {
            asked = Some(day_of(found.key.queued));
        }
        day -= DAY;
    }
    index.window(runs, today);
    let Some(found) = lines.remove(id) else {
        let unread = match unread.len() {
            0 => String::new(),
            n => format!(", {n} of them not read"),
        };
        return (
            Err(format!(
                "no run {id} in the last {RUNS_DAYS} days' files{unread}"
            )),
            Vec::new(),
        );
    };
    let steps: Vec<&Found> = lines.values().collect();
    let (steps, mut changed) = lines_of(&days, &under(id, &steps));
    let page = match line_of(&days, &found) {
        Some(run) => Ok(RunPage { run, steps, unread }),
        None => {
            changed.extend(days.get(found.day).map(|day| day.path.clone()));
            Err(format!("run {id}'s line changed as it was read"))
        }
    };
    (page, changed)
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

/// The page of runs `filters` take, of the project `hub` serves, at the time its records
/// are read at ([`crate::live::Hub::recorded_now`]).
pub(crate) fn answer(
    hub: &crate::live::Hub,
    filters: &Filters,
) -> Result<RunsPage, (StatusCode, String)> {
    Ok(page(
        &hub.runs_index,
        &folder(hub)?,
        filters,
        hub.recorded_now(),
    ))
}

/// The run `id` of the project `hub` serves and its steps, at that time, or the status and why
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
    run(&hub.runs_index, &folder(hub)?, id, hub.recorded_now())
        .map_err(|why| (StatusCode::NOT_FOUND, why))
}

#[cfg(test)]
mod tests;
