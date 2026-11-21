//! Each call into a project's plugins timed, and under the loopback sent through a JSON round trip
//! inside the process: what a protocol's seam would cost, measured before any process boundary
//! exists (`[plugins] timing` and `[plugins] loopback`, docs/engineering/PLUGIN_CALLS.md).
//!
//! A measured plugin is the plugin behind a wrapper of the same seams: every call into a seam's
//! object (a component's render, a card kind's slot, a watch's change…) goes through its
//! meter, which times it and appends one line to the project's record ([`RECORD`], under its
//! artifacts folder, rolled at [`CAP`]). What a plugin declares (its names, markers, paths,
//! navigation, generators and theme) is read from the plugin itself: a protocol sends it once, at
//! the start ([`Declare`]), and the loopback times that once per wrapper. A wrapper is made once
//! for each plugin, record and setting, and kept for the process's life, so a seam's object it
//! hands out lives as long as the plugin's own.
//!
//! The loopback sends each call as a protocol would ([`symphony_plugin::protocol`]): the request
//! built from the call's arguments, written as JSON and read back as a [`Request`]; the call made
//! with what was read; its answer written as an [`Answered`] and read back. What comes back is the
//! answer the seam gives. A value that does not survive (it fails to write or read, or reads back
//! otherwise) is named on the call's line and on stderr: a request that does not read back leaves
//! the call linked, and an answer that does not leaves the plugin's own. A page's render and a doc
//! check's reading of `docs/` stay the server's own on the plugin's side, as a protocol's plugin
//! keeps its own for one render or check.

use std::collections::BTreeSet;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use symphony_plugin::authority::{Operation, Signed};
use symphony_plugin::protocol::{
    Alive, AnswerGates, Answered, ApprovalNeedsAgent, Ask, Attest, AutoApprovable, CacheVersion,
    CardData, CardGates, Changed, CheckDocs, CheckProse, Chosen, Declaration, Declare, Deliver,
    DocComponents, Draw, FileData, Forget, KeptWhile, Offer, PROTOCOL, PageData, Pictures,
    ProblemData, Recognises, RegisterAgent, Render, Request, Resolve, RunCommand, Serve,
    ShowRegion, Slot, Submit, TaskData, TriageCategory, Validate,
};
use symphony_plugin::vocabulary::{Block, FieldValue, Gate, NavItem, Picture, Region, TaskOffer};
use symphony_plugin::{
    Authority, BoardAction, BoardField, Card, CardKind, Command, Component, DocCheck, Docs,
    Drawing, FieldKind, File, Gates, Generator, HomeRegion, Page, Plugin, Problem, ProseRule,
    Renderer, Reply, Route, Submission, Submitted, TaskRef, Theme, Transport, Watch,
};

/// The record of a project's plugin calls, under its artifacts folder: one JSON line per call.
pub const RECORD: &str = "plugin-calls.jsonl";

/// `plugin` with each of its calls measured into the record at `record`: timed, and through the
/// JSON loopback when `loopback` says so. The same plugin, record and setting give the same
/// wrapper.
pub(super) fn measured(
    plugin: &'static dyn Plugin,
    record: &Path,
    loopback: bool,
) -> &'static dyn Plugin {
    struct Made {
        plugin: &'static dyn Plugin,
        record: PathBuf,
        loopback: bool,
        measured: &'static Measured,
    }
    static MADE: LazyLock<Mutex<Vec<Made>>> = LazyLock::new(|| Mutex::new(Vec::new()));
    let mut made = MADE.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(found) = made.iter().find(|made| {
        std::ptr::eq(made.plugin, plugin) && made.record == record && made.loopback == loopback
    }) {
        return found.measured;
    }
    let meter: &'static Meter = Box::leak(Box::new(Meter {
        plugin: plugin.name(),
        loopback,
        record: Record::at(record),
        said: Mutex::new(BTreeSet::new()),
    }));
    let measured: &'static Measured = Box::leak(Box::new(Measured::new(plugin, meter)));
    made.push(Made {
        plugin,
        record: record.to_path_buf(),
        loopback,
        measured,
    });
    drop(made);
    if loopback {
        meter.declared(plugin);
    }
    measured
}

