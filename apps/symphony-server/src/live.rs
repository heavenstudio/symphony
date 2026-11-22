//! One filesystem watcher and board reader for all open clients. Changes are retained briefly so a
//! board GET and its following feed subscribe cannot lose an edit between them. Slow clients heal
//! through a new snapshot instead of silently dropping events.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, LazyLock, Mutex, OnceLock, RwLock, Weak, mpsc};
use std::time::{Duration, Instant};

use crate::backstop::{self, Backstop, Sweep, Wake, Waker, Wakers};
use axum::http::StatusCode;
use notify::{RecursiveMode, Watcher};
use serde_json::{Value, json};
use symphony::{branches, dashboard, taskview};
use symphony_api::{Activity, Board, Pending, Queue, Tasks};
use tokio::sync::watch;

const HISTORY: usize = 128;
const BOARD_INTERVAL: Duration = Duration::from_secs(1);

/// `run`, timed under `SYMPHONY_PROFILE`: mark its start, then report wall and process CPU
/// time as it returns. A start without a timing line identifies a still-pending stage.
pub(crate) fn measured<T>(name: &str, run: impl FnOnce() -> T) -> T {
    let stage = Stage::begin();
    if stage.is_some() {
        eprintln!("· review timing start: {name}");
    }
    let result = run();
    if let Some(stage) = stage {
        eprintln!("{}", stage.line(name));
    }
    result
}

/// When a timed stage began, on the wall clock and in process CPU time.
struct Stage {
    began: Instant,
    cpu: Option<f64>,
}

impl Stage {
    /// Now, when `SYMPHONY_PROFILE` is set; ordinary serving reads no clock.
    fn begin() -> Option<Stage> {
        static PROFILE: LazyLock<bool> = LazyLock::new(|| symphony_env::PROFILE.os().is_some());
        PROFILE.then(Stage::now)
    }

    fn now() -> Stage {
        Stage {
            began: Instant::now(),
            cpu: cpu_millis(),
        }
    }

    /// The stage `name`'s line, ended now.
    fn line(&self, name: &str) -> String {
        let cpu = self
            .cpu
            .and_then(|before| cpu_millis().map(|after| after - before))
            .map_or_else(|| "unavailable".to_string(), |cpu| format!("{cpu:.3}"));
        format!(
            "· review timing: {name}: {:.3} ms cpu {cpu} ms",
            self.began.elapsed().as_secs_f64() * 1000.0
        )
    }
}

fn cpu_millis() -> Option<f64> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // SAFETY: getrusage initializes this owned record on success, before it is read.
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: the successful call initialized the whole rusage record.
    let usage = unsafe { usage.assume_init() };
    let time = |tv: libc::timeval| -> Option<Duration> {
        Some(
            Duration::from_secs(u64::try_from(tv.tv_sec).ok()?)
                + Duration::from_micros(u64::try_from(tv.tv_usec).ok()?),
        )
    };
    Some((time(usage.ru_utime)? + time(usage.ru_stime)?).as_secs_f64() * 1000.0)
}

#[derive(Clone)]
pub(crate) struct Event {
    pub version: u64,
    pub json: String,
}

pub(crate) struct Page {
    pub board: Board,
    pub queue: Queue,
    pub pending: Pending,
    pub activity: Activity,
    order: Arc<symphony::postpone::View>,
    pub history: VecDeque<Event>,
    pub documents: BTreeMap<String, symphony_api::Document>,
    /// The read ([`Hub::reads`]) each document's copy in `documents`, or its removal, came from: a
    /// render begun before it places nothing.
    copied: BTreeMap<String, u64>,
    /// Where the last refresh that renders anew (its files changed, or main's one cards moved)
    /// or board read whose tasks moved ([`Hub::render_relinked`]) began, in [`Hub::reads`]'
    /// numbers: a first read of a document begun before it reads again, as each renders only the
    /// documents followed when it listed them.
    renews_from: u64,
    /// Documents a first read placed though a refresh that renders anew, or a board read whose
    /// tasks moved, began after it, as it had read again once already: the next refresh renders
    /// them whatever changed.
    stale: BTreeSet<String>,
    /// The tag of each document's outline as its copy in `documents` makes it, with the read that
    /// copy came from ([`Hub::outline_kept`]): made at the first check of each copy, and `None`
    /// for a copy whose check renders.
    outlines: Mutex<BTreeMap<String, (u64, Option<String>)>>,
    missing: BTreeSet<String>,
    /// When the documents its queue came from began to be read, in seconds since the epoch: an
    /// owner's ask said after it is not in it yet ([`symphony::owner_asks`]).
    read_at: f64,
    /// Every card the page held when it was read ([`dashboard::Snapshot::held`]).
    held: Arc<dashboard::Held>,
    /// The owner's lines under those cards ([`dashboard::Snapshot::said`]).
    said: Arc<symphony::decided::Said>,
    /// The branches whose worktree the scan that page read could not read
    /// ([`branches::Scan::failed`]): where their cards are, `held` does not say.
    unscanned: Arc<BTreeSet<String>>,
}

pub(crate) struct Hub {
    pub root: PathBuf,
    ui_dist: PathBuf,
    embedded_ui: bool,
    pub pages: RwLock<BTreeMap<String, Page>>,
    pub changed: watch::Sender<u64>,
    pub draining: watch::Sender<bool>,
    pub server_notice: watch::Sender<String>,
    initial_board: watch::Sender<bool>,
    initial_start: Mutex<Option<mpsc::Sender<()>>>,
    invalid: RwLock<BTreeMap<String, String>>,
    trees: RwLock<BTreeMap<String, PathBuf>>,
    tasks: Mutex<TaskState>,
    /// What [`Hub::tasks_read`] answers: the board's last read, or why there is none, set where
    /// `tasks` is, under it, and read without it. A board read holds `tasks` across its reads of
    /// the board (`update_board`), so a slow board holds it for as long as it is slow; a reader of
    /// this waits on none of that, and reads what `tasks` held before that read began.
    listing: RwLock<Result<Arc<Tasks>, String>>,
    agents: Mutex<Option<symphony_api::Agents>>,
    machine_work: Mutex<machine_work::Sampler>,
    latest_message: Mutex<Option<i64>>,
    pub task_signoff: Mutex<Vec<String>>,
    /// The custom workflow's active generation as the last board read found it: none before a
    /// read, `Some(None)` on a board that never activated one. A new one is said on the feed
    /// (`workflow`), so a page reads the definition again: a label changed moves no task.
    workflow_generation: Mutex<Option<Option<u64>>>,
    /// What the board's snapshot reads beside the board ([`Waiting`]).
    waiting: Mutex<Waiting>,
    /// The last scan of the branches' own cards ([`BranchScan`]).
    branches: Mutex<BranchScan>,
    /// The branches whose task the board last said is done or cancelled ([`branches::finished`]):
    /// they post no card.
    finished: Mutex<BTreeSet<String>>,
    /// Set when `finished` changed, so the loop scans the branches again without a file event
    /// ([`Hub::ask_rescan`]).
    rescan: std::sync::atomic::AtomicBool,
    /// What wakes the documents' loop ([`documents`]): a rescan asked for, a picture a held card
    /// waits on landed. Set as the hub starts watching; a hub that never does has none.
    documents_waker: OnceLock<Waker>,
    /// What the hub wakes as it finds or loses its board ([`Hub::has_board`]): its server's
    /// maintenance, given as the hub starts ([`Hub::wake_at_board`]).
    board_wakers: OnceLock<Arc<Wakers>>,
    /// What the documents' loop calls after each refresh ([`Hub::after_refresh`]).
    refreshed: OnceLock<Box<dyn Fn() + Send + Sync>>,
    /// Whether `trees` is what git last listed: unset before the first listing and after one that
    /// failed, when the owner's asks on a branch wait ([`symphony::owner_asks::Reading::listed`]).
    listed: std::sync::atomic::AtomicBool,
    /// The one card each page shows in place of its file's copy, from main's last snapshot
    /// ([`dashboard::Snapshot::pages`]).
    one_cards: RwLock<Arc<dashboard::Pages>>,
    /// By branch page, what it draws otherwise than its file, from the last scan
    /// ([`branches::Scan::pages`]).
    marks: RwLock<Arc<BTreeMap<String, branches::PageMarks>>>,
    /// The missing pictures the cards the last scan held back wait on ([`branches::Scan::awaited`]):
    /// the loop scans again once one is on disk.
    awaited: Mutex<Vec<PathBuf>>,
    watch_changes: Mutex<BTreeSet<PathBuf>>,
    warmed_watches: Mutex<BTreeMap<PathBuf, Sweep>>,
    callbacks: Mutex<Option<symphony::callbacks::Flusher>>,
    /// The shared server's part in the owner's asks; none on a private server, which leaves them alone.
    owner_asks: Mutex<Option<owner_asks::Kept>>,
    /// The shared server's part in the blocks the owner's word ends ([`symphony::decided`]); none on a
    /// private server, which leaves them alone.
    decided: Mutex<Option<symphony::decided::Kept>>,
    /// The owner's open asks as the board last gave them, which the agent queue's cards are told
    /// ([`Hub::note_asks`]); none until the board is read.
    queue_asks: Mutex<Arc<Vec<symphony_board::Ask>>>,
    /// The shared server's hourly roll-up of the project into its day files ([`reports::Rollup`]);
    /// none on a private server, which writes none.
    pub(crate) reports: Mutex<Option<reports::Rollup>>,
    dirty: Mutex<BTreeSet<PathBuf>>,
    /// What the backstop last saw of main and the served trees.
    swept: Mutex<Sweep>,
    common: Option<PathBuf>,
    /// The repository (its common dir) the board's lookup from `root` found when the hub was made
    /// ([`taskview::serve`]): what [`Hub::holds_its_project`] holds the folder to, and every reach
    /// of its board from here. None for a folder no repository held.
    repository: Option<PathBuf>,
    /// The cards waiting on the owner and how many plugin gate answers had come back
    /// ([`symphony::gates::Kept::answered`]) when the sign-off lane was last read: either moving
    /// reads it again.
    signoff_pending: Mutex<Option<(Pending, usize)>>,
    task_activity: Mutex<Option<(i64, Vec<symphony_api::ActivityItem>)>>,
    /// What the task board asks of each person beyond the cards ([`task_asks`]). Taken after
    /// `pages` where both are held, never before.
    task_asks: Mutex<task_asks::Asked>,
    worker: Mutex<Option<std::thread::JoinHandle<()>>>,
    /// What the board's ticks wait on between two reads, on their own thread ([`run`]): [`Hub::stop`]
    /// drops it, as the hub's own drop does, and that wakes the wait at once.
    ticks: Mutex<Option<mpsc::Sender<()>>>,
    /// The board's health over the last hour: the clock's dispatch passes, the idle sweeps and
    /// every request this process sent the board ([`symphony_board::board_health`]).
    pub(crate) health: symphony_board::board_health::Recorder,
    /// The agents' health changes and hourly counts, appended to their day files by the machine's
    /// one server alone ([`agent_activity`]); none on a private server.
    pub(crate) agent_activity: Mutex<Option<agent_activity::Recorder>>,
    /// Where the Reports page reads the project's day files ([`crate::api::reports::Folders`]),
    /// found at its first read: the folders the shared server's writers find, or a test's.
    pub(crate) report_folders: OnceLock<crate::api::reports::Folders>,
    /// The Reports page's answers kept between reads, each with what it was read from
    /// ([`crate::api::reports::Kept`]).
    pub(crate) reports_kept: crate::api::reports::Kept,
    /// What the Runs page's reads keep of its day files between them ([`crate::api::runs::Index`]).
    pub(crate) runs_index: crate::api::runs::Index,
    /// When they are recorded: its server generation's gate ([`symphony_board::recording`]), the
    /// one `health` and the roll-up's writes of the day files ([`reports`]) keep to as well, given
    /// as the hub starts ([`Hub::gate_recorders`]); none records nothing.
    recording_gate: OnceLock<Arc<symphony_board::recording::Gate>>,
    /// What `server_notice` says, by where it comes from ([`health::Notices`]).
    notices: Mutex<health::Notices>,
    /// Where the server serves this project's pages, `/<id>` under the machine's one server, empty
    /// for a server of its own at `/` ([`crate::front::Shared`]'s `base`): given as the hub starts
    /// ([`Hub::serve_at`]), and empty until then.
    base: OnceLock<String>,
    /// The machine folder, whose ledger of schema updates the read-only notice and the schema
    /// updates' route read ([`symphony::machine::updates`]): the machine's, none where it cannot be
    /// found and in the library's tests, which read no machine's unless they give one.
    pub(crate) machine: Option<PathBuf>,
    /// What the pages say of a replay of the board's store's log while the worker's last reach of
    /// the board was refused for one ([`symphony_board::server::replaying_in`]).
    replaying: Mutex<Option<String>>,
    /// The time the board's health is recorded and read at, in seconds since the epoch: the wall
    /// clock's, or a test's.
    pub(crate) clock: Box<dyn Fn() -> f64 + Send + Sync>,
    /// Where the hub is in the asks file ([`asks::AskWatch`]).
    asks: Mutex<asks::AskWatch>,
    /// The board's chat recipient as its last read named it; `None` before the first read.
    chat: Mutex<Option<Option<String>>>,
    /// The disk holding the project, when it runs low, read at each tick ([`health::low_disk`]): the
    /// machine's, or a test's.
    pub(crate) low_disk:
        Box<dyn Fn() -> Option<symphony_board::board_health::disk::Low> + Send + Sync>,
    /// Numbers each document's render as its read begins ([`Page::copied`]), so renders run side by
    /// side and the copy a page keeps is the last one begun: a reader waits on no refresh's
    /// renders. `pages` is taken only to place a copy, so the board, the feed and the project
    /// list, which read `pages`, never wait on a render either.
    reads: AtomicU64,
    /// How many times a document was rendered ([`Hub::render_document`]): what a test counts to
    /// see that a read rendered nothing.
    renders: AtomicU64,
    /// The store the page reads of the board share ([`Hub::reader`]), once one has been opened.
    reader: Mutex<BoardReader>,
    /// Independent deadlines for failed board work ([`retry`]).
    board_retry: Mutex<retry::Retry>,
    chat_retry: Mutex<retry::Retry>,
    dispatch_retry: Mutex<retry::Retry>,
    give_retry: Mutex<retry::Retry>,
    /// The idle sweep's rounds as the tick needs them: after a sleep, its dispatch and its give wait
    /// for a round begun after the wake ([`presence`]).
    presence: Mutex<presence::Presence>,
    retry_clock: Box<dyn Fn() -> Instant + Send + Sync>,
    /// How [`Hub::reader`] opens the board: [`taskview::connect`], held to `repository` as every
    /// reach from `root` here is ([`taskview::serve`]), or a test's board.
    pub(crate) connect_reader: Box<Connect>,
    /// The hc a Go to session asks about the panes of the checkout at a path ([`Panes`]).
    pub(crate) panes: Box<Panes>,
    /// The focuses and screen reads of a session under way, by kind and the session's handle: one
    /// at a time for each (`crate::session_go`).
    pub(crate) going: Mutex<std::collections::HashSet<String>>,
    /// The project's plugins' answers of a task's gates, kept by its branch's tip
    /// ([`symphony::gates::Kept`]): what the task page's gates read, and any other reader here.
    pub(crate) gates: symphony::gates::Kept,
    /// The execution providers this server holds, the installed ones or a test's: a task's
    /// attempts are resumed and viewed through them (`crate::attempts`).
    pub(crate) providers: crate::execution::Providers,
}

/// What the hub keeps of its last scan of the branches: their own cards, when they began to be
/// read (seconds since the epoch), and the branches whose worktree that scan could not read
/// ([`branches::Scan::failed`]). The cards are shared, so the hourly roll-up takes them without a
/// copy ([`reports`]).
type BranchScan = (Arc<Vec<branches::BranchCard>>, f64, Arc<BTreeSet<String>>);

#[derive(Default)]
struct BoardReader {
    store: Option<Arc<symphony_board::Store>>,
    retry: retry::Retry,
    error: Option<String>,
}

/// How a hub opens the board of the checkout at a path.
pub(crate) type Connect = dyn Fn(&Path) -> Result<symphony_board::Store, String> + Send + Sync;

/// The hc and Herdr app one Go to session asks about the panes of the checkout at a path, asked
/// for each focus or screen read: [`symphony_board::launch::go::asked`] in the board's environment,
/// its calls bound by one deadline from then, or a test's stand-in.
pub(crate) type Panes = dyn Fn(&Path) -> Box<dyn symphony_board::launch::hc::Hc> + Send + Sync;

/// Why the hub holds no board read before its first.
const STARTING: &str = "the task board is starting";

struct TaskState {
    /// Shared with the reads that take what they need of it, so none copies the whole board.
    last: Option<Arc<Tasks>>,
    raw: Option<symphony_board::Board>,
    busy: Vec<String>,
    error: Option<String>,
}

