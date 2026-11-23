//! Each call into a project's plugins timed into its record (`[plugins] timing`,
//! docs/engineering/PLUGIN_CALLS.md).
//!
//! A measured plugin is the plugin behind a wrapper of the same seams: every call into a seam's
//! object (a component's render, a card kind's slot, a watch's change…) goes through its
//! meter, which times it and appends one line to the project's record ([`RECORD`], under its
//! artifacts folder, rolled at [`CAP`]). Each line names its call as the protocol's request for it
//! does ([`Ask::names`]), so a linked plugin's lines and a process's read alike. What a plugin
//! declares (its names, markers, paths, navigation, generators and theme) is read from the plugin
//! itself and not timed. A wrapper is made once for each plugin and record, and kept for the
//! process's life, so a seam's object it hands out lives as long as the plugin's own.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use symphony_board::lock::Recovered as _;
use symphony_plugin::authority::{Operation, Signed};
use symphony_plugin::protocol::{
    Alive, AnswerGates, ApprovalNeedsAgent, Ask, Attest, AutoApprovable, CacheVersion, CardGates,
    Changed, CheckDocs, CheckProse, Chosen, Deliver, Describe, DocComponents, Draw, Forget,
    KeptWhile, Offer, Pictures, Recognises, RegisterAgent, Render, Resolve, RunCommand, Serve,
    ShowRegion, Slot, Submit, TriageCategory, Validate,
};
use symphony_plugin::vocabulary::{
    Block, FieldValue, Gate, NavItem, Picture, Region, SettingsEntry, TaskOffer,
};
use symphony_plugin::{
    Authority, BoardAction, BoardField, Card, CardKind, Command, Component, Describer, DocCheck,
    Docs, Drawing, FieldKind, File, Gates, Generator, HomeRegion, Page, Plugin, Problem, ProseRule,
    Renderer, Reply, Route, Sender, Submission, Submitted, TaskRef, Theme, Transport, Watch,
};

/// The record of a project's plugin calls, under its artifacts folder: one JSON line per call.
pub const RECORD: &str = "plugin-calls.jsonl";

/// `plugin` with each of its calls timed into the record at `record`. The same plugin and record
/// give the same wrapper.
pub(super) fn measured(plugin: &'static dyn Plugin, record: &Path) -> &'static dyn Plugin {
    struct Made {
        plugin: &'static dyn Plugin,
        record: PathBuf,
        measured: &'static Measured,
    }
    static MADE: LazyLock<Mutex<Vec<Made>>> = LazyLock::new(|| Mutex::new(Vec::new()));
    let mut made = MADE.lock().recover();
    if let Some(found) = made
        .iter()
        .find(|made| std::ptr::eq(made.plugin, plugin) && made.record == record)
    {
        return found.measured;
    }
    let meter: &'static Meter = Box::leak(Box::new(Meter {
        plugin: plugin.name(),
        record: Record::at(record),
    }));
    let measured: &'static Measured = Box::leak(Box::new(Measured::new(plugin, meter)));
    made.push(Made {
        plugin,
        record: record.to_path_buf(),
        measured,
    });
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
        let mut records = RECORDS.lock().recover();
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
        let mut file = self.file.lock().recover();
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
    /// How the call went: [`LINKED`] or [`PROCESS`].
    pub(super) path: &'static str,
    pub(super) seam: &'a str,
    /// The seam's object: its name, a route's path, a region's id, a watch's paths.
    pub(super) name: &'a str,
    pub(super) call: &'a str,
    /// The plugin's own call, in microseconds: under a process as the plugin says it, absent when
    /// it said nothing or the call had no answer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) us: Option<f64>,
    /// Under a process, the call as the host waits for it: its line built and written, the
    /// plugin's answer read back.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) round_trip_us: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) request_bytes: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) answer_bytes: Option<usize>,
    /// Under a process, why the call had no answer: the process down, no answer in time, a
    /// refusal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) failed: Option<String>,
}