/// The size at which a record is rolled: renamed to its `.1` (`plugin-calls.1.jsonl`), replacing
/// the one before, and begun again, so a record and its roll hold twice this at most.
pub const CAP: u64 = 32 * 1024 * 1024;

/// Where a record at `path` is rolled to: `plugin-calls.1.jsonl` beside `plugin-calls.jsonl`; and a
/// plugin's log, `NAME.1.log` beside `NAME.log` ([`super::process`]).
pub(super) fn rolled(path: &Path) -> PathBuf {
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    match path.extension() {
        Some(extension) => path.with_file_name(format!("{stem}.1.{}", extension.to_string_lossy())),
        None => path.with_file_name(format!("{stem}.1")),
    }
}

/// A file's device and inode: what tells the file a path names from another put in its place.
type Identity = (u64, u64);

fn identity(metadata: &std::fs::Metadata) -> Identity {
    use std::os::unix::fs::MetadataExt as _;
    (metadata.dev(), metadata.ino())
}

/// The file a record is appended to, one per path in the process.
pub(super) struct Record {
    path: PathBuf,
    /// The size past which it is rolled.
    cap: u64,
    /// The file as last opened, and which file that was. Another at the path (the one removed, or
    /// removed and made again by another process) is opened in its place, so no line goes to a
    /// file no path names.
    file: Mutex<Option<(std::fs::File, Identity)>>,
    /// Whether a write failed and was said on stderr: said once.
    warned: AtomicBool,
}

impl Record {
    /// The record at `path`, the one this process writes there.
    pub(super) fn at(path: &Path) -> &'static Record {
        static RECORDS: LazyLock<Mutex<Vec<&'static Record>>> =
            LazyLock::new(|| Mutex::new(Vec::new()));
        let mut records = RECORDS.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(found) = records.iter().find(|record| record.path == path) {
            return found;
        }
        let record: &'static Record = Box::leak(Box::new(Record::new(path, CAP)));
        records.push(record);
        record
    }

    fn new(path: &Path, cap: u64) -> Record {
        Record {
            path: path.to_path_buf(),
            cap,
            file: Mutex::new(None),
            warned: AtomicBool::new(false),
        }
    }

    /// `line` appended, in one write: to the file at the path now, rolled first when it is full.
    pub(super) fn write(&self, line: &Line<'_>) {
        let Ok(mut text) = serde_json::to_string(line) else {
            return;
        };
        text.push('\n');
        let mut file = self.file.lock().unwrap_or_else(PoisonError::into_inner);
        let now = std::fs::metadata(&self.path).ok();
        let held = file.as_ref().map(|(_, held)| *held);
        if held.is_none() || now.as_ref().map(identity) != held {
            *file = self.open();
        } else if now.is_some_and(|now| now.len() >= self.cap) {
            let _ = std::fs::rename(&self.path, rolled(&self.path));
            *file = self.open();
        }
        let written = match file.as_mut() {
            Some((open, _)) => open.write_all(text.as_bytes()).map_err(|e| e.to_string()),
            None => Err("it cannot be opened".to_string()),
        };
        if let Err(why) = written {
            *file = None;
            if !self.warned.swap(true, Ordering::SeqCst) {
                eprintln!(
                    "plugin calls: {} is not written: {why}",
                    self.path.display()
                );
            }
        }
    }

    fn open(&self) -> Option<(std::fs::File, Identity)> {
        if let Some(folder) = self.path.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .ok()?;
        let opened = identity(&file.metadata().ok()?);
        Some((file, opened))
    }
}

/// One call's line in the record.
#[derive(Serialize)]
pub(super) struct Line<'a> {
    /// When it was written, in milliseconds since the epoch.
    pub(super) at_ms: u64,
    pub(super) pid: u32,
    pub(super) plugin: &'a str,
    /// How the call went: [`LINKED`], [`LOOPBACK`] or [`PROCESS`].
    pub(super) path: &'static str,
    pub(super) seam: &'a str,
    /// The seam's object: its name, a route's path, a region's id, a watch's paths.
    pub(super) name: &'a str,
    pub(super) call: &'a str,
    /// The plugin's own call, in microseconds: under a process as the plugin says it, absent when
    /// it said nothing or the call had no answer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) us: Option<f64>,
    /// Under the loopback, the whole round trip: the request built, written and read, the call,
    /// its answer written and read. Under a process, the call as the host waits for it: its line
    /// built and written, the plugin's answer read back.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) round_trip_us: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) request_bytes: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) answer_bytes: Option<usize>,
    /// What did not survive JSON.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) lost: Option<String>,
    /// Under a process, why the call had no answer: the process down, no answer in time, a
    /// refusal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) failed: Option<String>,
}