/// What the board's snapshot says of each task beside the board's own read: when each open task
/// took its status, read again only for a task whose status or write moved
/// ([`taskview::Stood`]), and the owner's sign-off lane with the cards each waits on, which the
/// snapshot sends as a decision ([`taskview::waiting::decided`]).
#[derive(Default)]
struct Waiting {
    stood: taskview::Stood,
    lane: BTreeMap<String, Vec<String>>,
}

#[derive(Default)]
struct OrderExpiry {
    retry_at: u64,
    retry_delay: u64,
}

impl OrderExpiry {
    fn tick(&mut self, root: &Path, now: u64, refresh: impl FnOnce() -> Result<(), String>) {
        if now < self.retry_at {
            return;
        }
        if !symphony::postpone::expired(root, now) {
            *self = Self::default();
            return;
        }
        match refresh() {
            Ok(()) => *self = Self::default(),
            Err(error) => {
                if self.retry_delay == 0 {
                    eprintln!("· queue expiry: {error}");
                }
                // Failed prunes retry exponentially, with a delay capped at one minute.
                // Only the first failure in a streak is logged; success resets the retry state.
                self.retry_delay = self.retry_delay.saturating_mul(2).clamp(2, 60);
                self.retry_at = now.saturating_add(self.retry_delay);
            }
        }
    }
}

impl Hub {
    /// The hub of the project at `root`, which this process serves from now on
    /// ([`taskview::serve`]): every reach of the board from `root` here, the hub's and any route's,
    /// is held to the repository found there now, and every journal found from it is the one
    /// found now.
    pub(crate) fn new(root: &Path, ui_dist: &Path) -> Self {
        let native_reader = machine_work::READER_ENV.os();
        let repository = taskview::serve(root);
        Self {
            root: root.into(),
            ui_dist: ui_dist.into(),
            embedded_ui: false,
            pages: RwLock::new(BTreeMap::new()),
            changed: watch::Sender::new(0),
            draining: watch::Sender::new(false),
            server_notice: watch::Sender::new(String::new()),
            initial_board: watch::Sender::new(true),
            initial_start: Mutex::new(None),
            invalid: RwLock::new(BTreeMap::new()),
            trees: RwLock::new({
                let mut trees = BTreeMap::new();
                for tree in measured("hub branch trees", || branches::trees(root)) {
                    trees.insert(tree.id(), tree.path);
                }
                trees
            }),
            tasks: Mutex::new(TaskState {
                last: None,
                raw: None,
                busy: Vec::new(),
                error: Some(STARTING.into()),
            }),
            listing: RwLock::new(Err(STARTING.into())),
            agents: Mutex::new(None),
            machine_work: Mutex::new(machine_work::Sampler::new(
                root,
                native_reader.as_deref().map(Path::new),
            )),
            latest_message: Mutex::new(None),
            task_signoff: Mutex::new(Vec::new()),
            workflow_generation: Mutex::new(None),
            waiting: Mutex::new(Waiting::default()),
            branches: Mutex::new((Arc::default(), 0.0, Arc::default())),
            finished: Mutex::new(BTreeSet::new()),
            rescan: std::sync::atomic::AtomicBool::new(false),
            documents_waker: OnceLock::new(),
            board_wakers: OnceLock::new(),
            refreshed: OnceLock::new(),
            listed: std::sync::atomic::AtomicBool::new(false),
            one_cards: RwLock::new(Arc::default()),
            marks: RwLock::new(Arc::default()),
            awaited: Mutex::new(Vec::new()),
            watch_changes: Mutex::new(BTreeSet::new()),
            warmed_watches: Mutex::new(BTreeMap::new()),
            callbacks: Mutex::new(None),
            owner_asks: Mutex::new(None),
            decided: Mutex::new(None),
            queue_asks: Mutex::default(),
            reports: Mutex::new(None),
            dirty: Mutex::new(BTreeSet::new()),
            swept: Mutex::new(Sweep::default()),
            common: measured("hub checkout", || crate::repo::checkout(root))
                .ok()
                .map(|c| c.common),
            repository,
            signoff_pending: Mutex::new(None),
            task_activity: Mutex::new(None),
            task_asks: Mutex::default(),
            worker: Mutex::new(None),
            ticks: Mutex::new(None),
            health: symphony_board::board_health::Recorder::default(),
            agent_activity: Mutex::new(None),
            report_folders: OnceLock::new(),
            reports_kept: crate::api::reports::Kept::default(),
            runs_index: crate::api::runs::Index::default(),
            recording_gate: OnceLock::new(),
            notices: Mutex::new(health::Notices::of(root)),
            base: OnceLock::new(),
            machine: health::machine_folder(),
            replaying: Mutex::new(None),
            clock: Box::new(symphony_board::sweep::trigger::now),
            asks: Mutex::new(asks::AskWatch::default()),
            chat: Mutex::new(None),
            low_disk: health::low_disk(root),
            reads: AtomicU64::new(0),
            renders: AtomicU64::new(0),
            reader: Mutex::default(),
            board_retry: Mutex::default(),
            chat_retry: Mutex::default(),
            dispatch_retry: Mutex::default(),
            give_retry: Mutex::default(),
            presence: Mutex::default(),
            retry_clock: Box::new(Instant::now),
            connect_reader: Box::new(taskview::connect),
            panes: Box::new(|root| {
                let mut env = symphony::children::board_env();
                env.cwd = root.to_path_buf();
                Box::new(symphony_board::launch::go::asked(&env))
            }),
            going: Mutex::new(std::collections::HashSet::new()),
            gates: symphony::gates::Kept::default(),
            providers: crate::execution::Providers::installed(),
        }
    }

    /// The cards waiting on the owner as the main checkout's page last read them; none before its
    /// first read.
    pub(crate) fn pending(&self) -> Pending {
        self.pages
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get("")
            .map_or_else(
                || Pending {
                    items: Vec::new(),
                    docs: Vec::new(),
                },
                |page| page.pending.clone(),
            )
    }

    /// Queue one watcher notification, retaining a watched input's change until its tree is refreshed.
    fn record_event(&self, event: notify::Result<notify::Event>) -> bool {
        if event
            .as_ref()
            .is_ok_and(|e| matches!(e.kind, notify::EventKind::Access(_)))
        {
            return false;
        }
        let failed = event.as_ref().map_or(true, notify::Event::need_rescan);
        let paths: Vec<PathBuf> = match event {
            Ok(event) if !failed => event
                .paths
                .into_iter()
                .filter(|p| relevant(self.common.as_deref(), p))
                .collect(),
            _ => vec![self.root.clone()],
        };
        if paths.is_empty() {
            return false;
        }
        self.dirty
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .extend(paths.iter().cloned());
        let roots: Vec<PathBuf> = std::iter::once(self.root.clone())
            .chain(
                self.trees
                    .read()
                    .unwrap_or_else(|e| e.into_inner())
                    .values()
                    .cloned(),
            )
            .collect();
        let mut dirty = self.watch_changes.lock().unwrap_or_else(|e| e.into_inner());
        for path in paths {
            if let Some(root) = watched_root(&roots, &path) {
                dirty.insert(root.to_path_buf());
            }
        }
        if failed {
            dirty.insert(self.root.clone());
            dirty.extend(
                self.trees
                    .read()
                    .unwrap_or_else(|e| e.into_inner())
                    .values()
                    .cloned(),
            );
        }
        true
    }

    pub fn task_snapshot(&self) -> Result<Tasks, String> {
        self.tasks_read().map(|tasks| Tasks::clone(&tasks))
    }