/// A call made linked and timed.
pub const LINKED: &str = "linked";
/// A call answered by a plugin's process of its own.
pub const PROCESS: &str = "process";

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

/// What one plugin's calls are timed into.
struct Meter {
    plugin: &'static str,
    record: &'static Record,
}

impl Meter {
    /// What `linked` returns, its time recorded as the call `A` on the seam's object `name`.
    fn time<A: Ask, R>(&self, name: &str, linked: impl FnOnce() -> R) -> R {
        let start = Instant::now();
        let answer = linked();
        let took = start.elapsed();
        self.write(A::names(), name, took);
        answer
    }

    /// The line of a call on `name`, its seam and call `names` ([`Ask::names`]): one body for every
    /// call, as only its names are the call's own.
    #[inline(never)]
    fn write(&self, names: (&str, &str), name: &str, took: Duration) {
        let (seam, call) = names;
        self.record.write(&Line {
            at_ms: now_ms(),
            pid: std::process::id(),
            plugin: self.plugin,
            path: LINKED,
            seam,
            name,
            call,
            us: Some(micros(took)),
            round_trip_us: None,
            request_bytes: None,
            answer_bytes: None,
            failed: None,
        });
    }
}

/// A seam's object of a measured plugin: the plugin's own, its meter, and its name in the record.
struct On<T: ?Sized + 'static> {
    inner: &'static T,
    meter: &'static Meter,
    name: String,
}