/// A call made linked and timed.
pub const LINKED: &str = "linked";
/// A call sent through JSON inside the process.
pub const LOOPBACK: &str = "loopback";
/// A call answered by a plugin's process of its own.
pub const PROCESS: &str = "process";
/// What a loopback call's line says when its request reads back from JSON as another call. The
/// loopback's round trip alone writes it, so a binary that links no loopback holds no such text.
pub const READ_AS_ANOTHER: &str = "the request read back as another call";

/// Now, in milliseconds since the epoch.
pub(super) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        })
}

/// A duration in microseconds, to the nanosecond.
pub(super) fn micros(duration: Duration) -> f64 {
    (duration.as_secs_f64() * 1e9).round() / 1e3
}

/// What one plugin's calls are measured with.
struct Meter {
    plugin: &'static str,
    loopback: bool,
    record: &'static Record,
    /// The calls whose lost value stderr has named: each once.
    said: Mutex<BTreeSet<&'static str>>,
}

/// One call sent through the loopback: what came back, and what it cost.
struct Trip<T> {
    answer: T,
    call: Duration,
    round_trip: Option<Duration>,
    request_bytes: Option<usize>,
    answer_bytes: Option<usize>,
    lost: Option<String>,
}

impl Meter {
    /// The answer of the call `A` on the seam's object `name`: `linked` timed, or under the
    /// loopback the request `ask` builds sent to `far` and its answer back.
    fn call<A: Ask>(
        &self,
        name: &str,
        linked: impl FnOnce() -> A::Answer,
        ask: impl FnOnce() -> A,
        far: impl FnOnce(A) -> A::Answer,
    ) -> A::Answer {
        if self.loopback {
            self.trip(name, linked, ask, far)
        } else {
            self.time::<A, _>(name, linked)
        }
    }

    /// What `linked` returns, its time recorded as the call `A` on `name`.
    fn time<A: Ask, R>(&self, name: &str, linked: impl FnOnce() -> R) -> R {
        let start = Instant::now();
        let answer = linked();
        let took = start.elapsed();
        self.write(A::names(), name, took, None, None, None, None);
        answer
    }

    /// The call `A` on `name` through the loopback, recorded.
    fn trip<A: Ask>(
        &self,
        name: &str,
        linked: impl FnOnce() -> A::Answer,
        ask: impl FnOnce() -> A,
        far: impl FnOnce(A) -> A::Answer,
    ) -> A::Answer {
        let trip = round_trip(linked, ask, far);
        if let Some(lost) = &trip.lost
            && self
                .said
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(A::TAG)
        {
            eprintln!(
                "plugin calls: {} {} of {name}: {lost} (each such call is named in {})",
                self.plugin,
                A::TAG,
                self.record.path.display()
            );
        }
        self.write(
            A::names(),
            name,
            trip.call,
            trip.round_trip,
            trip.request_bytes,
            trip.answer_bytes,
            trip.lost,
        );
        trip.answer
    }

    /// What the plugin declares, sent once through the loopback and recorded.
    fn declared(&self, plugin: &'static dyn Plugin) {
        let declared = || Declaration::of(plugin);
        self.trip::<Declare>(
            plugin.name(),
            declared,
            || Declare::new(PROTOCOL),
            |_| declared(),
        );
    }

    /// The line of a call on `name`, its seam and call `names` ([`Ask::names`]): one body for every
    /// call, as only its names are the call's own.
    #[expect(
        clippy::too_many_arguments,
        reason = "a line's fields, each its caller's, handed on as they are"
    )]
    #[inline(never)]
    fn write(
        &self,
        names: (&str, &str),
        name: &str,
        call: Duration,
        round_trip: Option<Duration>,
        request_bytes: Option<usize>,
        answer_bytes: Option<usize>,
        lost: Option<String>,
    ) {
        let (seam, call_name) = names;
        self.record.write(&Line {
            at_ms: now_ms(),
            pid: std::process::id(),
            plugin: self.plugin,
            path: if self.loopback { LOOPBACK } else { LINKED },
            seam,
            name,
            call: call_name,
            us: Some(micros(call)),
            round_trip_us: round_trip.map(micros),
            request_bytes,
            answer_bytes,
            lost,
            failed: None,
        });
    }
}