    /// The board's tasks and sessions as the last board read found them, shared rather than
    /// copied, or why there are none: the board cannot be read now, or has not been read yet.
    /// It never waits on a board read under way (the hub's `listing`).
    pub(crate) fn tasks_read(&self) -> Result<Arc<Tasks>, String> {
        // A panic under this lock leaves at worst the read before, a whole value.
        self.listing
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// The board's last read, or why there is none, set as [`Hub::tasks_read`] answers it: called
    /// with the task state held, where it changes, so the two never disagree once it is let go. A
    /// read that finds the board, or that finds none, where the one before said otherwise, wakes
    /// what waits for that ([`Hub::wake_at_board`]).
    fn publish_listing(&self, read: Result<Arc<Tasks>, String>) {
        let absent = |read: &Result<Arc<Tasks>, String>| {
            read.as_ref()
                .err()
                .is_some_and(|why| symphony_board::board_health::absent(why))
        };
        let mut listing = self.listing.write().unwrap_or_else(|e| e.into_inner());
        let moved = absent(&listing) != absent(&read);
        *listing = read;
        drop(listing);
        if moved {
            self.board_moved();
        }
    }

    /// Wake what waits for the board's arrival or its going ([`Hub::has_board`]).
    pub(super) fn board_moved(&self) {
        if let Some(wakers) = self.board_wakers.get() {
            wakers.wake();
        }
    }

    /// Wake `wakers` each time this hub finds or loses its board, its first read's end included:
    /// its server's maintenance, which waits for a board ([`crate::front::maintenance_worker`]).
    /// Given once, as the hub starts; a second giving is ignored.
    pub(crate) fn wake_at_board(&self, wakers: Arc<Wakers>) {
        let _ = self.board_wakers.set(wakers);
    }

    /// Call `then` after each refresh of the documents: the server's saves take up again what
    /// they keep for delivery by pull request, which a checkout that moved may have settled.
    /// Given once, as the hub starts; a second giving is ignored.
    pub(crate) fn after_refresh(&self, then: Box<dyn Fn() + Send + Sync>) {
        let _ = self.refreshed.set(then);
    }

    /// Ask the documents' loop to scan the branches again, though no file moved, and wake it.
    fn ask_rescan(&self) {
        self.rescan.store(true, std::sync::atomic::Ordering::SeqCst);
        self.wake_documents();
    }

    /// Wake the documents' loop, when it runs.
    fn wake_documents(&self) {
        if let Some(waker) = self.documents_waker.get() {
            waker.wake();
        }
    }

    /// The hub's task state held as a board read holds it across its reads of the board
    /// (`update_board`), until what this returns drops: what a test holds to see that a reader
    /// waits on none of it.
    #[cfg(test)]
    pub(crate) fn hold_board_read(&self) -> impl Sized + '_ {
        self.tasks.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The store the page reads of the board share, opened by the first and kept, so the reads of a
    /// page (a task page asks for its task, its offer and its history) and of every page after it go
    /// on its connections, never opening the board anew: finding its server, asking `ps` and `lsof`
    /// that it is that, and reading its schema. Reads wait while one opens it. A failed opening
    /// keeps its error until its retry deadline ([`retry`]); requests never reset that deadline. One
    /// refused while the board is held is tried again by the first request after its release.
    pub(crate) fn reader(&self) -> Result<Arc<symphony_board::Store>, String> {
        let mut kept = self.reader.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(store) = &kept.store {
            return Ok(Arc::clone(store));
        }
        if !self.holds_its_project() {
            return Err(self.gone());
        }
        match kept
            .retry
            .run_unless_released(&self.retry_clock, &|| self.board_held(), || {
                (self.connect_reader)(&self.root)
            }) {
            Some(Ok(store)) => {
                let store = Arc::new(store);
                kept.store = Some(Arc::clone(&store));
                kept.error = None;
                Ok(store)
            }
            Some(Err(error)) => {
                kept.error = Some(error.clone());
                Err(error)
            }
            None => Err(kept.error.clone().expect("a failed opening waits")),
        }
    }

    /// Forget `store` after the board failed a read on it, unless it is already forgotten: the next
    /// read opens the board anew, where its server is then.
    pub(crate) fn reader_failed(&self, store: &Arc<symphony_board::Store>) {
        let mut kept = self.reader.lock().unwrap_or_else(|e| e.into_inner());
        if kept
            .store
            .as_ref()
            .is_some_and(|kept| Arc::ptr_eq(kept, store))
        {
            kept.store = None;
        }
    }

    /// `read`, made on `store`, as it is: where the board failed it (503, or 504 for one too slow),
    /// `store` is forgotten ([`Hub::reader_failed`]), as its server may have gone, or moved. A
    /// refusal of what was asked (404, 400, 409) keeps it.
    pub(crate) fn read_on<T>(
        &self,
        store: &Arc<symphony_board::Store>,
        read: Result<T, (StatusCode, String)>,
    ) -> Result<T, (StatusCode, String)> {
        if let Err((status, _)) = &read
            && [StatusCode::SERVICE_UNAVAILABLE, StatusCode::GATEWAY_TIMEOUT].contains(status)
        {
            self.reader_failed(store);
        }
        read
    }

    /// The document `rel` of the checkout at `root`, its tasks those of the board as last read that
    /// link it: a render reads no board.
    fn render_document(
        &self,
        root: &Path,
        rel: &str,
        scope: &str,
    ) -> Result<symphony_api::Document, String> {
        self.renders
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let board = self.tasks_read().ok();
        let (cards, drafts) = if scope.is_empty() {
            (self.one_cards(rel), BTreeSet::new())
        } else {
            // A branch page draws main's card for each card its branch did not post, and marks
            // its drafts.
            let page = format!("@{scope}/{rel}");
            let marks = self
                .marks
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .get(&page)
                .cloned()
                .unwrap_or_default();
            let cards = branches::page_cards(
                &self.root,
                root,
                rel,
                &self.one_cards(&page),
                &self.one_cards(rel),
                &marks,
            );
            (cards, marks.drafts)
        };
        symphony::docview::document_marked(
            &symphony::render::Render::new(root),
            rel,
            scope,
            board.as_ref().map_or(&[], |board| &board.tasks),
            &cards,
            &drafts,
        )
    }

    /// The one cards the page `page` (`docs/X.md`, or `@<tree>/docs/X.md`) shows in place of its
    /// file's copies, as main's last snapshot made them.
    fn one_cards(&self, page: &str) -> Vec<symphony::Decision> {
        self.one_cards
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(page)
            .cloned()
            .unwrap_or_default()
    }

    /// Note the branches whose task `board` says is finished; a change to a branch a worktree is on
    /// asks the loop to scan the branches again, since no file moves when a task is done.
    fn note_finished(&self, board: &symphony_board::Board) {
        let finished = branches::finished(board);
        let mut held = self.finished.lock().unwrap_or_else(|e| e.into_inner());
        if *held == finished {
            return;
        }
        let trees = self.trees.read().unwrap_or_else(|e| e.into_inner());
        let touched = held
            .symmetric_difference(&finished)
            .any(|branch| trees.contains_key(&branch.replace('/', "~")));
        drop(trees);
        *held = finished;
        if touched {
            self.ask_rescan();
        }
    }

    /// Whether a picture a held card waits on is on disk now: its card is posted at the next scan.
    /// No watcher follows those files, so each of the board's ticks looks ([`ticks`]).
    fn pictures_arrived(&self) -> bool {
        self.awaited
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .any(|file| file.exists())
    }

    /// The checkout the worktree `scope` names: the project's own for none, a linked worktree's by
    /// its id, and nothing for one that is not there (never the project's own in its place).
    pub(crate) fn checkout(&self, scope: &str) -> Option<PathBuf> {
        if scope.is_empty() {
            Some(self.root.clone())
        } else {
            self.tree(scope)
        }
    }

    pub fn tree(&self, id: &str) -> Option<PathBuf> {
        self.trees
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .cloned()
    }

    pub fn ensure_page(&self, scope: &str) -> Result<(), String> {
        if self
            .pages
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(scope)
        {
            return Ok(());
        }
        let root = self.tree(scope).ok_or_else(|| {
            "no worktree for this branch (merged, removed or never had cards)".to_string()
        })?;
        // Stamped before the read, so the sweep that first covers this tree reports only what
        // changed after it.
        let roots: Vec<_> = tree_inputs(&root)
            .into_iter()
            .map(|(below, mode)| (root.clone(), below, mode))
            .collect();
        self.swept
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .adopt(&roots);
        self.refresh_page(scope, &root)
    }

    /// The feed's cursor of the checkout `scope` names: the version of the last event published
    /// for its pages; none for a checkout with no page.
    pub fn cursor(&self, scope: &str) -> Option<u64> {
        let pages = self.pages.read().unwrap_or_else(|e| e.into_inner());
        pages.get(scope).map(|page| page.board.version)
    }

    pub fn problem(&self, scope: &str) -> Option<String> {
        self.invalid
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(scope)
            .cloned()
    }

    pub fn document(&self, scope: &str, rel: &str) -> Result<symphony_api::Document, String> {
        self.rendered(scope, rel, true)
    }

    /// The tag of `rel`'s outline as the copy `scope`'s page follows makes it, and the feed's
    /// cursor it stands at: what an outline's check that names that tag is answered `304` by,
    /// with no render. `tag_of` makes a document's outline's tag, and is run once for each copy
    /// placed.
    ///
    /// The copy is the hub's own record of what a render reads from the checkout: a refresh
    /// renders it again once a file of its checkout's documents, the project's settings, its one
    /// cards or, on a branch's page, main's baseline changed ([`Hub::refresh_page_read`]), a board
    /// read once a task that links it, or comes to, moved ([`Hub::update_board`]), and its
    /// sections are read from it too ([`Hub::document_sections`]). So the check says what a page
    /// left open shows: an edit the watcher has not told yet, or a board read has not rendered
    /// yet, arrives on the feed from the cursor given here.
    ///
    /// `None`, and the check renders as [`Hub::document`] does, which publishes what it finds:
    /// for a document the page does not follow yet or holds as missing, and for a copy naming a
    /// file from outside the entrypoints' folders by its validator
    /// ([`symphony::docview::names_files_outside`]), whose change no refresh sees.
    pub fn outline_kept(
        &self,
        scope: &str,
        rel: &str,
        tag_of: impl FnOnce(symphony_api::Document) -> String,
    ) -> Option<(String, u64)> {
        let root = self.checkout(scope)?;
        // The copy the page holds now, and the read it came from.
        let held = |page: &Page| -> Option<u64> {
            if !page.documents.contains_key(rel) || page.missing.contains(rel) {
                return None;
            }
            page.copied.get(rel).copied()
        };
        let made = |page: &Page, read: u64| -> Option<Option<String>> {
            let outlines = page.outlines.lock().unwrap_or_else(|e| e.into_inner());
            outlines
                .get(rel)
                .filter(|(of, _)| *of == read)
                .map(|(_, tag)| tag.clone())
        };
        let (read, document) = {
            let pages = self.pages.read().unwrap_or_else(|e| e.into_inner());
            let page = pages.get(scope)?;
            let read = held(page)?;
            if let Some(tag) = made(page, read) {
                return tag.map(|tag| (tag, page.board.version));
            }
            (read, page.documents.get(rel)?.clone())
        };
        // Made outside the lock: the outline is the whole document written out.
        let project = symphony::project::Project::of(&root);
        let tag = (!symphony::docview::names_files_outside(&document, &project, scope))
            .then(|| tag_of(document));
        let pages = self.pages.read().unwrap_or_else(|e| e.into_inner());
        let page = pages.get(scope)?;
        // A copy placed meanwhile is another document: its own check makes its tag.
        if held(page) != Some(read) {
            return None;
        }
        page.outlines
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(rel.into(), (read, tag.clone()));
        tag.map(|tag| (tag, page.board.version))
    }

    /// `rel` rendered as [`Hub::document`] renders it, for a page reading it ahead
    /// (`/api/document?view=first`): not followed, so it publishes no change and the documents'
    /// refresh never renders it again. The visit's own outline read follows it.
    pub fn document_ahead(&self, scope: &str, rel: &str) -> Result<symphony_api::Document, String> {
        self.rendered(scope, rel, false)
    }

    /// `rel` rendered in `scope` at the feed's cursor; while `follow`, any change from the page's
    /// copy published first and the copy replaced, the document followed from then on.
    fn rendered(
        &self,
        scope: &str,
        rel: &str,
        follow: bool,
    ) -> Result<symphony_api::Document, String> {
        let root = self.checkout(scope).ok_or("this worktree was removed")?;
        let removed = || "this worktree was removed".to_string();
        if !self
            .pages
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(scope)
        {
            return Err(removed());
        }
        let mut read_again = true;
        loop {
            let read = self.reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let mut doc = self.render_document(&root, rel, scope)?;
            if !follow {
                let pages = self.pages.read().unwrap_or_else(|e| e.into_inner());
                doc.version = pages.get(scope).ok_or_else(removed)?.board.version;
                return Ok(doc);
            }
            let mut pages = self.pages.write().unwrap_or_else(|e| e.into_inner());
            let page = pages.get_mut(scope).ok_or_else(removed)?;
            // A refresh that renders anew, or a board read whose tasks moved, begun since this read
            // did, which listed the documents to render without this one, as none followed it:
            // what this read read, its files or its tasks, may be gone. It reads once again; behind
            // a second such refresh it is placed and left to the next, so no reader renders on
            // while the files or the board keep changing.
            let behind = read < page.renews_from && !page.documents.contains_key(rel);
            if behind && std::mem::take(&mut read_again) {
                continue;
            }
            // A render begun after this one placed its copy first: that copy is the newer.
            if page.copied.get(rel).is_some_and(|copied| *copied > read)
                && let Some(newer) = page.documents.get(rel)
            {
                doc = newer.clone();
            } else {
                // A GET can observe an edit before the watcher. Publish it before handing out its
                // cursor, so existing subscribers also receive that edit.
                if let Some(delta) = document_change(page, rel, &doc) {
                    self.publish(page, vec![region_event("document", json!(delta))]);
                }
                page.documents.insert(rel.into(), doc.clone());
                page.copied.insert(rel.into(), read);
                if behind {
                    page.stale.insert(rel.into());
                    self.ask_rescan();
                }
            }
            doc.version = page.board.version;
            return Ok(doc);
        }
    }

    pub fn folder(&self, scope: &str, rel: &str) -> Result<symphony_api::Folder, String> {
        let root = self.checkout(scope).ok_or("this worktree was removed")?;
        let board = self.tasks_read().ok();
        let mut folder = symphony::docview::folder(
            &symphony::render::Render::new(root),
            rel,
            scope,
            board.as_ref().map_or(&[], |board| &board.tasks),
        )?;
        let pages = self.pages.read().unwrap_or_else(|e| e.into_inner());
        if let Some(main) = pages.get("") {
            symphony::docview::count_waiting(&mut folder, &main.queue.waiting);
        }
        Ok(folder)
    }

    /// An outline has already registered the document with the watcher. Read its maintained
    /// snapshot under the same lock as the feed cursor; scrolling never rerenders a whole file.
    pub fn document_sections(
        &self,
        scope: &str,
        rel: &str,
        keys: &[String],
    ) -> Result<symphony_api::DocumentSections, String> {
        {
            let pages = self.pages.read().unwrap_or_else(|e| e.into_inner());
            if let Some(page) = pages.get(scope)
                && let Some(document) = page.documents.get(rel)
            {
                return Ok(symphony::docview::selected(
                    document,
                    keys,
                    page.board.version,
                ));
            }
        }
        let document = self.document(scope, rel)?;
        Ok(symphony::docview::selected(
            &document,
            keys,
            document.version,
        ))
    }

    fn refresh_docs(&self, dirty: Option<&BTreeSet<PathBuf>>) -> Result<(), String> {
        let listing = branches::listed(&self.root);
        if listing.is_none() {
            eprintln!("branch worktrees not listed: the owner's asks on a branch wait");
        }
        self.listed
            .store(listing.is_some(), std::sync::atomic::Ordering::SeqCst);
        let listing = listing.unwrap_or_default();
        let mut trees = BTreeMap::new();
        for tree in &listing {
            trees.insert(tree.id(), tree.path.clone());
        }
        let previous = std::mem::replace(
            &mut *self.trees.write().unwrap_or_else(|e| e.into_inner()),
            trees,
        );
        let read_at = symphony::owner_asks::now();
        let finished = self
            .finished
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let scan = measured("branch cards", || {
            branches::scan_listed(&self.root, listing, &finished)
        });
        if !scan.failed.is_empty() {
            let failed: Vec<&str> = scan.failed.iter().map(String::as_str).collect();
            eprintln!(
                "branch worktrees not read: {}: the owner's asks on their cards wait",
                failed.join(", ")
            );
        }
        *self.branches.lock().unwrap_or_else(|e| e.into_inner()) =
            (Arc::new(scan.cards), read_at, Arc::new(scan.failed));
        *self.awaited.lock().unwrap_or_else(|e| e.into_inner()) = scan.awaited;
        *self.marks.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(scan.pages);
        let shown_before = Arc::clone(&self.one_cards.read().unwrap_or_else(|e| e.into_inner()));
        // Main's documents are read from main's own files: a ref, a worktree's record or a branch's
        // edit changes none of them. A lost event names main's root.
        let main_files_changed = dirty.is_none_or(|paths| {
            let inputs = tree_inputs(&self.root);
            let trees = self.trees.read().unwrap_or_else(|e| e.into_inner());
            paths.iter().any(|path| {
                *path == self.root
                    || (inputs
                        .iter()
                        .any(|(below, _)| path.starts_with(self.root.join(below)))
                        && !trees.values().any(|tree| path.starts_with(tree)))
            })
        });
        // Main also displays branch-only cards; branch document edits can change that queue.
        let main = self.refresh_page_read("", &self.root, main_files_changed);
        let shown_after = Arc::clone(&self.one_cards.read().unwrap_or_else(|e| e.into_inner()));
        let scopes = self
            .pages
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .filter(|s| !s.is_empty())
            .cloned()
            .collect::<Vec<_>>();
        // Branch document bodies also read main's committed baseline. A main ref update can
        // clear their comparisons without touching a single file in the branch checkout.
        let baseline_changed = self.common.as_ref().is_some_and(|common| {
            dirty.is_none_or(|paths| {
                paths.contains(&common.join(format!(
                    "refs/heads/{}",
                    symphony::project::Project::of(&self.root).default_branch
                ))) || paths.contains(&common.join("packed-refs"))
            })
        });
        // A branch page draws main's card for each card its branch did not post: a change to
        // main's cards changes every branch page.
        let mains = |pages: &dashboard::Pages| {
            pages
                .iter()
                .filter(|(page, _)| !page.starts_with('@'))
                .map(|(page, cards)| (page.clone(), cards.clone()))
                .collect::<Vec<_>>()
        };
        let main_cards_changed = mains(&shown_before) != mains(&shown_after) || main_files_changed;
        for scope in scopes {
            let root = self.tree(&scope);
            // A page that goes, or follows its tree to another folder, leaves that folder's
            // watched inputs cold, so serving the folder again reads them afresh.
            if let Some(left) = previous.get(&scope)
                && root.as_ref() != Some(left)
            {
                self.forget_watched(left);
            }
            if let Some(root) = root {
                // A branch page shows its posted cards as one card with main's copy: a change on
                // either side changes the page.
                let prefix = format!("@{scope}/");
                let shown = |pages: &dashboard::Pages| {
                    pages
                        .iter()
                        .filter(|(page, _)| page.starts_with(&prefix))
                        .map(|(page, cards)| (page.clone(), cards.clone()))
                        .collect::<Vec<_>>()
                };
                // Its drafts and own cards ([`branches::PageMarks`]) change with its files, whose
                // event refreshes it, or with what it posted, which changes its one cards.
                let documents_changed = baseline_changed
                    || main_cards_changed
                    || shown(&shown_before) != shown(&shown_after)
                    || scope_affected(
                        previous.get(&scope).map(PathBuf::as_path),
                        &root,
                        dirty,
                        &self.root,
                        &self.ui_dist,
                    );
                // Decision metadata is global, even while an unaffected branch's open bodies
                // remain cached. An edit in another checkout can change the next decision.
                if let Err(e) = self.refresh_page_read(&scope, &root, documents_changed) {
                    eprintln!("· Symphony data for {scope}: {e}");
                }
            } else {
                self.pages
                    .write()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&scope);
                self.changed.send_modify(|version| *version += 1);
            }
        }
        // A branch's new tip changes its task's gates: an open task page reads them again.
        let moved = moved_branches(self.common.as_deref(), dirty);
        if !moved.is_empty() {
            let change = region_event("branches", json!(moved));
            let mut pages = self.pages.write().unwrap_or_else(|e| e.into_inner());
            for page in pages.values_mut() {
                self.publish(page, vec![change.clone()]);
            }
        }
        main
    }

    /// Drop what the plugins' watches keep for `root`, so its next read builds it afresh.
    fn forget_watched(&self, root: &Path) {
        self.warmed_watches
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(root);
        self.watch_changes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(root);
        for watch in symphony::plugins::Plugins::of(root).watches() {
            watch.forget(root);
        }
    }

    pub fn queue_order(&self) -> Result<Arc<symphony::postpone::View>, String> {
        self.pages
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get("")
            .map(|page| page.order.clone())
            .ok_or_else(|| "main's Symphony data is not ready".into())
    }

    pub fn refresh_order(&self, scope: &str) -> Result<(), String> {
        if !scope.is_empty() && self.tree(scope).is_none() {
            return Err("no such worktree".into());
        }
        let mut pages = self.pages.write().unwrap_or_else(|e| e.into_inner());
        let waiting = pages
            .get("")
            .ok_or("main's Symphony data is not ready")?
            .order
            .waiting(
                &self.root,
                symphony::postpone::OWNER,
                symphony::postpone::now(),
            )
            .map_err(|e| e.to_string())?;
        for page in pages.values_mut() {
            let mut board = page.board.clone();
            board.next = waiting
                .iter()
                .take(symphony_api::BOARD_CARDS)
                .cloned()
                .collect();
            let mut changes = board_changes(&page.board, &board);
            diff_items(
                &mut changes,
                "queue-item",
                &page.queue.waiting,
                &waiting,
                |card| format!("{}#{}", card.doc, card.slug),
            );
            page.board = board;
            page.queue.waiting = waiting.clone();
            self.publish(page, changes);
        }
        Ok(())
    }

    /// Read `scope`'s page again from its checkout at `root`, its documents among it.
    pub(crate) fn refresh_page(&self, scope: &str, root: &Path) -> Result<(), String> {
        self.refresh_page_read(scope, root, true)
    }