impl<T: ?Sized + 'static> On<T> {
    /// What `call` returns of the plugin's own object, timed as the call `A`, whose answer it is:
    /// a call timed as another's, whose answer differs, does not compile.
    fn time<A: Ask>(&self, call: impl FnOnce(&'static T) -> A::Answer) -> A::Answer {
        self.time_as::<A, A::Answer>(call)
    }

    /// The same for a call whose method returns other than `A`'s answer: a doc check's problems,
    /// which [`CheckDocs`] answers as `ProblemData`.
    fn time_as<A: Ask, R>(&self, call: impl FnOnce(&'static T) -> R) -> R {
        self.meter.time::<A, R>(&self.name, || call(self.inner))
    }
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
    describers: Vec<On<dyn Describer>>,
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
                .map(|inner| On {
                    inner,
                    meter,
                    name: name(inner),
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
            describers: each(plugin.describers(), meter, |d| d.id().into()),
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
    fn describers(&self) -> Vec<&dyn Describer> {
        objects!(self.describers, Describer)
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
        self.time::<Render>(|c| c.render(page, argument, option))
    }

    fn resolve(&self, page: &Page<'_>, argument: &str, option: Option<&str>) -> Result<(), String> {
        self.time::<Resolve>(|c| c.resolve(page, argument, option))
    }

    fn cache_version(&self, page: &Page<'_>) -> Option<String> {
        self.time::<CacheVersion>(|c| c.cache_version(page))
    }
}

impl CardKind for On<dyn CardKind> {
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn recognises(&self, card: &Card<'_>) -> bool {
        self.time::<Recognises>(|k| k.recognises(card))
    }
    fn pictures(&self, root: &Path, card: &Card<'_>) -> Vec<Picture> {
        self.time::<Pictures>(|k| k.pictures(root, card))
    }
    fn slot(&self, card: &Card<'_>) -> Option<String> {
        self.time::<Slot>(|k| k.slot(card))
    }
    fn markers(&self) -> &[&str] {
        self.inner.markers()
    }
    fn triage_category(&self, card: &Card<'_>) -> Option<String> {
        self.time::<TriageCategory>(|k| k.triage_category(card))
    }
    fn auto_approvable(&self, card: &Card<'_>) -> bool {
        self.time::<AutoApprovable>(|k| k.auto_approvable(card))
    }
    fn gates(&self, card: &Card<'_>) -> Option<(usize, String)> {
        self.time::<CardGates>(|k| k.gates(card))
    }
    fn approval_needs_agent(&self, card: &Card<'_>) -> bool {
        self.time::<ApprovalNeedsAgent>(|k| k.approval_needs_agent(card))
    }
    fn chosen(&self, card: &Card<'_>) -> Result<Option<String>, String> {
        self.time::<Chosen>(|k| k.chosen(card))
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
        self.time::<Draw>(|r| r.draw(page, file))
    }
}

impl Route for On<dyn Route> {
    fn path(&self) -> &str {
        self.inner.path()
    }
    fn serve(&self, root: &Path, rest: &str) -> Option<Reply> {
        self.time::<Serve>(|r| r.serve(root, rest))
    }
}

impl HomeRegion for On<dyn HomeRegion> {
    fn id(&self) -> &str {
        self.inner.id()
    }
    fn region(&self, root: &Path) -> Option<Region> {
        self.time::<ShowRegion>(|r| r.region(root))
    }
}

impl BoardAction for On<dyn BoardAction> {
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn offer(&self, root: &Path, task: &TaskRef<'_>) -> Result<TaskOffer, String> {
        self.time::<Offer>(|a| a.offer(root, task))
    }
    fn submit(
        &self,
        root: &Path,
        task: &TaskRef<'_>,
        submission: &Submission,
    ) -> Result<Submitted, String> {
        self.time::<Submit>(|a| a.submit(root, task, submission))
    }
    fn submit_from(
        &self,
        root: &Path,
        task: &TaskRef<'_>,
        submission: &Submission,
        sender: &Sender,
    ) -> Result<Submitted, String> {
        self.time::<Submit>(|a| a.submit_from(root, task, submission, sender))
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
        self.time::<Validate>(|f| f.validate(value))
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
        self.time::<AnswerGates>(|g| g.answer(root, task, tip))
    }
    fn kept_while(&self, root: &Path, task: &TaskRef<'_>) -> String {
        self.time::<KeptWhile>(|g| g.kept_while(root, task))
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
        self.time::<Attest>(|a| a.attest(root, operation, within))
    }
}

impl Describer for On<dyn Describer> {
    fn id(&self) -> &str {
        self.inner.id()
    }
    fn describe(&self, root: &Path) -> Result<SettingsEntry, String> {
        self.time::<Describe>(|d| d.describe(root))
    }
}

impl Watch for On<dyn Watch> {
    fn paths(&self) -> Vec<String> {
        self.inner.paths()
    }
    fn changed(&self, root: &Path) -> Option<String> {
        self.time::<Changed>(|w| w.changed(root))
    }
    fn forget(&self, root: &Path) {
        self.time::<Forget>(|w| w.forget(root));
    }
}

impl DocCheck for On<dyn DocCheck> {
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn components(&self, docs: &Docs) -> Vec<String> {
        self.time::<DocComponents>(|c| c.components(docs))
    }
    fn inputs(&self) -> Vec<String> {
        self.inner.inputs()
    }
    fn check(&self, docs: &Docs) -> Vec<Problem> {
        self.time_as::<CheckDocs, _>(|c| c.check(docs))
    }
}

impl ProseRule for On<dyn ProseRule> {
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn check(&self, rel: &str, text: &str) -> Vec<(usize, String)> {
        self.time::<CheckProse>(|r| r.check(rel, text))
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
        self.time::<RunCommand>(|c| c.run(args, out, err))
    }
}

impl Transport for On<dyn Transport> {
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn register(&self, handle: &str, reported: &str) -> Result<String, String> {
        self.time::<RegisterAgent>(|t| t.register(handle, reported))
    }
    fn deliver(&self, address: &str, message: &str) -> Result<(), String> {
        self.time::<Deliver>(|t| t.deliver(address, message))
    }
    fn alive(&self, address: &str) -> bool {
        self.time::<Alive>(|t| t.alive(address))
    }
}

#[cfg(test)]
#[path = "measure_tests.rs"]
mod tests;