/// The call `A` through JSON: the request `ask` builds written and read back, `far` called with
/// it, its answer written and read back; `linked` called instead when the request does not read
/// back as `A`. The comparisons that tell a value lost are left out of the times.
fn round_trip<A: Ask>(
    linked: impl FnOnce() -> A::Answer,
    ask: impl FnOnce() -> A,
    far: impl FnOnce(A) -> A::Answer,
) -> Trip<A::Answer> {
    let start = Instant::now();
    let (request, read, request_bytes) = read_back(ask().into());
    let asking = start.elapsed();
    let read = match read {
        Ok(read) => read,
        Err(why) => return linked_trip(linked, format!("the request: {why}")),
    };
    let mut lost = (read != request).then(|| "the request read back otherwise".to_string());
    let Some(asked) = A::from_request(read) else {
        return linked_trip(linked, READ_AS_ANOTHER.into());
    };
    let start = Instant::now();
    let answer = far(asked);
    let call = start.elapsed();
    let start = Instant::now();
    let written = serde_json::to_string(&Answered::Answer(&answer));
    let read = written
        .as_deref()
        .map_err(ToString::to_string)
        .and_then(|text| {
            serde_json::from_str::<Answered<A::Answer>>(text).map_err(|e| e.to_string())
        })
        .and_then(Answered::shown);
    let answering = start.elapsed();
    let answer_bytes = written.as_ref().map(String::len).ok();
    let answer = match read {
        Ok(read) => {
            if read != answer {
                lose(&mut lost, "the answer read back otherwise".into());
            }
            read
        }
        Err(why) => {
            lose(&mut lost, format!("the answer: {why}"));
            answer
        }
    };
    Trip {
        answer,
        call,
        round_trip: Some(asking + call + answering),
        request_bytes,
        answer_bytes,
        lost,
    }
}

/// `request` written and read back, as every call's request is ([`round_trip`]): the request, what
/// read back or why nothing did, and the written line's bytes.
#[inline(never)]
fn read_back(request: Request) -> (Request, Result<Request, String>, Option<usize>) {
    let written = serde_json::to_string(&request);
    let read = written
        .as_deref()
        .map_err(ToString::to_string)
        .and_then(|text| serde_json::from_str::<Request>(text).map_err(|e| e.to_string()));
    let request_bytes = written.as_ref().map(String::len).ok();
    (request, read, request_bytes)
}

/// `what` did not survive JSON, said after what `lost` already says: both are said when the request
/// and then the answer did not survive.
fn lose(lost: &mut Option<String>, what: String) {
    *lost = Some(match lost.take() {
        Some(before) => format!("{before}; {what}"),
        None => what,
    });
}

/// The call made linked, timed, for a request that did not survive JSON (`lost`).
fn linked_trip<T>(linked: impl FnOnce() -> T, lost: String) -> Trip<T> {
    let start = Instant::now();
    let answer = linked();
    Trip {
        answer,
        call: start.elapsed(),
        round_trip: None,
        request_bytes: None,
        answer_bytes: None,
        lost: Some(lost),
    }
}

/// `page` on the plugin's side: the server's own when `named` names it, so its components share
/// what they load for the render as they do linked; else a page of its own for what was named.
fn far_page<R>(named: &PageData, page: &Page<'_>, run: impl FnOnce(&Page<'_>) -> R) -> R {
    if named.names(page) {
        run(page)
    } else {
        run(&Page::new(&named.root, &named.doc_path).in_artifacts(&named.artifacts))
    }
}

/// A seam's object of a measured plugin: the plugin's own, its meter, and its name and place in
/// the record.
struct On<T: ?Sized + 'static> {
    inner: &'static T,
    meter: &'static Meter,
    name: String,
    /// Its place among the plugin's objects of the seam: what names a watch.
    index: usize,
}