    fn refresh_page_read(
        &self,
        scope: &str,
        root: &Path,
        documents_changed: bool,
    ) -> Result<(), String> {
        let watched_changed = self
            .watch_changes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(root);
        let mut warmed = self
            .warmed_watches
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let inputs: Vec<_> = watched(root)
            .into_iter()
            .map(|below| (root.to_path_buf(), below, RecursiveMode::Recursive))
            .collect();
        // A native notification can arrive after the read that already included its edit.
        // Compare with the inputs stamped before that read, retaining edits made during it.
        let rebuild = match warmed.entry(root.to_path_buf()) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(Sweep::new(&inputs));
                true
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                watched_changed && !entry.get_mut().next(&inputs).is_empty()
            }
        };
        if rebuild {
            for watch in symphony::plugins::Plugins::of(root).watches() {
                let started = Instant::now();
                if let Some(line) = watch.changed(root) {
                    eprintln!(
                        "→ {line} in {:.1} ms",
                        started.elapsed().as_secs_f64() * 1000.0
                    );
                }
            }
        }
        drop(warmed);
        // Main's documents are read from here on, its branches' cards from when they were read.
        let mut read_at = symphony::owner_asks::now();
        let mut unscanned = Arc::default();
        let snapshot = measured("dashboard", || {
            if scope.is_empty() {
                let cards = self.branches.lock().unwrap_or_else(|e| e.into_inner());
                read_at = read_at.min(cards.1);
                unscanned = Arc::clone(&cards.2);
                dashboard::snapshot(
                    root,
                    &cards.0,
                    &crate::client::build(&self.ui_dist, self.embedded_ui),
                )
            } else {
                let pages = self.pages.read().unwrap_or_else(|e| e.into_inner());
                let main = pages.get("").ok_or_else(|| {
                    symphony::ReviewError("main's Symphony data is not ready".into())
                })?;
                Ok(dashboard::Snapshot {
                    board: main.board.clone(),
                    queue: main.queue.clone(),
                    pending: main.pending.clone(),
                    activity: main.activity.items.clone(),
                    order: main.order.clone(),
                    pages: Arc::clone(&self.one_cards.read().unwrap_or_else(|e| e.into_inner())),
                    held: Arc::clone(&main.held),
                    said: Arc::clone(&main.said),
                })
            }
        });
        // Main's one cards before this read, to tell which of its documents show other cards now.
        let mut cards_before = None;
        let mut snapshot = match snapshot {
            Ok(snapshot) => {
                self.invalid
                    .write()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(scope);
                if scope.is_empty() {
                    // Read by the documents rendered below, and by every page's after them.
                    cards_before = Some(std::mem::replace(
                        &mut *self.one_cards.write().unwrap_or_else(|e| e.into_inner()),
                        Arc::clone(&snapshot.pages),
                    ));
                }
                snapshot
            }
            Err(e) => {
                let error = e.to_string();
                self.invalid
                    .write()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(scope.into(), error.clone());
                self.changed.send_modify(|version| *version += 1);
                return Err(error);
            }
        };
        let board = self.tasks_read().ok();
        let cards_moved = cards_before.as_ref().is_some_and(|before| {
            !Arc::ptr_eq(before, &snapshot.pages) && **before != *snapshot.pages
        });
        // A first read begun before this point may have read what this refresh reads anew, and
        // is in no list below: it reads again rather than place its copy. Main's new one cards
        // are in place, and every file read from here on was edited before this refresh began.
        let begun = (documents_changed || cards_moved)
            .then(|| self.reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst));
        // Which followed documents are read again: every one while the page's files changed, else
        // only one a first read left behind, or whose cards or linking tasks moved, which no file
        // says. Listed under the lock the first reads place under, so each is listed or reads again.
        let listed: Vec<(String, bool, Vec<symphony_api::DocumentTask>)> = self
            .pages
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .get_mut(scope)
            .map(|page| {
                // A board read's listing, which the ticks make beside this refresh, may have
                // marked a later read since this one began: the mark only moves forward.
                if let Some(begun) = begun {
                    page.renews_from = page.renews_from.max(begun);
                }
                let stale = std::mem::take(&mut page.stale);
                page.documents
                    .iter()
                    .map(|(rel, doc)| {
                        let tasks = if documents_changed {
                            Vec::new()
                        } else {
                            doc.tasks.clone()
                        };
                        (rel.clone(), stale.contains(rel), tasks)
                    })
                    .collect()
            })
            .unwrap_or_default();
        let followed: Vec<String> = listed
            .into_iter()
            .filter(|(rel, stale, tasks)| {
                documents_changed
                    || *stale
                    || cards_before
                        .as_ref()
                        .is_some_and(|before| before.get(rel) != snapshot.pages.get(rel))
                    || *tasks
                        != symphony::docview::linked(
                            root,
                            rel,
                            board.as_ref().map_or(&[], |board| &board.tasks),
                        )
            })
            .map(|(rel, _, _)| rel)
            .collect();
        let rendered = self.render_again(root, scope, followed);
        let mut pages = self.pages.write().unwrap_or_else(|e| e.into_inner());
        // Preferences can change while a watched source snapshot is being prepared.
        if scope.is_empty() {
            if let Ok(waiting) = snapshot.order.waiting(
                &self.root,
                symphony::postpone::OWNER,
                symphony::postpone::now(),
            ) {
                snapshot.board.next = waiting
                    .iter()
                    .take(symphony_api::BOARD_CARDS)
                    .cloned()
                    .collect();
                snapshot.queue.waiting = waiting;
            }
            // The board's task counts are the ticks' ([`Hub::update_board`], [`Hub::board_error`]),
            // which run beside this refresh: the page keeps the ones they last gave it, never ones
            // read before these renders. A page made anew takes the last board read's.
            snapshot.board.counts.tasks = match pages.get(scope) {
                Some(page) => page.board.counts.tasks.clone(),
                None => board.as_deref().map(taskview::counts),
            };
        } else if let Some(main) = pages.get("") {
            snapshot.board = main.board.clone();
            snapshot.queue = main.queue.clone();
            snapshot.pending = main.pending.clone();
            snapshot.activity = main.activity.items.clone();
            snapshot.order = main.order.clone();
        }
        let activity = Activity {
            items: snapshot.activity,
        };
        // Each card waiting on an agent says the ask the board last gave for it.
        let asks = Arc::clone(&self.queue_asks.lock().unwrap_or_else(|e| e.into_inner()));
        symphony::owner_asks::note(&mut snapshot.queue.agent, &asks, &snapshot.held);
        if let Some(page) = pages.get_mut(scope) {
            // Keep board activity until the next board read combines it with document activity.
            let mut combined = activity;
            if scope.is_empty() {
                combined.items.extend(
                    page.activity
                        .items
                        .iter()
                        .filter(|i| i.kind == "task message")
                        .cloned(),
                );
            }
            sort_activity(&mut combined);
            let mut changes = board_changes(&page.board, &snapshot.board);
            place(page, rendered, &mut changes);
            diff_items(
                &mut changes,
                "queue-item",
                &page.queue.waiting,
                &snapshot.queue.waiting,
                |c| format!("{}#{}", c.doc, c.slug),
            );
            diff_items(
                &mut changes,
                "spot-check",
                &page.queue.spot_check,
                &snapshot.queue.spot_check,
                |c| c.href.clone(),
            );
            diff_items(
                &mut changes,
                "agent-queue",
                &page.queue.agent,
                &snapshot.queue.agent,
                |c| c.href.clone(),
            );
            diff_items(
                &mut changes,
                "pending",
                &page.pending.items,
                &snapshot.pending.items,
                |c| c.href.clone(),
            );
            if page.pending.docs != snapshot.pending.docs {
                changes.push(region_event("docs", json!(snapshot.pending.docs)));
            }
            diff_items(
                &mut changes,
                "activity",
                &page.activity.items,
                &combined.items,
                |c| c.id.clone(),
            );
            snapshot.board.version = page.board.version;
            page.board = snapshot.board;
            page.queue = snapshot.queue;
            page.pending = snapshot.pending;
            page.activity = combined;
            page.order = snapshot.order;
            page.read_at = read_at;
            page.held = snapshot.held;
            page.said = snapshot.said;
            page.unscanned = unscanned;
            self.publish(page, changes);
        } else {
            snapshot.board.version = 1;
            pages.insert(
                scope.into(),
                Page {
                    board: snapshot.board,
                    queue: snapshot.queue,
                    pending: snapshot.pending,
                    activity,
                    order: snapshot.order,
                    history: VecDeque::new(),
                    documents: BTreeMap::new(),
                    copied: BTreeMap::new(),
                    renews_from: 0,
                    stale: BTreeSet::new(),
                    outlines: Mutex::default(),
                    missing: BTreeSet::new(),
                    read_at,
                    held: snapshot.held,
                    said: snapshot.said,
                    unscanned,
                },
            );
        }
        Ok(())
    }

    /// The followed documents `followed` of the checkout at `root` rendered again for `scope`'s
    /// page, each numbered as its read begins ([`Hub::reads`]): read before `pages` is taken, and
    /// placed under it ([`place`]).
    fn render_again(&self, root: &Path, scope: &str, followed: Vec<String>) -> Vec<Rendered> {
        followed
            .into_iter()
            .map(|rel| {
                let read = self.reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let doc = self.render_document(root, &rel, scope);
                (rel, read, doc)
            })
            .collect()
    }

    /// Each page's followed documents that the board's `tasks` link otherwise than its copy says,
    /// rendered again ([`Hub::render_again`]), by the page's scope: a board read changes no file,
    /// so no refresh renders them. Read once the hub holds `tasks`, so each render lists them.
    ///
    /// Listed under the lock the first reads place under, as a refresh lists: a first read begun
    /// before may have read the board before `tasks`, and reads again ([`Page::renews_from`]).
    fn render_relinked(&self, tasks: &[symphony_api::Task]) -> BTreeMap<String, Vec<Rendered>> {
        let begun = self.reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let listed: Vec<(String, Followed)> = self
            .pages
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .iter_mut()
            .map(|(scope, page)| {
                // A refresh beside the ticks may have marked a later read: it only moves forward.
                page.renews_from = page.renews_from.max(begun);
                let followed = page
                    .documents
                    .iter()
                    .filter(|(rel, _)| !page.missing.contains(*rel))
                    .map(|(rel, doc)| (rel.clone(), doc.tasks.clone()))
                    .collect();
                (scope.clone(), followed)
            })
            .collect();
        let mut rendered = BTreeMap::new();
        for (scope, documents) in listed.into_iter().filter_map(|(scope, followed)| {
            let root = self.checkout(&scope)?;
            let moved: Vec<String> = followed
                .into_iter()
                .filter(|(rel, held)| *held != symphony::docview::linked(&root, rel, tasks))
                .map(|(rel, _)| rel)
                .collect();
            (!moved.is_empty()).then(|| {
                let rendered = self.render_again(&root, &scope, moved);
                (scope, rendered)
            })
        }) {
            rendered.insert(scope, documents);
        }
        rendered
    }

    fn publish(&self, page: &mut Page, changes: Vec<Value>) {
        if changes.is_empty() {
            return;
        }
        page.board.version += 1;
        let version = page.board.version;
        let mut data = serde_json::Map::new();
        data.insert("version".into(), Value::from(version));
        data.insert("changes".into(), Value::Array(changes));
        page.history.push_back(Event {
            version,
            json: Value::Object(data).to_string(),
        });
        while page.history.len() > HISTORY {
            page.history.pop_front();
        }
        self.changed.send_modify(|version| *version += 1);
    }

    /// The active custom workflow's activation generation as the hub last read its board: none
    /// before its first read and on a board with none.
    pub(crate) fn workflow_generation(&self) -> Option<u64> {
        let held = (self.workflow_generation.lock()).unwrap_or_else(|e| e.into_inner());
        held.flatten()
    }

    /// Keep `generation`, the custom workflow's active generation just read (none on a board that
    /// never activated one); once it changes after the first read, the change every page hears,
    /// so a page that draws by the definition reads it again.
    fn workflow_read(&self, generation: Option<u64>) -> Option<Value> {
        let mut held = (self.workflow_generation.lock()).unwrap_or_else(|e| e.into_inner());
        if *held == Some(generation) {
            return None;
        }
        let first = held.is_none();
        *held = Some(generation);
        (!first).then(|| region_event("workflow", json!({ "generation": generation })))
    }

    /// Apply one board observation after the journal, callback delivery and machine probes finish.
    fn update_board(
        &self,
        store: &symphony_board::Store,
        snapshot: symphony_board::Board,
        agents: symphony_api::Agents,
    ) -> Result<(), String> {
        let mut busy: Vec<String> = agents
            .agents
            .iter()
            .filter(|agent| agent.state == "running_test")
            .map(|agent| agent.handle.clone())
            .collect();
        busy.sort_unstable();
        let unchanged = {
            let state = self.tasks.lock().unwrap_or_else(|e| e.into_inner());
            (state.raw.as_ref() == Some(&snapshot) && state.busy == busy)
                .then(|| state.last.clone())
                .flatten()
        };
        let (mut next, latest) = match unchanged {
            Some(last) => (last, snapshot.latest_message),
            None => {
                // Only a board that changed can finish a task.
                self.note_finished(&snapshot);
                let mut waiting = self.waiting.lock().unwrap_or_else(|e| e.into_inner());
                // A beat moves the board, not a task's status: this reads no thread then.
                measured("task stages", || waiting.stood.read(store, &snapshot))
                    .map_err(|e| e.to_string())?;
                let (next, latest) = measured("board snapshot", || {
                    taskview::snapshot_board_with_busy(
                        &snapshot,
                        &waiting.stood,
                        &waiting.lane,
                        &busy,
                    )
                });
                (Arc::new(next), latest)
            }
        };
        let message_changed = *self
            .latest_message
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            != Some(latest);
        let activity = {
            let mut cached = self.task_activity.lock().unwrap_or_else(|e| e.into_inner());
            if cached.as_ref().is_none_or(|(cursor, _)| *cursor != latest) {
                let activity = measured("task activity", || taskview::activity(store))
                    .map_err(|e| e.to_string())?;
                *cached = Some((latest, activity));
            }
            cached.as_ref().expect("activity was read").1.clone()
        };
        // Read before anything moves, so a failed read leaves the cursor where it was.
        let asks = measured("task asks", || self.read_asks(store, latest, &next.tasks))?;
        let mut held = self.tasks.lock().unwrap_or_else(|e| e.into_inner());
        let mut changed = Vec::new();
        let pending = self
            .pages
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get("")
            .map(|p| p.pending.clone());
        if let Some(pending) = pending {
            let mut previous = self
                .signoff_pending
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            // Counted before the read, so an answer that comes back during it reads it again.
            let answered = self.gates.answered();
            if message_changed
                || previous
                    .as_ref()
                    .is_none_or(|(cards, then)| *cards != pending || *then != answered)
                || held
                    .last
                    .as_ref()
                    .is_none_or(|old| !tasks_alike(&old.tasks, &next.tasks))
            {
                let lane = measured("signoff", || {
                    taskview::signoff(store, &next.tasks, &pending, &self.root, &self.gates)
                })?;
                let signoff: Vec<String> = lane.iter().map(|(slug, _)| slug.clone()).collect();
                let mut old = self.task_signoff.lock().unwrap_or_else(|e| e.into_inner());
                if *old != signoff {
                    changed.push(region_event("task-signoff", json!(signoff)));
                    *old = signoff;
                }
                // A lane that moved moves what its tasks wait on: the board's snapshot again, over
                // what was read of it already.
                let lane = {
                    let mut mapped = BTreeMap::new();
                    for (slug, cards) in lane {
                        mapped.insert(slug, cards);
                    }
                    mapped
                };
                let mut waiting = self.waiting.lock().unwrap_or_else(|e| e.into_inner());
                if waiting.lane != lane {
                    waiting.lane = lane;
                    let (again, _) = measured("board snapshot", || {
                        taskview::snapshot_board_with_busy(
                            &snapshot,
                            &waiting.stood,
                            &waiting.lane,
                            &busy,
                        )
                    });
                    next = Arc::new(again);
                }
                drop(waiting);
                *previous = Some((pending, answered));
            }
        }
        let mut cursor = self
            .latest_message
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut lines = Vec::new();
        if let Some(since) = *cursor
            && since < latest
        {
            let render = symphony::render::Render::new(&self.root);
            lines = store
                .messages_after(since, latest)
                .map_err(|e| e.to_string())?;
            for message in &lines {
                let record = taskview::message(message);
                let mut data = json!(record);
                let entry = taskview::entry(&record, &render);
                data["blocks"] = json!(entry.blocks);
                data["stamp"] = entry.stamp.into();
                changed.push(region_event("message", data));
            }
        }
        *cursor = Some(latest);
        drop(cursor);
        if let Some(change) = self.apply_asks(asks, &lines) {
            changed.push(change);
        }
        // A failed read keeps the recipient known before; it is no change.
        if let Ok(recipient) = store.chat_recipient()
            && let Some(change) = self.chat_read(recipient)
        {
            changed.push(change);
        }
        if held.error.is_some() {
            changed.push(region_event("tasks-status", json!({"available":true})));
        }
        // A failed read keeps the generation known before; it is no change.
        if let Ok(active) = store.workflow_active()
            && let Some(change) = self.workflow_read(active.map(|a| a.generation))
        {
            changed.push(change);
        }
        // A document rendered while the board was not read yet, or could not be read, lists none of
        // its tasks, however little the board moved since.
        let relinked = held.error.is_some()
            || held
                .last
                .as_ref()
                .is_none_or(|old| !Arc::ptr_eq(old, &next) && relinks(&old.tasks, &next.tasks));
        if let Some(old) = &held.last {
            task_changes(&mut changed, &old.tasks, &next.tasks);
            // A holder's health is no task's change: it moves no column's count.
            if !tasks_alike(&old.tasks, &next.tasks) {
                let mut counts = BTreeMap::<&str, usize>::new();
                for status in symphony_board::STATUSES {
                    counts.insert(status, 0);
                }
                for task in symphony_api::tasks(&next.tasks) {
                    *counts.entry(&task.status).or_default() += 1;
                }
                changed.push(region_event(
                    "task-counts",
                    json!(counts.into_iter().collect::<Vec<_>>()),
                ));
                let recent = |days: f64| {
                    let mut counts = BTreeMap::<&str, usize>::new();
                    for task in symphony_api::tasks(&next.tasks) {
                        if task.is_finished() && task.updated >= store.now() - days * 86_400.0 {
                            *counts.entry(task.status.as_str()).or_default() += 1;
                        }
                    }
                    for status in ["done", "cancelled"] {
                        counts.entry(status).or_default();
                    }
                    counts.into_iter().collect::<Vec<_>>()
                };
                changed.push(region_event(
                    "task-recent-counts",
                    json!({"day":recent(1.0),"week":recent(7.0)}),
                ));
                // A task closed or gone holds no page's room for its gates any more.
                let open: BTreeSet<&str> = next
                    .tasks
                    .iter()
                    .filter(|task| !task.is_finished())
                    .map(|task| task.slug.as_str())
                    .collect();
                self.gates.keep_open(|slug| open.contains(slug));
            }
            // Heartbeats are not displayed. Sending last_seen/last_tool on every beat makes every
            // active worker a stream of pointless events. Sessions change when displayed state does,
            // and a session's health is its roles' too: the client sets them from this change.
            diff_shown(
                &mut changed,
                "session",
                &old.sessions,
                &next.sessions,
                |s| s.handle.clone(),
                |s| {
                    s.last_seen = 0.0;
                    s.last_tool = None;
                },
            );
        }
        held.last = Some(next.clone());
        held.raw = Some(snapshot);
        held.busy = busy;
        held.error = None;
        self.publish_listing(Ok(Arc::clone(&next)));
        drop(held);
        let mut old_agents = self.agents.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(old) = &*old_agents {
            // No page shows when an agent last called a tool: a tool call alone sends nothing.
            diff_shown(
                &mut changed,
                "agent",
                &old.agents,
                &agents.agents,
                |a| a.handle.clone(),
                |a| a.last_tool_call = None,
            );
            if old.probes != agents.probes {
                changed.push(region_event("agent-probes", json!(agents.probes)));
            }
            if old.machine_work != agents.machine_work {
                changed.push(region_event("machine-work", json!(agents.machine_work)));
            }
        }
        *old_agents = Some(agents);
        drop(old_agents);
        let mut rendered = if relinked {
            measured("relinked documents", || self.render_relinked(&next.tasks))
        } else {
            BTreeMap::new()
        };
        let mut pages = self.pages.write().unwrap_or_else(|e| e.into_inner());
        for (scope, page) in pages.iter_mut() {
            let mut changes = changed.clone();
            if let Some(rendered) = rendered.remove(scope) {
                place(page, rendered, &mut changes);
            }
            let counts = Some(taskview::counts(&next));
            if page.board.counts.tasks != counts {
                page.board.counts.tasks = counts;
                changes.push(region_event("counts", json!(page.board.counts)));
            }
            let mut combined = Activity {
                items: page
                    .activity
                    .items
                    .iter()
                    .filter(|i| i.kind != "task message")
                    .cloned()
                    .collect(),
            };
            combined.items.extend(activity.iter().cloned());
            sort_activity(&mut combined);
            diff_items(
                &mut changes,
                "activity",
                &page.activity.items,
                &combined.items,
                |i| i.id.clone(),
            );
            page.activity = combined;
            self.publish(page, changes);
        }
        Ok(())
    }

    /// What the watcher follows: main's inputs, every tree's documents, models and concepts, and
    /// the refs and worktree records in the common folder.
    fn watch_paths(&self) -> Vec<(PathBuf, RecursiveMode)> {
        let trees: Vec<_> = self
            .trees
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .cloned()
            .collect();
        self.inputs(&trees)
            .into_iter()
            .map(|(base, below, mode)| (crate::backstop::joined(&base, &below), mode))
            .collect()
    }

    /// What the backstop sweeps: the watched paths of main and of the trees with a page. An
    /// unserved tree's documents feed only main's branch cards, which every refresh rereads.
    fn swept_paths(&self) -> Vec<crate::backstop::Root> {
        let served: Vec<_> = self
            .pages
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .filter(|scope| !scope.is_empty())
            .filter_map(|scope| self.tree(scope))
            .collect();
        self.inputs(&served)
    }

    /// Report the files that changed since the last sweep through the watcher's own path, as a
    /// lost event would have. True when one of them is a review input, so a refresh is due.
    fn sweep(&self) -> bool {
        let roots = self.swept_paths();
        let missed = self
            .swept
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .next(&roots);
        if missed.is_empty() {
            return false;
        }
        let mut event = notify::Event::new(notify::EventKind::Any);
        event.paths = missed.into_iter().collect();
        self.record_event(Ok(event))
    }

    /// Each input as the folder it belongs to (main, a tree, the client build, the common folder)
    /// and its path below that folder.
    fn inputs(&self, trees: &[PathBuf]) -> Vec<crate::backstop::Root> {
        let mut paths: Vec<_> = tree_inputs(&self.root)
            .into_iter()
            .chain([("build".to_string(), RecursiveMode::NonRecursive)])
            .map(|(below, mode)| (self.root.clone(), below, mode))
            .collect();
        paths.push((
            self.ui_dist.parent().unwrap_or(&self.root).to_path_buf(),
            String::new(),
            RecursiveMode::Recursive,
        ));
        paths.extend(trees.iter().flat_map(|root| {
            tree_inputs(root)
                .into_iter()
                .map(|(below, mode)| (root.clone(), below, mode))
        }));
        if let Some(common) = &self.common {
            paths.extend(
                [
                    ("", RecursiveMode::NonRecursive),
                    ("refs", RecursiveMode::Recursive),
                    ("worktrees", RecursiveMode::Recursive),
                ]
                .map(|(below, mode)| (common.clone(), below.to_string(), mode)),
            );
        }
        paths
    }

    fn watch(&self, watcher: &mut impl Watcher, watched: &mut BTreeSet<PathBuf>) {
        let paths = self.watch_paths();
        let removed: Vec<_> = watched
            .iter()
            .filter(|path| !path.is_dir())
            .cloned()
            .collect();
        let mut next = watched.clone();
        for path in &removed {
            next.remove(path);
        }
        let added: Vec<_> = paths
            .into_iter()
            .filter(|(path, _)| path.is_dir() && next.insert(path.clone()))
            .collect();
        if removed.is_empty() && added.is_empty() {
            return;
        }
        // FSEvents restarts its stream when applying paths. Commit the whole update once,
        // including at startup, and leave an unchanged watcher running.
        let mut update = watcher.paths_mut();
        for path in removed {
            let _ = update.remove(&path);
            watched.remove(&path);
        }
        for (path, mode) in added {
            match update.add(&path, mode) {
                Ok(()) => {
                    watched.insert(path);
                }
                Err(e) => eprintln!("· review watcher: {}: {e}", path.display()),
            }
        }
        if let Err(e) = update.commit() {
            eprintln!("· review watcher commit: {e}");
            watched.clear();
        }
    }
}

