//! Each call into a project's plugins timed, and under the loopback sent through a JSON round trip
//! inside the process: what a protocol's seam would cost, measured before any process boundary
//! exists (`[plugins] timing` and `[plugins] loopback`, docs/PLUGIN_CALLS.md).
//!
//! A measured plugin is the plugin behind a wrapper of the same seams: every call into a seam's
//! object (a component's render, a card kind's slot, a watch's change…) goes through its
//! meter, which times it and appends one line to the project's record ([`RECORD`], under its
//! artifacts folder). What a plugin declares (its names, markers, paths, navigation, generators
//! and theme) is read from the plugin itself: a protocol sends it once, at the start
//! ([`Declare`]), and the loopback times that once per wrapper. A wrapper is made once for each
//! plugin, record and setting, and kept for the process's life, so a seam's object it hands out
//! lives as long as the plugin's own.
//!
//! The loopback sends each call as a protocol would ([`symphony_plugin::protocol`]): the request
//! built from the call's arguments, written as JSON and read back as a [`Request`]; the call made
//! with what was read; its answer written and read back. What comes back is the answer the seam
//! gives. A value that does not survive (it fails to write or read, or reads back otherwise) is
//! named on the call's line and on stderr: a request that does not read back leaves the call
//! linked, and an answer that does not leaves the plugin's own. A page's render and a doc check's
//! reading of `docs/` stay the server's own on the plugin's side, as a protocol's plugin keeps its
//! own for one render or check.

use std::collections::BTreeSet;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use symphony_plugin::protocol::{
    Alive, AnswerGates, ApprovalNeedsAgent, Ask, AutoApprovable, CacheVersion, CardData, CardGates,
    Changed, CheckDocs, CheckProse, Chosen, Declaration, Declare, Deliver, DocComponents, Draw,
    FieldData, FileData, Forget, KeptWhile, Offer, PageData, Pictures, ProblemData, Recognises,
    RegisterAgent, Render, Request, Resolve, RunCommand, Serve, ShowRegion, Slot, Submit, TaskData,
    TriageCategory, Validate,
};
use symphony_plugin::vocabulary::{Block, FieldValue, Gate, NavItem, Picture, Region, TaskOffer};
use symphony_plugin::{
    BoardAction, BoardField, Card, CardKind, Command, Component, DocCheck, Docs, Drawing,
    FieldKind, File, Gates, Generator, HomeRegion, Page, Plugin, Problem, ProseRule, Renderer,
    Reply, Route, Submission, Submitted, TaskRef, Theme, Transport, Watch,
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

/// The file a record is appended to, one per path in the process.
struct Record {
    path: PathBuf,
    /// The file as last opened; opened again when it is gone, so a record removed starts over.
    file: Mutex<Option<std::fs::File>>,
    /// Whether a write failed and was said on stderr: said once.
    warned: AtomicBool,
}

impl Record {
    fn at(path: &Path) -> &'static Record {
        static RECORDS: LazyLock<Mutex<Vec<&'static Record>>> =
            LazyLock::new(|| Mutex::new(Vec::new()));
        let mut records = RECORDS.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(found) = records.iter().find(|record| record.path == path) {
            return found;
        }
        let record: &'static Record = Box::leak(Box::new(Record {
            path: path.to_path_buf(),
            file: Mutex::new(None),
            warned: AtomicBool::new(false),
        }));
        records.push(record);
        record
    }

    /// `line` appended, in one write.
    fn write(&self, line: &Line<'_>) {
        let Ok(mut text) = serde_json::to_string(line) else {
            return;
        };
        text.push('\n');
        let mut file = self.file.lock().unwrap_or_else(PoisonError::into_inner);
        if file.is_none() || !self.path.exists() {
            *file = self.open();
        }
        let written = match file.as_mut() {
            Some(open) => open.write_all(text.as_bytes()).map_err(|e| e.to_string()),
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

    fn open(&self) -> Option<std::fs::File> {
        if let Some(folder) = self.path.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .ok()
    }
}

/// One call's line in the record.
#[derive(Serialize)]
struct Line<'a> {
    /// When it was written, in milliseconds since the epoch.
    at_ms: u64,
    pid: u32,
    plugin: &'a str,
    seam: &'a str,
    /// The seam's object: its name, a route's path, a region's id, a watch's paths.
    name: &'a str,
    call: &'a str,
    /// The plugin's own call, in microseconds.
    us: f64,
    /// Under the loopback, the whole round trip: the request built, written and read, the call,
    /// its answer written and read.
    #[serde(skip_serializing_if = "Option::is_none")]
    round_trip_us: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    request_bytes: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    answer_bytes: Option<usize>,
    /// What did not survive JSON.
    #[serde(skip_serializing_if = "Option::is_none")]
    lost: Option<String>,
}