/// A measured plugin: its seams' objects, each behind an [`On`].
struct Measured {
    plugin: &'static dyn Plugin,
    components: Vec<On<dyn Component>>,
    card_kinds: Vec<On<dyn CardKind>>,
    renderers: Vec<On<dyn Renderer>>,
    routes: Vec<On<dyn Route>>,
    home_regions: Vec<On<dyn HomeRegion>>,
    board_actions: Vec<On<dyn BoardAction>>,
    board_fields: Vec<On<dyn BoardField>>,
    gates: Vec<On<dyn Gates>>,
    authorities: Vec<On<dyn Authority>>,
    watches: Vec<On<dyn Watch>>,
    doc_checks: Vec<On<dyn DocCheck>>,
    prose_rules: Vec<On<dyn ProseRule>>,
    commands: Vec<On<dyn Command>>,
    transports: Vec<On<dyn Transport>>,
}

impl Measured {
    fn new(plugin: &'static dyn Plugin, meter: &'static Meter) -> Measured {
        fn each<T: ?Sized + 'static>(
            objects: Vec<&'static T>,
            meter: &'static Meter,
            name: impl Fn(&T) -> String,
        ) -> Vec<On<T>> {
            objects
                .into_iter()
                .enumerate()
                .map(|(index, inner)| On {
                    inner,
                    meter,
                    name: name(inner),
                    index,
                })
                .collect()
        }
        Measured {
            plugin,
            components: each(plugin.components(), meter, |c| c.name().into()),
            card_kinds: each(plugin.card_kinds(), meter, |k| k.name().into()),
            renderers: each(plugin.renderers(), meter, |r| r.name().into()),
            routes: each(plugin.routes(), meter, |r| r.path().into()),
            home_regions: each(plugin.home_regions(), meter, |r| r.id().into()),
            board_actions: each(plugin.board_actions(), meter, |a| a.name().into()),
            board_fields: each(plugin.board_fields(), meter, |f| f.name().into()),
            gates: each(plugin.gates(), meter, |g| g.name().into()),
            authorities: each(plugin.authorities(), meter, |a| a.adapter().into()),
            watches: each(plugin.watches(), meter, |w| w.paths().join(", ")),
            doc_checks: each(plugin.doc_checks(), meter, |c| c.name().into()),
            prose_rules: each(plugin.prose_rules(), meter, |r| r.name().into()),
            commands: each(plugin.commands(), meter, |c| c.name().into()),
            transports: each(plugin.transports(), meter, |t| t.name().into()),
        }
    }
}

/// Each object of a seam, as the seam's trait hands it out.
macro_rules! objects {
    ($list:expr, $trait:ident) => {
        $list
            .iter()
            .map(|object| -> &dyn $trait { object })
            .collect()
    };
}

impl Plugin for Measured {
    fn name(&self) -> &str {
        self.plugin.name()
    }
    fn components(&self) -> Vec<&dyn Component> {
        objects!(self.components, Component)
    }
    fn card_kinds(&self) -> Vec<&dyn CardKind> {
        objects!(self.card_kinds, CardKind)
    }
    fn renderers(&self) -> Vec<&dyn Renderer> {
        objects!(self.renderers, Renderer)
    }
    fn routes(&self) -> Vec<&dyn Route> {
        objects!(self.routes, Route)
    }
    fn nav(&self) -> Vec<NavItem> {
        self.plugin.nav()
    }
    fn home_regions(&self) -> Vec<&dyn HomeRegion> {
        objects!(self.home_regions, HomeRegion)
    }
    fn board_actions(&self) -> Vec<&dyn BoardAction> {
        objects!(self.board_actions, BoardAction)
    }
    fn board_fields(&self) -> Vec<&dyn BoardField> {
        objects!(self.board_fields, BoardField)
    }
    fn gates(&self) -> Vec<&dyn Gates> {
        objects!(self.gates, Gates)
    }
    fn authorities(&self) -> Vec<&dyn Authority> {
        objects!(self.authorities, Authority)
    }
    fn watches(&self) -> Vec<&dyn Watch> {
        objects!(self.watches, Watch)
    }
    fn doc_checks(&self) -> Vec<&dyn DocCheck> {
        objects!(self.doc_checks, DocCheck)
    }
    fn prose_rules(&self) -> Vec<&dyn ProseRule> {
        objects!(self.prose_rules, ProseRule)
    }
    fn generators(&self) -> Vec<&dyn Generator> {
        self.plugin.generators()
    }
    fn commands(&self) -> Vec<&dyn Command> {
        objects!(self.commands, Command)
    }
    fn transports(&self) -> Vec<&dyn Transport> {
        objects!(self.transports, Transport)
    }
    fn theme(&self) -> Option<Theme> {
        self.plugin.theme()
    }
}