/// Of `roots`, the checkout a changed path belongs to when it is below a path its project's plugins
/// watch; the deepest, as a worktree may sit inside another checkout.
fn watched_root<'a>(roots: &'a [PathBuf], path: &Path) -> Option<&'a Path> {
    roots
        .iter()
        .filter(|root| {
            watched(root)
                .iter()
                .any(|input| path.starts_with(root.join(input)))
        })
        .max_by_key(|root| root.components().count())
        .map(PathBuf::as_path)
}

/// The paths below the checkout at `root` its project's plugins' watches read.
fn watched(root: &Path) -> Vec<String> {
    let mut paths: Vec<String> = symphony::plugins::Plugins::of(root)
        .watches()
        .into_iter()
        .flat_map(|watch| watch.paths())
        .collect();
    paths.sort();
    paths.dedup();
    paths
}

fn scope_affected(
    previous: Option<&Path>,
    root: &Path,
    dirty: Option<&BTreeSet<PathBuf>>,
    main: &Path,
    ui: &Path,
) -> bool {
    // A topology/ref event can move a scope without producing any document event at its new path.
    previous != Some(root)
        || dirty.is_none_or(|paths| {
            paths
                .iter()
                .any(|p| p == main || p.starts_with(root) || p.starts_with(ui))
        })
}

/// The branches whose refs `dirty` names in the repository whose common folder is `common`, by
/// name (`tools/x`), each once: the tips that moved. A packed ref, a lock or a full refresh names
/// none.
fn moved_branches(common: Option<&Path>, dirty: Option<&BTreeSet<PathBuf>>) -> Vec<String> {
    let (Some(common), Some(dirty)) = (common, dirty) else {
        return Vec::new();
    };
    let heads = common.join("refs").join("heads");
    let mut moved: Vec<String> = dirty
        .iter()
        .filter_map(|path| path.strip_prefix(&heads).ok())
        .filter(|name| name.extension().is_none_or(|ext| ext != "lock"))
        .filter_map(|name| {
            let parts: Option<Vec<&str>> = name.iter().map(|part| part.to_str()).collect();
            parts.filter(|parts| !parts.is_empty()).map(|p| p.join("/"))
        })
        .collect();
    moved.sort();
    moved.dedup();
    moved
}

fn relevant(common: Option<&Path>, path: &Path) -> bool {
    let Some(common) = common else {
        return true;
    };
    let Ok(rel) = path.strip_prefix(common) else {
        return true;
    };
    let parts = rel.iter().collect::<Vec<_>>();
    match parts.first().and_then(|p| p.to_str()) {
        Some("refs") => !path.to_string_lossy().ends_with(".lock"),
        Some("packed-refs") => true,
        // Index locks, reflogs and heartbeat writes cannot change a review document or tree.
        Some("worktrees") => {
            let checkout = parts.len() == 3 && matches!(parts[2].to_str(), Some("HEAD" | "gitdir"));
            parts.len() <= 2 || checkout
        }
        _ => false,
    }
}

/// What the checkout at `root` adds to the watched and swept paths: its entrypoints' folders and
/// what its plugins watch outside them. An entrypoint at the project's root is the empty path.
fn tree_inputs(root: &Path) -> Vec<(String, RecursiveMode)> {
    let project = symphony::project::Project::of(root);
    let mut folders: Vec<String> = Vec::new();
    let mut by_depth: Vec<&String> = project.entrypoints.iter().map(|e| &e.folder).collect();
    symphony_board::order::sort_by_key(&mut by_depth, &|folder| folder.len());
    for folder in by_depth {
        // A folder inside one already watched recursively is watched with it.
        if !folders
            .iter()
            .any(|held| Path::new(folder).starts_with(held))
        {
            folders.push(folder.clone());
        }
    }
    let outside: Vec<String> = watched(root)
        .into_iter()
        .filter(|path| {
            !folders
                .iter()
                .any(|folder| Path::new(path).starts_with(folder))
        })
        .collect();
    folders
        .into_iter()
        .chain(outside)
        .map(|below| (below, RecursiveMode::Recursive))
        .collect()
}

/// A followed document rendered again ([`Hub::render_again`]): its path, the read it came from
/// ([`Hub::reads`]) and what the render made of it.
type Rendered = (String, u64, Result<symphony_api::Document, String>);

/// The documents a page follows and is not holding as missing, each with the tasks its copy lists
/// ([`Hub::render_relinked`]).
type Followed = Vec<(String, Vec<symphony_api::DocumentTask>)>;

/// Place each of `rendered` in `page`, unless a render begun after it placed its copy first, and
/// add to `changes` what each changes on the page's open documents.
fn place(page: &mut Page, rendered: Vec<Rendered>, changes: &mut Vec<Value>) {
    for (rel, read, rendered) in rendered {
        // A reader's render begun after this one placed the newer copy meanwhile.
        if page.copied.get(&rel).is_some_and(|copied| *copied > read) {
            continue;
        }
        page.copied.insert(rel.clone(), read);
        match rendered {
            Ok(doc) => {
                if let Some(delta) = document_change(page, &rel, &doc) {
                    changes.push(region_event("document", json!(delta)));
                }
                page.documents.insert(rel, doc);
            }
            // A page made again since its documents were listed follows none of them.
            Err(_) => {
                if let Some(doc) = page.documents.get(&rel)
                    && page.missing.insert(rel.clone())
                {
                    changes.push(region_event(
                        "document",
                        json!(symphony_api::DocumentChange {
                            path: doc.path.clone(),
                            removed: true,
                            ..Default::default()
                        }),
                    ));
                }
            }
        }
    }
}

/// Whether a document's tasks can read otherwise on `new` than on `old`
/// ([`symphony::docview::linked`]): a task came or went, or moved its slug, title, status or links.
fn relinks(old: &[symphony_api::Task], new: &[symphony_api::Task]) -> bool {
    old.len() != new.len()
        || old.iter().zip(new).any(|(a, b)| {
            (&a.slug, &a.title, &a.status, &a.links) != (&b.slug, &b.title, &b.status, &b.links)
        })
}

fn document_change(
    page: &mut Page,
    rel: &str,
    doc: &symphony_api::Document,
) -> Option<symphony_api::DocumentChange> {
    if page.missing.remove(rel) {
        // Some editors remove a file before putting its replacement in place. Keep the subscription
        // while it is absent, and restore it as soon as that path exists again.
        return Some(symphony_api::DocumentChange {
            path: doc.path.clone(),
            sections: doc.sections.clone(),
            order: Some(
                doc.sections
                    .iter()
                    .map(|section| section.key.clone())
                    .collect(),
            ),
            threads: {
                let mut threads = BTreeMap::new();
                for (slug, fragment) in &doc.threads {
                    threads.insert(slug.clone(), Some(fragment.clone()));
                }
                threads
            },
            title: Some(doc.title.clone()),
            waiting: Some(doc.waiting.clone()),
            crumbs: Some(doc.crumbs.clone()),
            headings: Some(doc.headings.clone()),
            tasks: Some(doc.tasks.clone()),
            comparison: Some(doc.comparison.clone().into_iter().collect()),
            cards: doc
                .cards
                .iter()
                .map(|card| symphony_api::ItemChange {
                    key: card.slug.clone(),
                    value: Some(card.clone()),
                    removed: false,
                })
                .collect(),
            ..Default::default()
        });
    }
    page.documents
        .get(rel)
        .and_then(|before| symphony::docview::changes(before, doc))
}

/// Tell the documents' loop that the ticks ended, including during unwinding. The flag is set
/// before the wake, so the loop sees it as soon as its wait ends.
struct TicksEnded<'a> {
    ended: &'a std::sync::atomic::AtomicBool,
    documents: &'a Waker,
}

impl Drop for TicksEnded<'_> {
    fn drop(&mut self) {
        self.ended.store(true, std::sync::atomic::Ordering::SeqCst);
        self.documents.wake();
    }
}

/// The hub's two loops once its first board read is done: the board's ticks on a thread of their
/// own, so a document refresh never holds a tick back nor a tick a refresh, and the documents'
/// refreshes on this one, waiting on `backstop`, which the watcher's handler wakes. It returns once
/// both have ended: the ticks when the hub stops, goes or loses its project; the documents when
/// the hub drains or goes, or once the ticks have ended, whose end wakes them, so a project lost
/// ends both.
fn run(
    weak: Weak<Hub>,
    watcher: impl Watcher,
    watched: BTreeSet<PathBuf>,
    backstop: Backstop,
    board: Option<symphony_board::Store>,
    connect: impl FnMut(&Path) -> Result<symphony_board::Store, String> + Send,
) {
    let (stop, stopped) = mpsc::channel();
    {
        let Some(hub) = weak.upgrade() else { return };
        let _ = hub.documents_waker.set(backstop.waker());
        let mut held = hub.ticks.lock().unwrap_or_else(|e| e.into_inner());
        // A stop sets `draining` before it takes what this holds, so one already past its take is
        // seen here, and neither loop starts.
        if *hub.draining.borrow() {
            return;
        }
        *held = Some(stop);
    }
    let weak = &weak;
    let ended = std::sync::atomic::AtomicBool::new(false);
    let ended = &ended;
    let waker = backstop.waker();
    std::thread::scope(|scope| {
        scope.spawn(move || {
            let _ended = TicksEnded {
                ended,
                documents: &waker,
            };
            ticks(weak, &stopped, &waker, board, connect);
        });
        documents(weak, watcher, watched, backstop, || {
            ended.load(std::sync::atomic::Ordering::SeqCst)
        });
    });
}

/// The board's ticks, one an interval, independent of client count: the board read (the chat
/// journal, callbacks, the give, the agents probe), the queue's expired postponements (with backoff
/// on store errors), the hourly roll-up, the asks file, the project's settings, and a look for the
/// pictures held cards wait on, which wakes the documents' loop by `documents` once one landed.
/// Between two it waits on `stopped`, which the hub's stop and its drop end at once, the first
/// tick right away.
fn ticks(
    weak: &Weak<Hub>,
    stopped: &mpsc::Receiver<()>,
    documents: &Waker,
    mut board: Option<symphony_board::Store>,
    mut connect: impl FnMut(&Path) -> Result<symphony_board::Store, String>,
) {
    let mut expiry = OrderExpiry::default();
    let mut wait = Duration::ZERO;
    while let Err(mpsc::RecvTimeoutError::Timeout) = stopped.recv_timeout(wait) {
        let started = Instant::now();
        let Some(hub) = weak.upgrade() else { break };
        // A project removed under the server: nothing more is read or recorded for it.
        if !hub.holds_its_project() {
            hub.lose();
            break;
        }
        expiry.tick(&hub.root, symphony::postpone::now(), || {
            hub.refresh_order("")
        });
        hub.board_tick(&mut board, &mut connect);
        measured("reports tick", || hub.reports_tick(board.as_ref()));
        measured("asks tick", || hub.asks_tick());
        measured("project tick", || hub.tell_project());
        if hub.pictures_arrived() {
            documents.wake();
        }
        wait = BOARD_INTERVAL.saturating_sub(started.elapsed());
    }
}

/// The documents' refreshes, from watcher events, the backstop's sweep and the rescans the ticks and
/// the reads ask for, until the hub drains or goes, or `ticks_ended` says the ticks have ended. Each
/// is a wake of `backstop` or its period: nothing else ends its wait.
fn documents(
    weak: &Weak<Hub>,
    mut watcher: impl Watcher,
    mut watched: BTreeSet<PathBuf>,
    mut backstop: Backstop,
    ticks_ended: impl Fn() -> bool,
) {
    // Whether the loop is over: the hub gone or draining, or its ticks ended.
    let over = || {
        weak.upgrade()
            .is_none_or(|hub| *hub.draining.borrow() || ticks_ended())
    };
    loop {
        let wake = backstop.wait();
        if wake == Wake::Event && !over() {
            // Every event within QUIET of the first is taken with it, and the pass reads then:
            // never later than that, as the watched worktrees seldom go quiet and every live
            // page's update waits on this pass.
            backstop.debounce(backstop::QUIET, backstop::QUIET);
        }
        let Some(hub) = weak.upgrade() else { break };
        if *hub.draining.borrow() || ticks_ended() {
            break;
        }
        // A task finished or came back since the last scan: its branch's cards come or go. A
        // picture a held card waits on landed: the card is posted.
        let rescan =
            hub.rescan.swap(false, std::sync::atomic::Ordering::SeqCst) || hub.pictures_arrived();
        let refresh = wake == Wake::Event
            || (wake == Wake::Period && measured("document sweep", || hub.sweep()))
            || rescan;
        if refresh {
            let dirty = std::mem::take(&mut *hub.dirty.lock().unwrap_or_else(|e| e.into_inner()));
            hub.swept
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .restamp(&dirty);
            if let Err(e) = measured("document event", || hub.refresh_docs(Some(&dirty))) {
                eprintln!("· Symphony data refresh failed: {e}");
            }
            if let Some(then) = hub.refreshed.get() {
                then();
            }
        }
        hub.watch(&mut watcher, &mut watched);
    }
}