/// A duration in microseconds, to the nanosecond.
fn micros(duration: Duration) -> f64 {
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
        self.write::<A>(name, took, None, None, None, None);
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
        self.write::<A>(
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
        self.trip::<Declare>(plugin.name(), declared, || Declare {}, |_| declared());
    }

    fn write<A: Ask>(
        &self,
        name: &str,
        call: Duration,
        round_trip: Option<Duration>,
        request_bytes: Option<usize>,
        answer_bytes: Option<usize>,
        lost: Option<String>,
    ) {
        let (seam, call_name) = A::names();
        let at_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| {
                u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
            });
        self.record.write(&Line {
            at_ms,
            pid: std::process::id(),
            plugin: self.plugin,
            seam,
            name,
            call: call_name,
            us: micros(call),
            round_trip_us: round_trip.map(micros),
            request_bytes,
            answer_bytes,
            lost,
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
    let request: Request = ask().into();
    let written = serde_json::to_string(&request);
    let read = written
        .as_deref()
        .map_err(ToString::to_string)
        .and_then(|text| serde_json::from_str::<Request>(text).map_err(|e| e.to_string()));
    let asking = start.elapsed();
    let request_bytes = written.as_ref().map(String::len).ok();
    let read = match read {
        Ok(read) => read,
        Err(why) => return linked_trip(linked, format!("the request: {why}")),
    };
    let mut lost = (read != request).then(|| "the request read back otherwise".to_string());
    let Some(asked) = A::from_request(read) else {
        return linked_trip(linked, "the request read back as another call".into());
    };
    let start = Instant::now();
    let answer = far(asked);
    let call = start.elapsed();
    let start = Instant::now();
    let written = serde_json::to_string(&answer);
    let read = written
        .as_deref()
        .map_err(ToString::to_string)
        .and_then(|text| serde_json::from_str::<A::Answer>(text).map_err(|e| e.to_string()));
    let answering = start.elapsed();
    let answer_bytes = written.as_ref().map(String::len).ok();
    let answer = match read {
        Ok(read) => {
            if read != answer && lost.is_none() {
                lost = Some("the answer read back otherwise".into());
            }
            read
        }
        Err(why) => {
            lost = Some(format!("the answer: {why}"));
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
        run(&Page::new(&named.root, &named.doc_path))
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
            || Render {
                component: self.name.clone(),
                page: PageData::of(page),
                argument: argument.into(),
                option: option.map(str::to_string),
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
            || Resolve {
                component: self.name.clone(),
                page: PageData::of(page),
                argument: argument.into(),
                option: option.map(str::to_string),
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
            || CacheVersion {
                component: self.name.clone(),
                page: PageData::of(page),
            },
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
            || $ask {
                kind: $self.name.clone(),
                card: CardData::of($card),
            },
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
            || Pictures {
                kind: self.name.clone(),
                root: root.to_path_buf(),
                card: CardData::of(card),
            },
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
            || Draw {
                renderer: self.name.clone(),
                page: PageData::of(page),
                file: FileData::of(file),
            },
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
            || Serve {
                route: self.name.clone(),
                root: root.to_path_buf(),
                rest: rest.into(),
            },
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
            || ShowRegion {
                region: self.name.clone(),
                root: root.to_path_buf(),
            },
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
            || Offer {
                action: self.name.clone(),
                root: root.to_path_buf(),
                task: TaskData::of(task),
            },
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
            || Submit {
                action: self.name.clone(),
                root: root.to_path_buf(),
                task: TaskData::of(task),
                submission: submission.clone(),
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
        if !self.meter.loopback {
            return self
                .meter
                .time::<Validate, _>(&self.name, || self.inner.validate(value));
        }
        let validate = |value: &FieldValue| self.inner.validate(value).map(FieldData::from);
        self.meter
            .trip(
                &self.name,
                || validate(value),
                || Validate {
                    field: self.name.clone(),
                    value: value.clone().into(),
                },
                |asked| validate(&asked.value.into()),
            )
            .map(FieldValue::from)
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
            || AnswerGates {
                answerer: self.name.clone(),
                root: root.to_path_buf(),
                task: TaskData::of(task),
                tip: tip.map(str::to_string),
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
            || KeptWhile {
                answerer: self.name.clone(),
                root: root.to_path_buf(),
                task: TaskData::of(task),
            },
            |asked| self.inner.kept_while(&asked.root, &asked.task.task()),
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
            || Changed {
                watch: self.index,
                root: root.to_path_buf(),
            },
            |asked| self.inner.changed(&asked.root),
        )
    }
    fn forget(&self, root: &Path) {
        self.meter.call(
            &self.name,
            || self.inner.forget(root),
            || Forget {
                watch: self.index,
                root: root.to_path_buf(),
            },
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
            || DocComponents {
                check: self.name.clone(),
                root: docs.root().to_path_buf(),
            },
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
                || CheckDocs {
                    check: self.name.clone(),
                    root: docs.root().to_path_buf(),
                },
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
            || CheckProse {
                rule: self.name.clone(),
                rel: rel.into(),
                text: text.into(),
            },
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
            || RunCommand {
                command: self.name.clone(),
                args: args.to_vec(),
            },
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
            || RegisterAgent {
                transport: self.name.clone(),
                handle: handle.into(),
                reported: reported.into(),
            },
            |asked| self.inner.register(&asked.handle, &asked.reported),
        )
    }
    fn deliver(&self, address: &str, message: &str) -> Result<(), String> {
        self.meter.call(
            &self.name,
            || self.inner.deliver(address, message),
            || Deliver {
                transport: self.name.clone(),
                address: address.into(),
                message: message.into(),
            },
            |asked| self.inner.deliver(&asked.address, &asked.message),
        )
    }
    fn alive(&self, address: &str) -> bool {
        self.meter.call(
            &self.name,
            || self.inner.alive(address),
            || Alive {
                transport: self.name.clone(),
                address: address.into(),
            },
            |asked| self.inner.alive(&asked.address),
        )
    }
}

#[cfg(test)]
#[path = "measure_tests.rs"]
mod tests;