impl Component for On<dyn Component> {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn render(
        &self,
        page: &Page<'_>,
        argument: &str,
        option: Option<&str>,
    ) -> Result<Vec<Block>, String> {
        self.meter.call(
            &self.name,
            || self.inner.render(page, argument, option),
            || {
                Render::new(
                    self.name.clone(),
                    PageData::of(page),
                    argument.into(),
                    option.map(str::to_string),
                )
            },
            |asked| {
                far_page(&asked.page, page, |page| {
                    self.inner
                        .render(page, &asked.argument, asked.option.as_deref())
                })
            },
        )
    }

    fn resolve(&self, page: &Page<'_>, argument: &str, option: Option<&str>) -> Result<(), String> {
        self.meter.call(
            &self.name,
            || self.inner.resolve(page, argument, option),
            || {
                Resolve::new(
                    self.name.clone(),
                    PageData::of(page),
                    argument.into(),
                    option.map(str::to_string),
                )
            },
            |asked| {
                far_page(&asked.page, page, |page| {
                    self.inner
                        .resolve(page, &asked.argument, asked.option.as_deref())
                })
            },
        )
    }

    fn cache_version(&self, page: &Page<'_>) -> Option<String> {
        self.meter.call(
            &self.name,
            || self.inner.cache_version(page),
            || CacheVersion::new(self.name.clone(), PageData::of(page)),
            |asked| far_page(&asked.page, page, |page| self.inner.cache_version(page)),
        )
    }
}

/// A card kind's call that reads the card alone: `$call` with the request `$ask`.
macro_rules! card_call {
    ($self:ident, $card:ident, $ask:ident, $call:ident) => {
        $self.meter.call(
            &$self.name,
            || $self.inner.$call($card),
            || $ask::new($self.name.clone(), CardData::of($card)),
            |asked| asked.card.read(|card| $self.inner.$call(card)),
        )
    };
}

impl CardKind for On<dyn CardKind> {
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn recognises(&self, card: &Card<'_>) -> bool {
        card_call!(self, card, Recognises, recognises)
    }
    fn pictures(&self, root: &Path, card: &Card<'_>) -> Vec<Picture> {
        self.meter.call(
            &self.name,
            || self.inner.pictures(root, card),
            || Pictures::new(self.name.clone(), root.to_path_buf(), CardData::of(card)),
            |asked| {
                asked
                    .card
                    .read(|card| self.inner.pictures(&asked.root, card))
            },
        )
    }
    fn slot(&self, card: &Card<'_>) -> Option<String> {
        card_call!(self, card, Slot, slot)
    }
    fn markers(&self) -> &[&str] {
        self.inner.markers()
    }
    fn triage_category(&self, card: &Card<'_>) -> Option<String> {
        card_call!(self, card, TriageCategory, triage_category)
    }
    fn auto_approvable(&self, card: &Card<'_>) -> bool {
        card_call!(self, card, AutoApprovable, auto_approvable)
    }
    fn gates(&self, card: &Card<'_>) -> Option<(usize, String)> {
        card_call!(self, card, CardGates, gates)
    }
    fn approval_needs_agent(&self, card: &Card<'_>) -> bool {
        card_call!(self, card, ApprovalNeedsAgent, approval_needs_agent)
    }
    fn chosen(&self, card: &Card<'_>) -> Result<Option<String>, String> {
        card_call!(self, card, Chosen, chosen)
    }
}