fn sort_activity(activity: &mut Activity) {
    symphony_board::order::sort_by(&mut activity.items, &|a, b| {
        (&b.ts, &b.id).cmp(&(&a.ts, &a.id))
    });
    activity.items.truncate(40);
}

fn board_changes(old: &Board, new: &Board) -> Vec<Value> {
    let mut changes = Vec::new();
    let keys = |b: &Board| {
        b.next
            .iter()
            .map(|c| (c.doc.clone(), c.slug.clone()))
            .collect::<Vec<_>>()
    };
    if keys(old) != keys(new) {
        changes.push(region_event("queue", json!(new.next)));
    } else {
        for (before, after) in old.next.iter().zip(&new.next) {
            if before != after {
                changes.push(region_event("card", json!(after)));
            }
        }
    }
    if old.counts != new.counts {
        changes.push(region_event("counts", json!(new.counts)));
    }
    if old.progress != new.progress {
        changes.push(region_event("progress", json!(new.progress)));
    }
    if old.build != new.build {
        changes.push(region_event("client", json!({"build":new.build})));
    }
    changes
}

/// Item positions in lexical key order, retaining the last position with a duplicate key.
fn keyed<T>(items: &[T], key: impl Fn(&T) -> String) -> BTreeMap<String, usize> {
    let mut mapped = BTreeMap::new();
    // Direct insertion keeps the last duplicate and sorts by key, without collecting a
    // temporary vector and linking a separate stable sorter for each wire item type.
    // Positions also share one map implementation across item types; callers still borrow items.
    for (index, item) in items.iter().enumerate() {
        mapped.insert(key(item), index);
    }
    mapped
}

/// Feed regions all use the same ordered envelope, regardless of their payload's wire type.
#[inline(never)]
fn region_event(region: &str, data: Value) -> Value {
    let mut event = serde_json::Map::new();
    event.insert("region".into(), Value::from(region));
    event.insert("data".into(), data);
    Value::Object(event)
}

/// The keyed feed envelope is independent of the serialized item's type. Keep its ordered
/// object insertion in one place instead of linking it for every compared list type.
#[inline(never)]
fn keyed_event(region: &str, key: &str, field: &str, value: Value) -> Value {
    let mut data = serde_json::Map::new();
    data.insert("key".into(), Value::from(key));
    data.insert(field.into(), value);
    region_event(region, Value::Object(data))
}

/// List regions carry keyed upserts/removals, never a copy of the task board or the entire queue.
/// `order` is sent only when membership/order changes, alongside those changed records.
fn diff_items<T: serde::Serialize + PartialEq>(
    changes: &mut Vec<Value>,
    region: &str,
    old: &[T],
    new: &[T],
    key: impl Fn(&T) -> String,
) {
    if old == new {
        return;
    }
    let before = keyed(old, &key);
    let after = keyed(new, &key);
    for (id, index) in &after {
        let item = &new[*index];
        if before.get(id).map(|index| &old[*index]) != Some(item) {
            changes.push(keyed_event(region, id, "value", json!(item)));
        }
    }
    for id in before.keys().filter(|id| !after.contains_key(*id)) {
        changes.push(keyed_event(region, id, "removed", Value::Bool(true)));
    }
    let old_keys: Vec<_> = old.iter().map(&key).collect();
    let new_keys: Vec<_> = new.iter().map(&key).collect();
    for (index, id) in new_keys.iter().enumerate() {
        let after = index.checked_sub(1).map(|i| &new_keys[i]);
        let old_after = old_keys
            .iter()
            .position(|key| key == id)
            .and_then(|i| i.checked_sub(1))
            .map(|i| &old_keys[i]);
        if !before.contains_key(id) || after != old_after {
            changes.push(keyed_event(
                &format!("{region}-order"),
                id,
                "after",
                Value::from(after.map(String::as_str)),
            ));
        }
    }
}

/// List changes of `region` by what a page shows of each item: two items alike once `hide` has
/// cleared what no page draws send nothing, and an item that changed is sent as it is now, whole.
fn diff_shown<T: serde::Serialize + PartialEq + Clone>(
    changes: &mut Vec<Value>,
    region: &str,
    old: &[T],
    new: &[T],
    key: impl Fn(&T) -> String,
    hide: impl Fn(&mut T),
) {
    let shown = |items: &[T]| {
        items
            .iter()
            .map(|item| {
                let mut item = item.clone();
                hide(&mut item);
                item
            })
            .collect::<Vec<_>>()
    };
    let at = changes.len();
    diff_items(changes, region, &shown(old), &shown(new), &key);
    let now = keyed(new, key);
    for event in &mut changes[at..] {
        if event["region"] == region
            && event["data"].get("value").is_some()
            && let Some(item) = event["data"]["key"]
                .as_str()
                .and_then(|key| now.get(key))
                .map(|index| &new[*index])
        {
            event["data"]["value"] = json!(item);
        }
    }
}

/// A task as the feed compares it: without its holders' health, which each holder's `session`
/// change says once, rather than every task it ever held saying it again.
#[derive(serde::Serialize)]
#[serde(transparent)]
struct Shown<'a>(&'a symphony_api::Task);

impl PartialEq for Shown<'_> {
    fn eq(&self, other: &Self) -> bool {
        shown_alike(self.0, other.0)
    }
}

/// Whether `a` and `b` are the same task, or differ only in their holders' health. Every field is
/// named, so a new one is compared or the build fails.
fn shown_alike(a: &symphony_api::Task, b: &symphony_api::Task) -> bool {
    task_inputs_alike(a, b) && a.waits_on == b.waits_on
}

/// Task inputs used by sign-off and column counts. A health-derived waits line is sent to the
/// client, but changes none of these inputs.
#[expect(clippy::float_cmp, reason = "the task's own derived equality")]
fn task_inputs_alike(a: &symphony_api::Task, b: &symphony_api::Task) -> bool {
    let symphony_api::Task {
        slug,
        title,
        body,
        status,
        priority,
        parent,
        created_by,
        created,
        updated,
        links,
        depends,
        roles,
        instruction,
        fields,
        waits_on: _,
        review_rounds,
        from,
        workflow,
    } = a;
    *slug == b.slug
        && *title == b.title
        && *body == b.body
        && *status == b.status
        && *priority == b.priority
        && *parent == b.parent
        && *created_by == b.created_by
        && *created == b.created
        && *updated == b.updated
        && *links == b.links
        && *depends == b.depends
        && *instruction == b.instruction
        && *fields == b.fields
        && *review_rounds == b.review_rounds
        && *from == b.from
        && *workflow == b.workflow
        && roles_alike(roles, &b.roles)
}

/// Whether `a` and `b` are the same roles, their holders' health aside.
#[expect(clippy::float_cmp, reason = "the role's own derived equality")]
fn roles_alike(a: &[symphony_api::Role], b: &[symphony_api::Role]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| {
            let symphony_api::Role {
                handle,
                role,
                since,
                expires,
                health: _,
            } = a;
            *handle == b.handle && *role == b.role && *since == b.since && *expires == b.expires
        })
}

/// Whether the board's tasks are the same in `old` and `new`, their holders' health aside.
fn tasks_alike(old: &[symphony_api::Task], new: &[symphony_api::Task]) -> bool {
    old.len() == new.len() && old.iter().zip(new).all(|(a, b)| task_inputs_alike(a, b))
}

fn task_changes(changes: &mut Vec<Value>, old: &[symphony_api::Task], new: &[symphony_api::Task]) {
    // Insertions/removals use the same keyed list protocol. An existing task is sent once, as the
    // fields that changed, never its whole record again: a status change would retransmit a long
    // brief, and a client that does not hold the task reads it with the view that shows it.
    let (shown_old, shown_new): (Vec<_>, Vec<_>) = (
        old.iter().map(Shown).collect(),
        new.iter().map(Shown).collect(),
    );
    let mut basic = Vec::new();
    diff_items(&mut basic, "task", &shown_old, &shown_new, |t| {
        t.0.slug.clone()
    });
    for event in basic {
        if let Some(key) = event["data"]["key"].as_str()
            && event["region"] == "task"
            && let Some(before) = old.iter().find(|t| t.slug == key)
            && let Some(after) = new.iter().find(|t| t.slug == key)
        {
            // Optional summaries distinguish unavailable from zero. A whole record clears one.
            if before.review_rounds.is_some() && after.review_rounds.is_none() {
                changes.push(event);
                continue;
            }
            let mut fields = symphony_api::TaskFields::default();
            macro_rules! field {
                ($name:ident) => {
                    if before.$name != after.$name {
                        fields.$name = Some(after.$name.clone());
                    }
                };
            }
            field!(title);
            field!(body);
            field!(status);
            field!(priority);
            field!(updated);
            field!(links);
            field!(depends);
            field!(instruction);
            if before.review_rounds != after.review_rounds {
                fields.review_rounds = after.review_rounds;
            }
            // Roles that changed say their holders' health as it is now.
            if !roles_alike(&before.roles, &after.roles) {
                fields.roles = Some(after.roles.clone());
            }
            if before.parent != after.parent {
                fields.parent = Some(after.parent.clone().unwrap_or_default());
            }
            // What it waits on, which a holder's health can change too; none is one of no kind.
            if before.waits_on != after.waits_on {
                fields.waits_on = Some(after.waits_on.clone().unwrap_or_default());
            }
            if before.from != after.from {
                fields.from = Some(after.from.clone().unwrap_or_default());
            }
            // A place in a custom workflow is never taken away: only a new one is sent.
            if before.workflow != after.workflow {
                fields.workflow.clone_from(&after.workflow);
            }
            // A change of what no field carries (a plugin's board fields, which no page draws)
            // sends nothing.
            if fields != symphony_api::TaskFields::default() {
                changes.push(region_event(
                    "task-fields",
                    json!(symphony_api::TaskChange {
                        key: key.into(),
                        fields
                    }),
                ));
            }
        } else {
            changes.push(event);
        }
    }
}

pub(crate) mod agent_activity;
mod asks;
mod board_status;
mod health;
mod machine_work;

pub(crate) use health::{Signal, Standing};
mod decided;
mod owner_asks;
mod presence;
#[cfg(test)]
mod presence_tests;
#[cfg(test)]
mod readiness_tests;
mod reports;
mod retry;
#[cfg(test)]
mod retry_tests;
mod startup;
mod task_asks;

#[cfg(test)]
mod decided_tests;
#[cfg(test)]
mod owner_asks_tests;

#[cfg(test)]
mod gallery_tests;

#[cfg(test)]
mod watch_tests;

#[cfg(test)]
mod postpone_tests;

#[cfg(test)]
mod chat_tests;

#[cfg(test)]
mod compatible_ahead_tests;

#[cfg(test)]
mod backstop_tests;

#[cfg(test)]
mod startup_tests;

#[cfg(test)]
mod boards_tests;

#[cfg(test)]
mod workflow_tests;

#[cfg(test)]
mod status_tests;

#[cfg(test)]
mod health_tests;

#[cfg(test)]
mod no_documents_tests;

#[cfg(test)]
mod checkout_tests;

#[cfg(test)]
mod read_paths_tests;

#[cfg(test)]
mod refresh_tests;

#[cfg(test)]
mod outline_check_tests;

#[cfg(test)]
mod relinked_tests;

#[cfg(test)]
mod feed_deltas_tests;

#[cfg(test)]
mod session_go_tests;

#[cfg(test)]
mod gates_tests;

#[cfg(test)]
mod timeline_tests;

#[cfg(test)]
mod task_diff_tests;

#[cfg(test)]
mod keyed_tests;

#[cfg(test)]
mod reports_tests;

#[cfg(test)]
mod ticks_tests;

#[cfg(test)]
mod tests {
    use super::{Hub, Sweep, moved_branches, relevant, scope_affected};
    use crate::testing::{
        Tree,
        board::{Fixture, Seed},
        git, repository,
    };
    use serde_json::{Value, json};
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    #[test]
    fn a_timing_line_names_its_stage_with_its_wall_and_cpu_time_or_says_cpu_is_unavailable() {
        let unread = super::Stage {
            began: Instant::now() - Duration::from_millis(40),
            cpu: None,
        };
        let line = unread.line("board read");
        let (wall, cpu) = line
            .strip_prefix("· review timing: board read: ")
            .and_then(|rest| rest.split_once(" ms cpu "))
            .unwrap_or_else(|| panic!("{line}"));
        assert!(wall.parse::<f64>().unwrap() >= 40.0, "{line}");
        assert_eq!(cpu, "unavailable ms");
        let line = super::Stage::now().line("board read");
        let cpu = line
            .split_once(" ms cpu ")
            .and_then(|(_, cpu)| cpu.strip_suffix(" ms"))
            .unwrap_or_else(|| panic!("{line}"));
        assert!(cpu.parse::<f64>().unwrap() >= 0.0, "{line}");
    }

    fn observe(hub: &Hub, store: &symphony_board::Store) {
        hub.update_board(
            store,
            store.board().unwrap(),
            symphony_api::Agents {
                machine_work: symphony_api::MachineWork::default(),
                generated: store.now(),
                probes: symphony_api::AgentProbes {
                    panes: "ok".into(),
                    slots: "ok".into(),
                },
                agents: Vec::new(),
            },
        )
        .unwrap();
    }

    #[test]
    fn board_ticks_keep_health_and_roles_current_with_cached_inputs_and_no_callbacks() {
        let tree = Tree::new();
        let root = repository(&tree);
        for standalone in [false, true] {
            let f = Fixture::new(
                Seed::default()
                    .session("worker")
                    .task("tools/clock", "in_progress")
                    .role("tools/clock", "worker", "executor"),
            );
            let mut hub = Hub::new(&root, &root.join("build/client"));
            if standalone {
                hub.common = None;
            }
            hub.board_read(&f.store).unwrap();
            let first = hub.task_snapshot().unwrap();
            assert_eq!(first.sessions[0].health, "live");
            f.advance(91.0);
            hub.board_read(&f.store).unwrap();
            let stale = hub.task_snapshot().unwrap();
            assert_eq!(stale.sessions[0].health, "stale");
            assert_eq!(stale.tasks[0].roles[0].health, "stale");
            assert_ne!(first.version, stale.version);
            f.store
                .raw()
                .table("task")
                .set(serde_json::json!({"title": "A changed title"}))
                .unwrap();
            hub.board_read(&f.store).unwrap();
            assert_eq!(
                hub.task_snapshot().unwrap().tasks[0].title,
                "A changed title"
            );
        }
    }

    fn pending(hub: &Hub, scope: &str) -> Vec<String> {
        hub.pages.read().unwrap()[scope]
            .pending
            .items
            .iter()
            .map(|item| item.slug.clone())
            .collect()
    }

    #[test]
    fn a_health_flip_updates_the_waits_line_without_signoff_reads_or_column_counts() {
        let tree = Tree::new();
        let root = repository(&tree);
        let f = Fixture::new(
            Seed::default()
                .session("worker")
                .task("tools/health", "in_progress")
                .role("tools/health", "worker", "executor")
                .link("tools/health", "doc", "docs/TOPIC.md"),
        );
        let hub = Hub::new(&root, &root.join("build/client"));
        hub.refresh_page("", &root).unwrap();
        observe(&hub, &f.store);
        // Sign-off would have to read the thread: the linked card is absent from the brief.
        assert!(hub.task_signoff.lock().unwrap().is_empty());
        let signoff_inputs = hub.signoff_pending.lock().unwrap().clone();
        f.advance(91.0);
        let board = f.store.board().unwrap();
        let before = f.store.client().requests();
        hub.update_board(&f.store, board, hub.agent_snapshot().unwrap())
            .unwrap();
        let pages = hub.pages.read().unwrap();
        let event: Value = serde_json::from_str(&pages[""].history.back().unwrap().json).unwrap();
        let regions: Vec<_> = event["changes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|change| change["region"].as_str().unwrap())
            .collect();
        eprintln!(
            "health flip: {} requests; regions: {}",
            f.store.client().requests() - before,
            regions.join(", ")
        );
        // Two small reads, and the active workflow's one, which a relabel's new generation needs
        // as it moves no task (`Hub::workflow_read`): no sign-off thread read, no column counts.
        assert_eq!(
            f.store.client().requests() - before,
            3,
            "no sign-off thread read"
        );
        assert_eq!(regions, ["task-fields", "session", "counts"]);
        assert_eq!(*hub.signoff_pending.lock().unwrap(), signoff_inputs);
        let changed = &event["changes"][0]["data"]["fields"];
        assert_eq!(changed["waits_on"]["who"], "worker (stale)");
        assert_eq!(changed.as_object().unwrap().len(), 1);
    }

    #[test]
    fn idle_hub_refreshes_only_affected_branch_pages_and_follows_a_moved_tree() {
        let tree = Tree::new();
        let root = repository(&tree);
        for branch in ["branch-a", "branch-b"] {
            git(
                &root,
                &[
                    "worktree",
                    "add",
                    "-b",
                    branch,
                    tree.path(branch).to_str().unwrap(),
                ],
            );
        }
        let a = tree.path("branch-a").canonicalize().unwrap();
        let b = tree.path("branch-b").canonicalize().unwrap();
        let hub = Hub::new(&root, &root.join("build/client"));
        hub.refresh_page("", &root).unwrap();
        hub.refresh_page("branch-a", &a).unwrap();
        hub.refresh_page("branch-b", &b).unwrap();
        assert_eq!(pending(&hub, "branch-a"), ["initial-card"]);
        assert_eq!(pending(&hub, "branch-b"), ["initial-card"]);
        hub.document("branch-a", "docs/TOPIC.md").unwrap();
        hub.document("branch-b", "docs/TOPIC.md").unwrap();
        let cached = |scope: &str| {
            hub.pages.read().unwrap()[scope].documents["docs/TOPIC.md"]
                .waiting
                .clone()
        };

        tree.write(
            "branch-a/docs/TOPIC.md",
            "# Topic\n\n- [ ] **a-new** — Changed A.\n",
        );
        tree.write(
            "branch-b/docs/TOPIC.md",
            "# Topic\n\n- [ ] **b-new** — Changed B.\n",
        );
        // B's notification has not arrived: refreshing it here would undo the idle filter.
        hub.refresh_docs(Some(&BTreeSet::from([a.join("docs/TOPIC.md")])))
            .unwrap();
        assert_eq!(cached("branch-a"), ["a-new"]);
        assert_eq!(cached("branch-b"), ["initial-card"]);
        assert_eq!(pending(&hub, "branch-b"), pending(&hub, ""));
        hub.refresh_docs(Some(&BTreeSet::from([b.join("docs/TOPIC.md")])))
            .unwrap();
        assert_eq!(cached("branch-b"), ["b-new"]);

        let moved = tree.path("moved-b");
        git(
            &root,
            &[
                "worktree",
                "move",
                b.to_str().unwrap(),
                moved.to_str().unwrap(),
            ],
        );
        tree.write(
            "moved-b/docs/TOPIC.md",
            "# Topic\n\n- [ ] **b-moved** — Moved B.\n",
        );
        let gitdir = root.join(".git/worktrees/branch-b/gitdir");
        assert!(gitdir.is_file(), "the move rewrote this topology record");
        hub.refresh_docs(Some(&BTreeSet::from([gitdir]))).unwrap();
        assert_eq!(cached("branch-b"), ["b-moved"]);
    }

    #[test]
    fn branch_pages_and_folder_pills_keep_the_global_decision_view_as_documents_and_order_change() {
        let tree = Tree::new();
        let root = repository(&tree);
        tree.write("main/docs/TOPIC.md", "# Topic\n\n<!-- register:start -->\n- [ ] **initial-card** — Initial question.\n<!-- register:end -->\n");
        git(&root, &["add", "docs/TOPIC.md"]);
        git(&root, &["commit", "-m", "Register the fixture decisions"]);
        let branch = tree.path("proposal");
        git(
            &root,
            &[
                "worktree",
                "add",
                "-b",
                "design/policy",
                branch.to_str().unwrap(),
            ],
        );
        let inherited = std::fs::read_to_string(root.join("docs/TOPIC.md")).unwrap();
        tree.write(
            "proposal/docs/TOPIC.md",
            &inherited.replace(
                "<!-- register:end -->",
                "- [ ] **branch-card** — A branch proposal.\n<!-- register:end -->",
            ),
        );
        // the branch posts its card: a draft would reach no one
        git(&branch, &["commit", "-qam", "Post the branch card"]);
        let main = inherited.replace("[ ]", "[x]").replace(
            "<!-- register:end -->",
            "- [ ] **main-card** — Main added this after the fork.\n<!-- register:end -->",
        );
        tree.write("main/docs/TOPIC.md", &main);
        git(&root, &["add", "docs/TOPIC.md"]);
        git(&root, &["commit", "-m", "Main moves after fork"]);
        assert_eq!(
            symphony::branches::scan(&root, &Default::default())
                .iter()
                .map(|c| c.card.slug.clone())
                .collect::<Vec<_>>(),
            ["branch-card"]
        );
        let hub = Hub::new(&root, &root.join("build/client"));
        hub.refresh_docs(None).unwrap();
        hub.ensure_page("design~policy").unwrap();
        assert_eq!(pending(&hub, "design~policy"), ["main-card", "branch-card"]);
        for scope in ["", "design~policy"] {
            let folder = hub.folder(scope, "docs").unwrap();
            let entry = folder
                .entries
                .iter()
                .find(|e| e.path.ends_with("/TOPIC.md"))
                .unwrap();
            assert_eq!(entry.waiting, 2);
            let pages = hub.pages.read().unwrap();
            assert_eq!(pages[scope].board.counts.waiting, 2);
            assert_eq!(pages[scope].board.counts.docs_waiting, 1);
            assert_eq!(pages[scope].queue, pages[""].queue);
        }
        let doc = hub.document("design~policy", "docs/TOPIC.md").unwrap();
        assert!(
            doc.waiting.contains(&"initial-card".into()),
            "the branch's file still has its inherited card"
        );
        let drawn = |doc: &symphony_api::Document, slug: &str| {
            doc.cards
                .iter()
                .find(|card| card.slug == slug)
                .map(|card| card.state.clone())
        };
        assert_eq!(
            drawn(&doc, "initial-card").as_deref(),
            Some("approved"),
            "the page draws main's card, which main approved since the fork"
        );
        // A card the branch's agent is still writing is drawn as a draft (the register drawn as
        // cards, a blank line under its start).
        tree.write(
            "proposal/docs/TOPIC.md",
            &std::fs::read_to_string(branch.join("docs/TOPIC.md"))
                .unwrap()
                .replace("<!-- register:start -->\n", "<!-- register:start -->\n\n")
                .replace(
                    "<!-- register:end -->",
                    "- [ ] **drafted** — Still being written.\n  [2026-11-11 10:00] agent — Draft.\n\n<!-- register:end -->",
                ),
        );
        hub.refresh_docs(Some(&BTreeSet::from([branch.join("docs/TOPIC.md")])))
            .unwrap();
        let doc = hub.document("design~policy", "docs/TOPIC.md").unwrap();
        let section = doc
            .sections
            .iter()
            .find(|s| {
                s.blocks.iter().any(|b| {
                    serde_json::to_string(b)
                        .unwrap()
                        .contains("\"slug\":\"drafted\"")
                })
            })
            .expect("the draft is drawn");
        assert!(
            serde_json::to_string(&section.blocks)
                .unwrap()
                .contains(symphony::docview::DRAFT),
            "{:?}",
            section.blocks
        );
        assert_eq!(pending(&hub, "design~policy"), ["main-card", "branch-card"]);
        git(&branch, &["checkout", "-q", "--", "docs/TOPIC.md"]);
        hub.refresh_docs(Some(&BTreeSet::from([branch.join("docs/TOPIC.md")])))
            .unwrap();
        tree.write(
            "main/docs/TOPIC.md",
            &main.replace(
                "<!-- register:end -->",
                "- [ ] **next-main** — Another main decision.\n<!-- register:end -->",
            ),
        );
        hub.refresh_docs(Some(&BTreeSet::from([root.join("docs/TOPIC.md")])))
            .unwrap();
        assert_eq!(
            pending(&hub, "design~policy"),
            ["main-card", "next-main", "branch-card"]
        );
        assert_eq!(
            hub.pages.read().unwrap()["design~policy"]
                .board
                .counts
                .waiting,
            3
        );
        assert_eq!(
            hub.folder("design~policy", "docs")
                .unwrap()
                .entries
                .iter()
                .find(|e| e.path.ends_with("/TOPIC.md"))
                .unwrap()
                .waiting,
            3
        );
        let saves = symphony::save::Saves::start(&root);
        saves
            .post(
                "design~policy",
                "/api/postpone",
                &serde_json::json!({"path":"docs/TOPIC.md", "slug":"branch-card", "action":"later"}),
            )
            .unwrap_or_else(|e| panic!("postpone: {}", e.error));
        hub.refresh_order("design~policy").unwrap();
        for scope in ["", "design~policy"] {
            let pages = hub.pages.read().unwrap();
            assert!(
                pages[scope]
                    .queue
                    .waiting
                    .iter()
                    .find(|card| card.slug == "branch-card")
                    .unwrap()
                    .postponed
            );
            assert_eq!(pages[scope].queue, pages[""].queue);
            assert_eq!(pages[scope].board.counts.waiting, 3);
        }
        let next = saves
            .post(
                "",
                "/api/postpone",
                &serde_json::json!({"path":"docs/TOPIC.md", "slug":"main-card", "action":"later"}),
            )
            .unwrap_or_else(|e| panic!("postpone: {}", e.error));
        assert_eq!(next["next"], "/docs/TOPIC.md#next-main");
        hub.refresh_order("").unwrap();
        for scope in ["", "design~policy"] {
            let pages = hub.pages.read().unwrap();
            assert_eq!(
                pages[scope]
                    .queue
                    .waiting
                    .iter()
                    .map(|card| card.slug.as_str())
                    .collect::<Vec<_>>(),
                ["next-main", "branch-card", "main-card"]
            );
            assert_eq!(pages[scope].queue, pages[""].queue);
            assert_eq!(pages[scope].board.next, pages[scope].queue.waiting);
        }
        saves.close();
    }

    #[test]
    fn a_folder_card_says_the_queues_cards_under_it_as_its_documents_and_the_home_do() {
        let tree = Tree::new();
        // A card that waits, a question an agent answered (the owner's again, though not pending
        // in the file), and a card the owner commented on (an agent's, though still unticked).
        let root = crate::testing::repository_holding(
            &tree,
            "main",
            "docs/design/TOPIC.md",
            "# Topic\n\n<!-- register:start -->\n- [ ] **waits** — Choose.\n\
             - [?] **answered** — A question.\n  [2026-10-20 10:00] owner · question — Why?\n  [2026-10-20 11:00] agent — Because.\n\
             - [ ] **commented** — Third.\n  [2026-10-20 10:00] owner — Not this way.\n<!-- register:end -->\n",
        );
        let branch = tree.path("proposal");
        git(
            &root,
            &[
                "worktree",
                "add",
                "-b",
                "design/policy",
                branch.to_str().unwrap(),
            ],
        );
        let inherited = std::fs::read_to_string(branch.join("docs/design/TOPIC.md")).unwrap();
        tree.write(
            "proposal/docs/design/TOPIC.md",
            &inherited.replace(
                "<!-- register:end -->",
                "- [ ] **proposed** — A branch proposal.\n<!-- register:end -->",
            ),
        );
        git(&branch, &["commit", "-qam", "Post the branch card"]);
        let hub = Hub::new(&root, &root.join("build/client"));
        hub.refresh_docs(None).unwrap();
        hub.ensure_page("design~policy").unwrap();
        for scope in ["", "design~policy"] {
            let waiting = |rel: &str, path: &str| {
                let folder = hub.folder(scope, rel).unwrap();
                let found = folder.entries.iter().find(|e| e.path.ends_with(path));
                found.unwrap().waiting
            };
            let (design, topic) = (
                waiting("docs", "docs/design"),
                waiting("docs/design", "docs/design/TOPIC.md"),
            );
            let pages = hub.pages.read().unwrap();
            let mut queued: Vec<_> = pages[scope]
                .queue
                .waiting
                .iter()
                .map(|card| card.slug.as_str())
                .collect();
            queued.sort();
            assert_eq!(queued, ["answered", "proposed", "waits"], "{scope}");
            assert_eq!(
                (design, topic, pages[scope].board.progress.decisions_waiting),
                (3, 3, 3),
                "{scope}: the folder's card, its document's and the home's"
            );
        }
    }

    /// A repository whose register holds one card, and a worktree of it in `proposal` on the
    /// branch `design/policy`, whose register adds `cards`, committed there: the repository's
    /// root.
    fn proposing(tree: &Tree, cards: &str) -> PathBuf {
        let root = repository(tree);
        let register = |cards: &str| {
            format!(
                "# Topic\n\n<!-- register:start -->\n- [ ] **initial-card** — Initial question.\n{cards}<!-- register:end -->\n"
            )
        };
        tree.write("main/docs/TOPIC.md", &register(""));
        git(&root, &["commit", "-qam", "Register the fixture decisions"]);
        let branch = tree.path("proposal");
        git(
            &root,
            &[
                "worktree",
                "add",
                "-b",
                "design/policy",
                branch.to_str().unwrap(),
            ],
        );
        tree.write("proposal/docs/TOPIC.md", &register(cards));
        git(&branch, &["commit", "-qam", "Post the branch's cards"]);
        root
    }

    /// A board of `tasks`, each a slug and its status.
    fn board_of(tasks: &[(&str, &str)]) -> symphony_board::Board {
        let task = |(slug, status): &(&str, &str)| symphony_board::Task {
            branch: None,
            id: 0,
            slug: (*slug).into(),
            title: "Policy".into(),
            body: String::new(),
            status: (*status).into(),
            priority: 2,
            parent: None,
            created_by: "test".into(),
            created: 0.0,
            updated: 0.0,
            links: Vec::new(),
            depends: Vec::new(),
            roles: Vec::new(),
            instruction: false,
            fields: Default::default(),
        };
        symphony_board::Board {
            tasks: tasks.iter().map(task).collect(),
            sessions: Vec::new(),
            latest_message: 0,
        }
    }

    /// The hub of the checkout at `root`, its documents' loop on a period no test reaches
    /// ([`crate::backstop::NO_PERIOD`]) and a watcher that says nothing, so only the wakes it is
    /// given end its wait; its first board read started, which finds no board.
    fn waking(root: &Path) -> std::sync::Arc<Hub> {
        let hub = Hub::start_watching::<notify::NullWatcher>(
            root,
            &root.join("build/client"),
            false,
            false,
            crate::backstop::NO_PERIOD,
            |_| Err("no board in this test".to_string()),
        )
        .unwrap();
        hub.start_initial_board();
        hub
    }

    /// `hub` stopped, within a stall's bound: a loop the stop does not reach fails the test rather
    /// than hold it.
    fn stopped(hub: &std::sync::Arc<Hub>) {
        let hub = hub.clone();
        crate::testing::joined(
            std::thread::spawn(move || hub.stop()),
            "the stop to end the hub's loops",
            test_time::backstop(Duration::ZERO),
        );
    }

    #[test]
    fn a_task_done_takes_its_branchs_cards_out_at_the_next_scan_without_a_file_event() {
        use std::sync::atomic::Ordering::SeqCst;
        let tree = Tree::new();
        let root = proposing(&tree, "- [ ] **branch-card** — A branch proposal.\n");
        let hub = Hub::new(&root, &root.join("build/client"));
        hub.refresh_docs(None).unwrap();
        assert_eq!(pending(&hub, ""), ["initial-card", "branch-card"]);
        let board = |status: &str| board_of(&[("design/policy", status)]);
        hub.note_finished(&board("review"));
        assert!(!hub.rescan.load(SeqCst), "a live task asks for nothing");
        hub.note_finished(&board_of(&[
            ("design/policy", "review"),
            ("design/elsewhere", "done"),
        ]));
        assert!(
            !hub.rescan.load(SeqCst),
            "a task done with no worktree on its branch asks for no scan"
        );
        hub.note_finished(&board("done"));
        assert!(
            hub.rescan.swap(false, SeqCst),
            "a task done asks for a scan"
        );
        hub.refresh_docs(Some(&BTreeSet::new())).unwrap();
        assert_eq!(pending(&hub, ""), ["initial-card"]);
        hub.note_finished(&board("done"));
        assert!(
            !hub.rescan.load(SeqCst),
            "the same board asks for nothing more"
        );
        // Back in progress, its card comes back.
        hub.note_finished(&board("in_progress"));
        assert!(hub.rescan.swap(false, SeqCst));
        hub.refresh_docs(Some(&BTreeSet::new())).unwrap();
        assert_eq!(pending(&hub, ""), ["initial-card", "branch-card"]);
    }

    /// A task done asks the documents' loop for a scan and wakes it: running on a period no test
    /// reaches, with a watcher that says nothing, the loop takes the card of the task's branch out
    /// at that wake.
    #[test]
    fn a_task_done_wakes_the_documents_loop_to_take_its_branchs_cards_out() {
        let tree = Tree::new();
        let root = proposing(&tree, "- [ ] **branch-card** — A branch proposal.\n");
        let hub = waking(&root);
        assert_eq!(pending(&hub, ""), ["initial-card", "branch-card"]);
        // As a board read that finds the task done does.
        hub.note_finished(&board_of(&[("design/policy", "done")]));
        let out = test_time::until_true(
            "the done task's card to be taken out",
            test_time::backstop(Duration::from_secs(20)),
            || pending(&hub, "") == ["initial-card"],
        );
        if let Err(stall) = out {
            panic!("{stall}; still pending: {:?}", pending(&hub, ""));
        }
        stopped(&hub);
    }

    /// Each refresh of the documents calls what the hub was given for after it
    /// ([`Hub::after_refresh`]): the server's saves take up again what they keep for delivery by
    /// pull request there, which is what drops a delivered save once a land moved the checkout.
    #[test]
    fn a_refresh_of_the_documents_calls_what_was_given_for_after_it() {
        use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
        let tree = Tree::new();
        let root = proposing(&tree, "- [ ] **branch-card** — A branch proposal.\n");
        let hub = waking(&root);
        let calls = std::sync::Arc::new(AtomicUsize::new(0));
        let counted = calls.clone();
        hub.after_refresh(Box::new(move || {
            counted.fetch_add(1, SeqCst);
        }));
        // A second giving is ignored: the first stays the one called.
        hub.after_refresh(Box::new(|| panic!("given second, never called")));
        assert_eq!(calls.load(SeqCst), 0, "nothing refreshed yet");
        hub.ask_rescan();
        let called = test_time::until_true(
            "the refresh to call what was given for after it",
            test_time::backstop(Duration::from_secs(20)),
            || calls.load(SeqCst) > 0,
        );
        if let Err(stall) = called {
            panic!("{stall}");
        }
        stopped(&hub);
    }

    /// A card posted before its picture is on disk is held, and the loop posts it once the picture
    /// lands, with no watcher event and no other save: the board's tick looks at the pictures the
    /// held cards wait on and wakes the documents' loop, which runs on a period no test reaches.
    #[test]
    fn a_held_card_is_posted_when_its_picture_lands_without_another_event() {
        let tree = Tree::new();
        let root = proposing(
            &tree,
            "- [ ] **picture-card** — Pick one.\n  [2026-11-11 10:00] agent — A: ![A](/artifacts/late.png)\n",
        );
        let hub = waking(&root);
        assert_eq!(pending(&hub, ""), ["initial-card"], "held for its picture");
        assert!(!hub.pictures_arrived());
        tree.write("proposal/artifacts/late.png", "png");
        let posted = test_time::until_true(
            "the held card to be posted once its picture landed",
            test_time::backstop(Duration::from_secs(20)),
            || pending(&hub, "") == ["initial-card", "picture-card"],
        );
        if let Err(stall) = posted {
            panic!("{stall}; still held: {:?}", pending(&hub, ""));
        }
        assert!(!hub.pictures_arrived(), "nothing is awaited any more");
        stopped(&hub);
    }

    #[test]
    fn main_ref_updates_refresh_open_branch_comparisons_without_a_branch_edit() {
        let tree = Tree::new();
        let root = repository(&tree);
        let branch = tree.path("proposal");
        git(
            &root,
            &[
                "worktree",
                "add",
                "-b",
                "design/policy",
                branch.to_str().unwrap(),
            ],
        );
        let text = "# Topic\n\nBranch proposal.\n";
        tree.write("proposal/docs/TOPIC.md", text);
        let hub = Hub::new(&root, &root.join("build/client"));
        hub.refresh_docs(None).unwrap();
        hub.ensure_page("design~policy").unwrap();
        let before = hub.document("design~policy", "docs/TOPIC.md").unwrap();
        assert!(before.comparison.is_some());
        tree.write("main/docs/TOPIC.md", text);
        git(&root, &["add", "docs/TOPIC.md"]);
        git(&root, &["commit", "-m", "Main adopts proposal"]);
        hub.refresh_docs(Some(&BTreeSet::from([root.join(".git/refs/heads/main")])))
            .unwrap();
        let pages = hub.pages.read().unwrap();
        let after = &pages["design~policy"].documents["docs/TOPIC.md"];
        assert_eq!(after.comparison, None);
        assert!(after.sections.iter().all(|s| s.comparison.is_none()));
        assert!(pages["design~policy"].board.version > before.version);
    }

    #[test]
    fn idle_hub_invalidates_activity_when_a_new_owner_message_arrives() {
        let tree = Tree::new();
        let root = repository(&tree);
        let branch = tree.path("activity");
        git(
            &root,
            &[
                "worktree",
                "add",
                "-b",
                "activity",
                branch.to_str().unwrap(),
            ],
        );
        let f = Fixture::new(
            Seed::default()
                .session("worker")
                .task("tools/activity", "in_progress"),
        );
        let hub = Hub::new(&root, &root.join("build/client"));
        hub.refresh_page("", &root).unwrap();
        f.store
            .say("tools/activity", "worker", "activity-before", Some("owner"))
            .unwrap();
        observe(&hub, &f.store);
        assert!(
            hub.pages.read().unwrap()[""]
                .activity
                .items
                .iter()
                .any(|item| item.text.contains("activity-before"))
        );
        let before = hub.task_snapshot().unwrap();

        f.store
            .say("tools/activity", "worker", "activity-after", Some("owner"))
            .unwrap();
        observe(&hub, &f.store);
        assert_eq!(
            before.tasks,
            hub.task_snapshot().unwrap().tasks,
            "only the message cursor changed"
        );
        let pages = hub.pages.read().unwrap();
        let page = &pages[""];
        assert!(
            page.activity
                .items
                .iter()
                .any(|item| item.text.contains("activity-after"))
        );
        assert!(
            page.history.back().unwrap().json.contains("activity-after"),
            "the new activity must reach connected clients"
        );
        drop(pages);
        for _ in 0..2 {
            hub.refresh_page("activity", &branch).unwrap();
            let pages = hub.pages.read().unwrap();
            for text in ["activity-before", "activity-after"] {
                assert_eq!(
                    pages["activity"]
                        .activity
                        .items
                        .iter()
                        .filter(|item| item.text.contains(text))
                        .count(),
                    1,
                    "global activity must not append its previous copy of {text}"
                );
            }
        }
    }

    #[test]
    fn idle_hub_rechecks_signoff_after_a_message_without_task_or_document_changes() {
        let tree = Tree::new();
        let root = repository(&tree);
        let f = Fixture::new(
            Seed::default()
                .session("worker")
                .task("tools/signoff", "in_progress")
                .role("tools/signoff", "worker", "executor")
                .link("tools/signoff", "doc", "docs/TOPIC.md"),
        );
        let hub = Hub::new(&root, &root.join("build/client"));
        hub.refresh_page("", &root).unwrap();
        observe(&hub, &f.store);
        assert_eq!(pending(&hub, ""), ["initial-card"]);
        assert!(hub.task_signoff.lock().unwrap().is_empty());
        let before = hub.task_snapshot().unwrap();

        f.store
            .say(
                "tools/signoff",
                "worker",
                "Please answer initial-card.",
                None,
            )
            .unwrap();
        observe(&hub, &f.store);
        let after = hub.task_snapshot().unwrap();
        // What the task waits on is the lane's: the board's own fields stand as they were.
        let board_fields = |tasks: &[symphony_api::Task]| {
            let mut tasks = tasks.to_vec();
            tasks.iter_mut().for_each(|t| t.waits_on = None);
            tasks
        };
        assert_eq!(
            board_fields(&before.tasks),
            board_fields(&after.tasks),
            "task changes must not trigger this case"
        );
        let waits = |tasks: &symphony_api::Tasks| tasks.tasks[0].waits_on.clone().unwrap();
        assert_eq!(
            (waits(&before).kind, waits(&after).kind, waits(&after).who),
            ("working".into(), "decision".into(), "initial-card".into()),
            "the lane's task waits on the card, a decision"
        );
        assert_ne!(before.version, after.version, "the message cursor advanced");
        assert_eq!(
            pending(&hub, ""),
            ["initial-card"],
            "pending documents did not change"
        );
        assert_eq!(*hub.task_signoff.lock().unwrap(), ["tools/signoff"]);
        let pages = hub.pages.read().unwrap();
        let event: serde_json::Value =
            serde_json::from_str(&pages[""].history.back().unwrap().json).unwrap();
        assert!(
            event["changes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|change| change["region"] == "task-signoff"
                    && change["data"] == serde_json::json!(["tools/signoff"]))
        );
        // And a page that holds the task is told what it waits on now.
        let told = event["changes"].as_array().unwrap().iter().any(|change| {
            change["data"]["fields"]["waits_on"]["who"] == "initial-card"
                && change["data"]["fields"]["waits_on"]["kind"] == "decision"
        });
        assert!(told, "{event}");
    }

    #[test]
    fn live_column_totals_count_only_parents_in_every_status() {
        let tree = Tree::new();
        let root = repository(&tree);
        let mut seed = Seed::default().task("tools/owner-controls", "ready");
        for (index, status) in symphony_board::STATUSES.iter().enumerate() {
            let mut child =
                crate::testing::board::task(&format!("tools/owner-controls/{status}"), status);
            child.id = index as i64 + 2;
            child.parent = Some("tools/owner-controls".into());
            seed = seed.task_row(child);
        }
        let f = Fixture::new(seed);
        let hub = Hub::new(&root, &root.join("build/client"));
        hub.refresh_page("", &root).unwrap();
        observe(&hub, &f.store);
        let before = hub.task_snapshot().unwrap();
        assert_eq!(before.tasks.len(), symphony_board::STATUSES.len() + 1);
        assert_eq!(before.tasks[0].status, "ready");

        f.store
            .move_to("tools/owner-controls", "cancelled", "owner")
            .unwrap();
        observe(&hub, &f.store);
        let pages = hub.pages.read().unwrap();
        let event: serde_json::Value =
            serde_json::from_str(&pages[""].history.back().unwrap().json).unwrap();
        let change = event["changes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|change| change["region"] == "task-counts")
            .unwrap();
        let counts: std::collections::BTreeMap<String, usize> =
            serde_json::from_value::<Vec<(String, usize)>>(change["data"].clone())
                .unwrap()
                .into_iter()
                .collect();
        assert_eq!(counts["ready"], 0);
        assert_eq!(counts["cancelled"], 1);
        assert_eq!(counts["done"], 0);
        assert_eq!(counts.len(), symphony_board::STATUSES.len());
        assert_eq!(counts.values().sum::<usize>(), 1);
        let recent = event["changes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|change| change["region"] == "task-recent-counts")
            .unwrap();
        for period in ["day", "week"] {
            let counts: std::collections::BTreeMap<String, usize> =
                serde_json::from_value::<Vec<(String, usize)>>(recent["data"][period].clone())
                    .unwrap()
                    .into_iter()
                    .collect();
            assert_eq!(counts["done"], 0);
            assert_eq!(counts["cancelled"], 1);
        }
    }

    #[test]
    fn idle_scope_refresh_follows_a_moved_tree_without_waiting_for_a_document_edit() {
        let main = Path::new("/repo");
        let ui = Path::new("/repo/build/client");
        let root = Path::new("/trees/new");
        let topology = BTreeSet::from([main.join(".git/worktrees/branch/HEAD")]);
        assert!(scope_affected(
            Some(Path::new("/trees/old")),
            root,
            Some(&topology),
            main,
            ui
        ));
        assert!(!scope_affected(Some(root), root, Some(&topology), main, ui));
        assert!(scope_affected(None, root, Some(&topology), main, ui));
        for path in [
            root.join("docs/TOPIC.md"),
            ui.join("app.js"),
            main.to_path_buf(),
        ] {
            let dirty = BTreeSet::from([path]);
            assert!(scope_affected(Some(root), root, Some(&dirty), main, ui));
        }
        assert!(scope_affected(Some(root), root, None, main, ui));
    }

    #[test]
    fn idle_watch_ignores_scan_bookkeeping_but_follows_documents_and_refs() {
        let common = Path::new("/repo/.git");
        for (path, expected) in [
            ("/repo/docs/game/SPEC.md", true),
            ("/repo/.git/refs/heads/main", true),
            ("/repo/.git/refs/heads/main.lock", false),
            ("/repo/.git/packed-refs", true),
            ("/repo/.git/worktrees/new-tree", true),
            ("/repo/.git/worktrees/tree/HEAD", true),
            ("/repo/.git/worktrees/tree/gitdir", true),
            ("/repo/.git/worktrees/tree/index", false),
            ("/repo/.git/worktrees/tree/index.lock", false),
            ("/repo/.git/worktrees/tree/logs/HEAD", false),
            ("/repo/.git/symphony/server/server.log", false),
            ("/repo/.git/index.lock", false),
        ] {
            assert_eq!(relevant(Some(common), Path::new(path)), expected, "{path}");
        }
        assert!(relevant(None, Path::new("/preview/docs/SPEC.md")));
    }

    #[test]
    fn the_branches_that_moved_are_the_heads_a_change_names_less_locks() {
        let common = Path::new("/repo/.git");
        let dirty = BTreeSet::from(
            [
                "/repo/.git/refs/heads/main",
                "/repo/.git/refs/heads/tools/task-gates-panel",
                "/repo/.git/refs/heads/tools/x.lock",
                "/repo/.git/refs/heads/v1.2",
                "/repo/.git/refs/tags/v1",
                "/repo/.git/refs/heads",
                "/repo/.git/packed-refs",
                "/repo/docs/TOPIC.md",
                "/elsewhere/.git/refs/heads/other",
            ]
            .map(PathBuf::from),
        );
        assert_eq!(
            moved_branches(Some(common), Some(&dirty)),
            ["main", "tools/task-gates-panel", "v1.2"]
        );
        assert!(moved_branches(None, Some(&dirty)).is_empty());
        assert!(
            moved_branches(Some(common), None).is_empty(),
            "a full refresh"
        );
    }

    #[test]
    fn a_branchs_new_tip_is_told_to_every_page_once() {
        let tree = Tree::new();
        let root = repository(&tree);
        let hub = Hub::new(&root, &root.join("build/client"));
        hub.refresh_docs(None).unwrap();
        let common = hub.common.clone().expect("a checkout");
        let told = |hub: &Hub| -> Vec<Value> {
            hub.pages.read().unwrap()[""]
                .history
                .iter()
                .flat_map(|event| {
                    let event: Value = serde_json::from_str(&event.json).unwrap();
                    event["changes"].as_array().cloned().unwrap_or_default()
                })
                .filter(|change| change["region"] == "branches")
                .collect()
        };
        hub.refresh_docs(Some(&BTreeSet::from([root.join("docs/TOPIC.md")])))
            .unwrap();
        assert!(told(&hub).is_empty(), "no ref moved");
        hub.refresh_docs(Some(&BTreeSet::from([
            common.join("refs/heads/tools/x"),
            common.join("refs/heads/tools/x.lock"),
        ])))
        .unwrap();
        assert_eq!(
            told(&hub),
            [json!({"region": "branches", "data": ["tools/x"]})]
        );
    }

    #[test]
    fn a_sweep_reports_a_missed_edit_once_and_only_in_main_and_served_trees() {
        let mut tree = Tree::new();
        tree.root = tree.root.canonicalize().unwrap();
        let root = repository(&tree);
        for branch in ["served", "unserved"] {
            git(
                &root,
                &[
                    "worktree",
                    "add",
                    "-b",
                    branch,
                    tree.path(branch).to_str().unwrap(),
                ],
            );
        }
        tree.write("served/view/models/lamps/A.item", "model fixture");
        let hub = Hub::new(&root, &root.join("build/client"));
        *hub.swept.lock().unwrap() = Sweep::new(&hub.swept_paths());
        hub.refresh_docs(None).unwrap();
        hub.ensure_page("served").unwrap();
        let swept: BTreeSet<_> = hub
            .swept_paths()
            .into_iter()
            .map(|(base, below, _)| crate::backstop::joined(&base, &below))
            .collect();
        for path in [root.join("docs"), tree.path("served/docs")] {
            assert!(swept.contains(&path), "{}", path.display());
        }
        assert!(!swept.contains(&tree.path("unserved/docs")));
        assert!(
            hub.watch_paths()
                .iter()
                .any(|(path, _)| *path == tree.path("unserved/docs")),
            "an unserved tree is still watched"
        );
        // Its page read the tree whole, so its documents and its model are no change.
        assert!(!hub.sweep(), "serving a tree is not a change in it");
        assert!(hub.dirty.lock().unwrap().is_empty());

        tree.write(
            "unserved/docs/TOPIC.md",
            "# Topic\n\n- [ ] **unserved-new** — Left to its event.\n",
        );
        assert!(!hub.sweep());
        let main = tree.write(
            "main/docs/TOPIC.md",
            "# Topic\n\n- [ ] **main-new** — Missed by the watcher.\n",
        );
        let served = tree.write(
            "served/docs/TOPIC.md",
            "# Topic\n\n- [ ] **served-new** — Missed by the watcher.\n",
        );
        assert!(hub.sweep());
        assert_eq!(
            std::mem::take(&mut *hub.dirty.lock().unwrap()),
            BTreeSet::from([main, served])
        );
        assert!(!hub.sweep(), "a change is reported once");
        assert!(hub.dirty.lock().unwrap().is_empty());
    }
}