impl Renderer for On<dyn Renderer> {
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn extensions(&self) -> &[&str] {
        self.inner.extensions()
    }
    fn content_type(&self) -> &str {
        self.inner.content_type()
    }
    fn draw(&self, page: &Page<'_>, file: &File<'_>) -> Result<Drawing, String> {
        self.meter.call(
            &self.name,
            || self.inner.draw(page, file),
            || Draw::new(self.name.clone(), PageData::of(page), FileData::of(file)),
            |asked| {
                far_page(&asked.page, page, |page| {
                    self.inner.draw(page, &asked.file.file())
                })
            },
        )
    }
}

impl Route for On<dyn Route> {
    fn path(&self) -> &str {
        self.inner.path()
    }
    fn serve(&self, root: &Path, rest: &str) -> Option<Reply> {
        self.meter.call(
            &self.name,
            || self.inner.serve(root, rest),
            || Serve::new(self.name.clone(), root.to_path_buf(), rest.into()),
            |asked| self.inner.serve(&asked.root, &asked.rest),
        )
    }
}

impl HomeRegion for On<dyn HomeRegion> {
    fn id(&self) -> &str {
        self.inner.id()
    }
    fn region(&self, root: &Path) -> Option<Region> {
        self.meter.call(
            &self.name,
            || self.inner.region(root),
            || ShowRegion::new(self.name.clone(), root.to_path_buf()),
            |asked| self.inner.region(&asked.root),
        )
    }
}

impl BoardAction for On<dyn BoardAction> {
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn offer(&self, root: &Path, task: &TaskRef<'_>) -> Result<TaskOffer, String> {
        self.meter.call(
            &self.name,
            || self.inner.offer(root, task),
            || Offer::new(self.name.clone(), root.to_path_buf(), TaskData::of(task)),
            |asked| self.inner.offer(&asked.root, &asked.task.task()),
        )
    }
    fn submit(
        &self,
        root: &Path,
        task: &TaskRef<'_>,
        submission: &Submission,
    ) -> Result<Submitted, String> {
        self.meter.call(
            &self.name,
            || self.inner.submit(root, task, submission),
            || {
                Submit::new(
                    self.name.clone(),
                    root.to_path_buf(),
                    TaskData::of(task),
                    submission.clone(),
                )
            },
            |asked| {
                self.inner
                    .submit(&asked.root, &asked.task.task(), &asked.submission)
            },
        )
    }
}

impl BoardField for On<dyn BoardField> {
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn kind(&self) -> FieldKind {
        self.inner.kind()
    }
    fn required(&self) -> bool {
        self.inner.required()
    }
    fn validate(&self, value: &FieldValue) -> Result<FieldValue, String> {
        self.meter.call(
            &self.name,
            || self.inner.validate(value),
            || Validate::new(self.name.clone(), value.clone()),
            |asked| self.inner.validate(&asked.value),
        )
    }
}

impl Gates for On<dyn Gates> {
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn answer(
        &self,
        root: &Path,
        task: &TaskRef<'_>,
        tip: Option<&str>,
    ) -> Result<Vec<Gate>, String> {
        self.meter.call(
            &self.name,
            || self.inner.answer(root, task, tip),
            || {
                AnswerGates::new(
                    self.name.clone(),
                    root.to_path_buf(),
                    TaskData::of(task),
                    tip.map(str::to_string),
                )
            },
            |asked| {
                self.inner
                    .answer(&asked.root, &asked.task.task(), asked.tip.as_deref())
            },
        )
    }
    fn kept_while(&self, root: &Path, task: &TaskRef<'_>) -> String {
        self.meter.call(
            &self.name,
            || self.inner.kept_while(root, task),
            || KeptWhile::new(self.name.clone(), root.to_path_buf(), TaskData::of(task)),
            |asked| self.inner.kept_while(&asked.root, &asked.task.task()),
        )
    }
}

impl Authority for On<dyn Authority> {
    fn adapter(&self) -> &str {
        self.inner.adapter()
    }
    fn attest(
        &self,
        root: &Path,
        operation: &Operation,
        within: Duration,
    ) -> Result<Signed, String> {
        self.meter.call(
            &self.name,
            || self.inner.attest(root, operation, within),
            || {
                Attest::new(
                    self.name.clone(),
                    root.to_path_buf(),
                    operation.clone(),
                    u64::try_from(within.as_millis()).unwrap_or(u64::MAX),
                )
            },
            |asked| {
                let within = Duration::from_millis(asked.within_ms);
                self.inner.attest(&asked.root, &asked.operation, within)
            },
        )
    }
}

impl Watch for On<dyn Watch> {
    fn paths(&self) -> Vec<String> {
        self.inner.paths()
    }
    fn changed(&self, root: &Path) -> Option<String> {
        self.meter.call(
            &self.name,
            || self.inner.changed(root),
            || Changed::new(self.index, root.to_path_buf()),
            |asked| self.inner.changed(&asked.root),
        )
    }
    fn forget(&self, root: &Path) {
        self.meter.call(
            &self.name,
            || self.inner.forget(root),
            || Forget::new(self.index, root.to_path_buf()),
            |asked| self.inner.forget(&asked.root),
        );
    }
}

impl DocCheck for On<dyn DocCheck> {
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn components(&self, docs: &Docs) -> Vec<String> {
        self.meter.call(
            &self.name,
            || self.inner.components(docs),
            || DocComponents::new(self.name.clone(), docs.root().to_path_buf()),
            |asked| far_docs(&asked.root, docs, |docs| self.inner.components(docs)),
        )
    }
    fn inputs(&self) -> Vec<String> {
        self.inner.inputs()
    }
    fn check(&self, docs: &Docs) -> Vec<Problem> {
        if !self.meter.loopback {
            return self
                .meter
                .time::<CheckDocs, _>(&self.name, || self.inner.check(docs));
        }
        let problems = |docs: &Docs| -> Vec<ProblemData> {
            self.inner.check(docs).iter().map(ProblemData::of).collect()
        };
        self.meter
            .trip(
                &self.name,
                || problems(docs),
                || CheckDocs::new(self.name.clone(), docs.root().to_path_buf()),
                |asked| far_docs(&asked.root, docs, problems),
            )
            .into_iter()
            .map(ProblemData::problem)
            .collect()
    }
}

/// `docs` on the plugin's side: the server's reading when `root` is its root, as a protocol's
/// plugin reads its own once for a check; else a reading of `root`.
fn far_docs<R>(root: &Path, docs: &Docs, run: impl FnOnce(&Docs) -> R) -> R {
    if root == docs.root() {
        run(docs)
    } else {
        run(&Docs::read(root))
    }
}

impl ProseRule for On<dyn ProseRule> {
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn check(&self, rel: &str, text: &str) -> Vec<(usize, String)> {
        self.meter.call(
            &self.name,
            || self.inner.check(rel, text),
            || CheckProse::new(self.name.clone(), rel.into(), text.into()),
            |asked| self.inner.check(&asked.rel, &asked.text),
        )
    }
}

impl Command for On<dyn Command> {
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn summary(&self) -> &str {
        self.inner.summary()
    }
    fn run(
        &self,
        args: &[String],
        out: &mut dyn std::io::Write,
        err: &mut dyn std::io::Write,
    ) -> i32 {
        let streams = std::cell::RefCell::new((out, err));
        let run = |args: &[String]| {
            let (out, err) = &mut *streams.borrow_mut();
            self.inner.run(args, &mut **out, &mut **err)
        };
        self.meter.call(
            &self.name,
            || run(args),
            || RunCommand::new(self.name.clone(), args.to_vec()),
            |asked| run(&asked.args),
        )
    }
}

impl Transport for On<dyn Transport> {
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn register(&self, handle: &str, reported: &str) -> Result<String, String> {
        self.meter.call(
            &self.name,
            || self.inner.register(handle, reported),
            || RegisterAgent::new(self.name.clone(), handle.into(), reported.into()),
            |asked| self.inner.register(&asked.handle, &asked.reported),
        )
    }
    fn deliver(&self, address: &str, message: &str) -> Result<(), String> {
        self.meter.call(
            &self.name,
            || self.inner.deliver(address, message),
            || Deliver::new(self.name.clone(), address.into(), message.into()),
            |asked| self.inner.deliver(&asked.address, &asked.message),
        )
    }
    fn alive(&self, address: &str) -> bool {
        self.meter.call(
            &self.name,
            || self.inner.alive(address),
            || Alive::new(self.name.clone(), address.into()),
            |asked| self.inner.alive(&asked.address),
        )
    }
}

#[cfg(test)]
#[path = "measure_tests.rs"]
mod tests;
