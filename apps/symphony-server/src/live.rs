//! One filesystem watcher and board reader for all open clients. Changes are retained briefly so a
//! board GET and its following feed subscribe cannot lose an edit between them. Slow clients heal
//! through a new snapshot instead of silently dropping events.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, RwLock, Weak, mpsc};
use std::time::{Duration, Instant};

use crate::backstop::{Backstop, Sweep, Wake};
use notify::{RecursiveMode, Watcher};
use serde_json::{Value, json};
use symphony::{branches, dashboard, taskview};
use symphony_api::{Activity, Board, Pending, Queue, Tasks};
use tokio::sync::watch;

const HISTORY: usize = 128;
const BOARD_INTERVAL: Duration = Duration::from_secs(1);

/// `run`, timed under `SF_SYMPHONY_PROFILE`: mark its start, then report wall and process CPU
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

/// [`measured`] for a pass that awaits, such as the orphan census: its timing line is logged
/// as `run` resolves; a dropped pass retains only its start marker.
pub(crate) async fn measured_async<T>(name: &str, run: impl Future<Output = T>) -> T {
    let stage = Stage::begin();
    if stage.is_some() {
        eprintln!("· review timing start: {name}");
    }
    let result = run.await;
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
    /// Now, when `SF_SYMPHONY_PROFILE` is set; ordinary serving reads no clock.
    fn begin() -> Option<Stage> {
        static PROFILE: LazyLock<bool> =
            LazyLock::new(|| std::env::var_os("SF_SYMPHONY_PROFILE").is_some());
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
    missing: BTreeSet<String>,
}

pub(crate) struct Hub {
    pub root: PathBuf,
    ui_dist: PathBuf,
    pub pages: RwLock<BTreeMap<String, Page>>,
    pub changed: watch::Sender<u64>,
    pub draining: watch::Sender<bool>,
    pub server_notice: watch::Sender<String>,
    initial_board: watch::Sender<bool>,
    initial_start: Mutex<Option<mpsc::Sender<()>>>,
    invalid: RwLock<BTreeMap<String, String>>,
    trees: RwLock<BTreeMap<String, PathBuf>>,
    tasks: Mutex<TaskState>,
    agents: Mutex<Option<symphony_api::Agents>>,
    latest_message: Mutex<Option<i64>>,
    pub task_signoff: Mutex<Vec<String>>,
    branches: Mutex<Vec<branches::BranchCard>>,
    watch_changes: Mutex<BTreeSet<PathBuf>>,
    warmed_watches: Mutex<BTreeMap<PathBuf, Sweep>>,
    callbacks: Mutex<Option<symphony::callbacks::Flusher>>,
    dirty: Mutex<BTreeSet<PathBuf>>,
    /// What the backstop last saw of main and the served trees.
    swept: Mutex<Sweep>,
    common: Option<PathBuf>,
    signoff_pending: Mutex<Option<Pending>>,
    task_activity: Mutex<Option<(i64, Vec<symphony_api::ActivityItem>)>>,
    worker: Mutex<Option<std::thread::JoinHandle<()>>>,
}

struct TaskState {
    last: Option<Tasks>,
    raw: Option<symphony_board::Board>,
    error: Option<String>,
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

fn build(dist: &Path) -> String {
    let shell = std::fs::read_to_string(dist.join("index.html")).unwrap_or_default();
    // dx writes the module script with a hashed filename. This is exactly the path the browser
    // compares with Board.build.
    shell
        .split("src=\"")
        .skip(1)
        .filter_map(|s| s.split('"').next())
        .find(|src| src.ends_with(".js"))
        .unwrap_or("")
        .to_string()
}

pub(crate) fn dist(root: &Path) -> PathBuf {
    std::env::var_os("SF_SYMPHONY_UI_DIST")
        .map_or_else(|| root.join("build/symphony-ui/public"), PathBuf::from)
}

impl Hub {
    pub(crate) fn new(root: &Path, ui_dist: &Path) -> Self {
        Self {
            root: root.into(),
            ui_dist: ui_dist.into(),
            pages: RwLock::new(BTreeMap::new()),
            changed: watch::Sender::new(0),
            draining: watch::Sender::new(false),
            server_notice: watch::Sender::new(String::new()),
            initial_board: watch::Sender::new(true),
            initial_start: Mutex::new(None),
            invalid: RwLock::new(BTreeMap::new()),
            trees: RwLock::new(
                measured("hub branch trees", || branches::trees(root))
                    .into_iter()
                    .map(|t| (t.id(), t.path))
                    .collect(),
            ),
            tasks: Mutex::new(TaskState {
                last: None,
                raw: None,
                error: Some("the task board is starting".into()),
            }),
            agents: Mutex::new(None),
            latest_message: Mutex::new(None),
            task_signoff: Mutex::new(Vec::new()),
            branches: Mutex::new(Vec::new()),
            watch_changes: Mutex::new(BTreeSet::new()),
            warmed_watches: Mutex::new(BTreeMap::new()),
            callbacks: Mutex::new(None),
            dirty: Mutex::new(BTreeSet::new()),
            swept: Mutex::new(Sweep::default()),
            common: measured("hub checkout", || crate::repo::checkout(root))
                .ok()
                .map(|c| c.common),
            signoff_pending: Mutex::new(None),
            task_activity: Mutex::new(None),
            worker: Mutex::new(None),
        }
    }

    /// Finish the current refresh (including its children), then release the watcher and board.
    pub fn stop(&self) {
        self.draining.send_replace(true);
        self.disarm_initial_board();
        let mut worker = self.worker.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(worker) = worker.take()
            && worker.join().is_err()
        {
            crate::front::tell(format_args!(
                "· Symphony data worker panicked while stopping"
            ));
        }
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
        let state = self.tasks.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(error) = &state.error {
            return Err(error.clone());
        }
        state
            .last
            .clone()
            .ok_or_else(|| "the task board is starting".into())
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

    pub fn problem(&self, scope: &str) -> Option<String> {
        self.invalid
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(scope)
            .cloned()
    }

    pub fn document(&self, scope: &str, rel: &str) -> Result<symphony_api::Document, String> {
        let root = if scope.is_empty() {
            self.root.clone()
        } else {
            self.tree(scope).ok_or("this worktree was removed")?
        };
        let mut pages = self.pages.write().unwrap_or_else(|e| e.into_inner());
        let page = pages.get_mut(scope).ok_or("this worktree was removed")?;
        let mut doc =
            symphony::docview::document(&symphony::render::Render::new(root), rel, scope)?;
        // A GET can observe an edit before the watcher. Publish it before handing out its cursor,
        // so existing subscribers also receive that edit.
        if let Some(delta) = document_change(page, rel, &doc) {
            self.publish(page, vec![json!({"region":"document","data":delta})]);
        }
        doc.version = page.board.version;
        page.documents.insert(rel.into(), doc.clone());
        Ok(doc)
    }

    pub fn folder(&self, scope: &str, rel: &str) -> Result<symphony_api::Folder, String> {
        let root = if scope.is_empty() {
            self.root.clone()
        } else {
            self.tree(scope).ok_or("this worktree was removed")?
        };
        let mut folder =
            symphony::docview::folder(&symphony::render::Render::new(root), rel, scope)?;
        let pages = self.pages.read().unwrap_or_else(|e| e.into_inner());
        if let Some(main) = pages.get("") {
            for entry in &mut folder.entries {
                if entry.kind == "doc" {
                    let path = dashboard::document_path(&entry.path);
                    entry.waiting = main
                        .queue
                        .waiting
                        .iter()
                        .filter(|card| dashboard::document_path(&card.doc) == path)
                        .count();
                }
            }
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
        let trees = branches::trees(&self.root)
            .into_iter()
            .map(|t| (t.id(), t.path))
            .collect();
        let previous = std::mem::replace(
            &mut *self.trees.write().unwrap_or_else(|e| e.into_inner()),
            trees,
        );
        *self.branches.lock().unwrap_or_else(|e| e.into_inner()) =
            measured("branch cards", || branches::branch_cards(&self.root));
        // Main also displays branch-only cards; branch document edits can change that queue.
        let main = self.refresh_page("", &self.root);
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
                let documents_changed = baseline_changed
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

    fn refresh_page(&self, scope: &str, root: &Path) -> Result<(), String> {
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
        let snapshot = measured("dashboard", || {
            if scope.is_empty() {
                let cards = self.branches.lock().unwrap_or_else(|e| e.into_inner());
                dashboard::snapshot(root, &cards, &build(&self.ui_dist))
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
                })
            }
        });
        let mut snapshot = match snapshot {
            Ok(snapshot) => {
                self.invalid
                    .write()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(scope);
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
        if let Ok(tasks) = self.task_snapshot() {
            snapshot.board.counts.tasks = Some(taskview::counts(&tasks));
        }
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
            let render = symphony::render::Render::new(root);
            let paths = if documents_changed {
                page.documents.keys().cloned().collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            for rel in paths {
                match symphony::docview::document(&render, &rel, scope) {
                    Ok(doc) => {
                        if let Some(delta) = document_change(page, &rel, &doc) {
                            changes.push(json!({"region":"document", "data":delta}));
                        }
                        page.documents.insert(rel, doc);
                    }
                    Err(_) => {
                        if page.missing.insert(rel.clone()) {
                            let doc = &page.documents[&rel];
                            changes.push(
                                json!({"region":"document", "data":symphony_api::DocumentChange {
                                    path:doc.path.clone(), removed:true, ..Default::default()
                                }}),
                            );
                        }
                    }
                }
            }
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
                changes.push(json!({"region":"docs", "data":snapshot.pending.docs}));
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
                    missing: BTreeSet::new(),
                },
            );
        }
        Ok(())
    }

    fn publish(&self, page: &mut Page, changes: Vec<Value>) {
        if changes.is_empty() {
            return;
        }
        page.board.version += 1;
        let version = page.board.version;
        page.history.push_back(Event {
            version,
            json: json!({"version":version, "changes":changes}).to_string(),
        });
        while page.history.len() > HISTORY {
            page.history.pop_front();
        }
        self.changed.send_modify(|version| *version += 1);
    }

    fn board_read(&self, store: &symphony_board::Store) -> Result<(), String> {
        if let Some(callbacks) = self
            .callbacks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_mut()
            && let Err(error) = measured("chat journal", || callbacks.flush(&self.root))
        {
            eprintln!("CHAT deferred: {error}");
        }
        // Timed events and retries belong to the shared server, never to an agent's monitor.
        let snapshot = match measured("callbacks", || store.dispatch_callbacks_snapshot()) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                eprintln!("CALLBACK deferred: {error}");
                None
            }
        };
        let snapshot = match snapshot {
            Some(snapshot) => snapshot,
            None => measured("board read", || store.clock_board()).map_err(|e| e.to_string())?,
        };
        let agents = measured("agents", || {
            taskview::agents_in(store, &self.root, self.common.as_deref(), &snapshot)
        })?;
        self.update_board(store, snapshot, agents)
    }

    /// Apply one board observation after the journal, callback delivery and machine probes finish.
    fn update_board(
        &self,
        store: &symphony_board::Store,
        snapshot: symphony_board::Board,
        agents: symphony_api::Agents,
    ) -> Result<(), String> {
        let (next, latest) = {
            let state = self.tasks.lock().unwrap_or_else(|e| e.into_inner());
            if state.raw.as_ref() == Some(&snapshot)
                && let Some(last) = &state.last
            {
                (last.clone(), snapshot.latest_message)
            } else {
                measured("board snapshot", || taskview::snapshot_board(&snapshot))
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
            if message_changed
                || previous.as_ref() != Some(&pending)
                || held.last.as_ref().is_none_or(|old| old.tasks != next.tasks)
            {
                let signoff = measured("signoff", || {
                    taskview::signoff(store, &next.tasks, &pending)
                })?;
                let mut old = self.task_signoff.lock().unwrap_or_else(|e| e.into_inner());
                if *old != signoff {
                    changed.push(json!({"region":"task-signoff", "data":signoff}));
                    *old = signoff;
                }
                *previous = Some(pending);
            }
        }
        let mut cursor = self
            .latest_message
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(since) = *cursor
            && since < latest
        {
            let render = symphony::render::Render::new(&self.root);
            for message in store
                .messages_after(since, latest)
                .map_err(|e| e.to_string())?
            {
                let record = taskview::message(&message);
                let mut data = json!(record);
                let entry = taskview::entry(&record, &render);
                data["blocks"] = json!(entry.blocks);
                data["stamp"] = entry.stamp.into();
                changed.push(json!({"region":"message", "data":data}));
            }
        }
        *cursor = Some(latest);
        drop(cursor);
        if held.error.is_some() {
            changed.push(json!({"region":"tasks-status", "data":{"available":true}}));
        }
        if let Some(old) = &held.last {
            task_changes(&mut changed, &old.tasks, &next.tasks);
            if old.tasks != next.tasks {
                let mut counts = BTreeMap::<&str, usize>::new();
                for status in symphony_board::STATUSES {
                    counts.insert(status, 0);
                }
                for task in &next.tasks {
                    *counts.entry(&task.status).or_default() += 1;
                }
                changed.push(
                    json!({"region":"task-counts", "data":counts.into_iter().collect::<Vec<_>>()}),
                );
                let recent = |days: f64| {
                    let mut counts = BTreeMap::<&str, usize>::new();
                    for task in &next.tasks {
                        if ["done", "cancelled"].contains(&task.status.as_str())
                            && task.updated >= store.now() - days * 86_400.0
                        {
                            *counts.entry(task.status.as_str()).or_default() += 1;
                        }
                    }
                    for status in ["done", "cancelled"] {
                        counts.entry(status).or_default();
                    }
                    counts.into_iter().collect::<Vec<_>>()
                };
                changed.push(json!({"region":"task-recent-counts","data":{"day":recent(1.0),"week":recent(7.0)}}));
            }
            // Heartbeats are not displayed. Sending last_seen/last_tool on every beat makes every
            // active worker a stream of pointless events. Sessions change when displayed state does.
            let visible = |sessions: &[symphony_api::Session]| {
                sessions
                    .iter()
                    .map(|s| {
                        let mut s = s.clone();
                        s.last_seen = 0.0;
                        s.last_tool = None;
                        s
                    })
                    .collect::<Vec<_>>()
            };
            let session_at = changed.len();
            diff_items(
                &mut changed,
                "session",
                &visible(&old.sessions),
                &visible(&next.sessions),
                |s| s.handle.clone(),
            );
            for event in &mut changed[session_at..] {
                if event["region"] == "session"
                    && event["data"].get("value").is_some()
                    && let Some(session) = next
                        .sessions
                        .iter()
                        .find(|s| event["data"]["key"] == s.handle)
                {
                    event["data"]["value"] = json!(session);
                }
            }
        }
        held.last = Some(next.clone());
        held.raw = Some(snapshot);
        held.error = None;
        drop(held);
        let mut old_agents = self.agents.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(old) = &*old_agents {
            diff_items(&mut changed, "agent", &old.agents, &agents.agents, |a| {
                a.handle.clone()
            });
            if old.probes != agents.probes {
                changed.push(json!({"region":"agent-probes","data":agents.probes}));
            }
        }
        *old_agents = Some(agents);
        drop(old_agents);
        let mut pages = self.pages.write().unwrap_or_else(|e| e.into_inner());
        for page in pages.values_mut() {
            let mut changes = changed.clone();
            let counts = Some(taskview::counts(&next));
            if page.board.counts.tasks != counts {
                page.board.counts.tasks = counts;
                changes.push(json!({"region":"counts", "data":page.board.counts}));
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

    fn board_error(&self, error: String) {
        let mut held = self.tasks.lock().unwrap_or_else(|e| e.into_inner());
        let newly_unavailable = held.error.is_none();
        held.error = Some(error.clone());
        drop(held);
        if newly_unavailable {
            let mut pages = self.pages.write().unwrap_or_else(|e| e.into_inner());
            for page in pages.values_mut() {
                page.board.counts.tasks = None;
                self.publish(
                    page,
                    vec![
                        json!({"region":"tasks-status", "data":{"available":false,"error":error}}),
                        json!({"region":"counts", "data":page.board.counts}),
                    ],
                );
            }
        }
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
    by_depth.sort_by_key(|folder| folder.len());
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
            threads: doc
                .threads
                .iter()
                .map(|(slug, fragment)| (slug.clone(), Some(fragment.clone())))
                .collect(),
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

fn run(
    weak: Weak<Hub>,
    recv: mpsc::Receiver<()>,
    mut watcher: impl Watcher,
    mut watched: BTreeSet<PathBuf>,
    mut backstop: Backstop,
    mut board: Option<symphony_board::Store>,
    mut connect: impl FnMut(&Path) -> Result<symphony_board::Store, String>,
) {
    let mut last_board = Instant::now() - BOARD_INTERVAL;
    let mut expiry = OrderExpiry::default();
    while let Some(hub) = weak.upgrade() {
        if *hub.draining.borrow() {
            break;
        }
        // One central task-board read per interval, independent of client count. Expired
        // postponements refresh the shared cached queue here, with backoff on store errors;
        // other document and worktree changes come from watcher events and the backstop's sweep.
        if last_board.elapsed() >= BOARD_INTERVAL {
            last_board = Instant::now();
            expiry.tick(&hub.root, symphony::postpone::now(), || {
                hub.refresh_order("")
            });
            if board.is_none() {
                board = connect(&hub.root).ok();
            }
            if let Some(store) = &board
                && let Err(e) = measured("board tick", || hub.board_read(store))
            {
                hub.board_error(e);
                board = None;
            }
        }
        drop(hub);
        let wake = backstop.wait(&recv, Duration::from_millis(250));
        match wake {
            Wake::Event => {
                std::thread::sleep(Duration::from_millis(100));
                while recv.try_recv().is_ok() {}
            }
            Wake::Period => {}
            Wake::Idle => continue,
        }
        let Some(hub) = weak.upgrade() else { break };
        if *hub.draining.borrow() {
            break;
        }
        let refresh = wake == Wake::Event || measured("document sweep", || hub.sweep());
        if refresh {
            let dirty = std::mem::take(&mut *hub.dirty.lock().unwrap_or_else(|e| e.into_inner()));
            hub.swept
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .restamp(&dirty);
            if let Err(e) = measured("document event", || hub.refresh_docs(Some(&dirty))) {
                eprintln!("· Symphony data refresh failed: {e}");
            }
        }
        hub.watch(&mut watcher, &mut watched);
    }
}

fn sort_activity(activity: &mut Activity) {
    activity
        .items
        .sort_by(|a, b| (&b.ts, &b.id).cmp(&(&a.ts, &a.id)));
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
        changes.push(json!({"region":"queue", "data":new.next}));
    } else {
        for (before, after) in old.next.iter().zip(&new.next) {
            if before != after {
                changes.push(json!({"region":"card", "data":after}));
            }
        }
    }
    if old.counts != new.counts {
        changes.push(json!({"region":"counts", "data":new.counts}));
    }
    if old.progress != new.progress {
        changes.push(json!({"region":"progress", "data":new.progress}));
    }
    if old.build != new.build {
        changes.push(json!({"region":"client", "data":{"build":new.build}}));
    }
    changes
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
    let before: BTreeMap<_, _> = old.iter().map(|t| (key(t), t)).collect();
    let after: BTreeMap<_, _> = new.iter().map(|t| (key(t), t)).collect();
    for (id, item) in &after {
        if before.get(id).copied() != Some(*item) {
            changes.push(json!({"region":region, "data":{"key":id, "value":item}}));
        }
    }
    for id in before.keys().filter(|id| !after.contains_key(*id)) {
        changes.push(json!({"region":region, "data":{"key":id, "removed":true}}));
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
            changes
                .push(json!({"region":format!("{region}-order"), "data":{"key":id,"after":after}}));
        }
    }
}

fn task_changes(changes: &mut Vec<Value>, old: &[symphony_api::Task], new: &[symphony_api::Task]) {
    // Insertions/removals use the same keyed list protocol. Existing records send fields only,
    // otherwise a status change would retransmit a task's potentially very long brief.
    let mut basic = Vec::new();
    diff_items(&mut basic, "task", old, new, |t| t.slug.clone());
    for event in basic {
        if let Some(key) = event["data"]["key"].as_str()
            && event["region"] == "task"
            && let Some(before) = old.iter().find(|t| t.slug == key)
            && let Some(after) = new.iter().find(|t| t.slug == key)
        {
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
            field!(roles);
            field!(instruction);
            if before.parent != after.parent {
                fields.parent = Some(after.parent.clone().unwrap_or_default());
            }
            changes.push(json!({"region":"task-fields", "data":symphony_api::TaskChange { key:key.into(), fields }}));
            let mut summary = after.clone();
            summary.body.clear();
            changes.push(json!({"region":"task-summary", "data":symphony_api::ItemChange { key:key.into(), value:Some(summary), removed:false }}));
        } else {
            changes.push(event);
        }
    }
}

mod startup;

#[cfg(test)]
mod gallery_tests;

#[cfg(test)]
mod watch_tests;

#[cfg(test)]
mod postpone_tests;

#[cfg(test)]
mod chat_tests;

#[cfg(test)]
mod backstop_tests;

#[cfg(test)]
mod startup_tests;

#[cfg(test)]
mod boards_tests;

#[cfg(test)]
mod tests {
    use super::{Hub, Sweep, relevant, scope_affected};
    use crate::testing::{
        Tree,
        board::{Fixture, Seed},
        git, repository,
    };
    use std::collections::BTreeSet;
    use std::path::Path;
    use std::time::{Duration, Instant};

    #[test]
    fn a_timing_line_names_its_stage_with_its_wall_and_cpu_time_or_says_cpu_is_unavailable() {
        let unread = super::Stage {
            began: Instant::now() - Duration::from_millis(40),
            cpu: None,
        };
        let line = unread.line("orphan census");
        let (wall, cpu) = line
            .strip_prefix("· review timing: orphan census: ")
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
                generated: store.now(),
                probes: symphony_api::AgentProbes {
                    herdr: "ok".into(),
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
                .client()
                .query(
                    "UPDATE task SET title = 'A changed title' RETURN NONE;",
                    serde_json::json!({}),
                )
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
        let main = inherited.replace("[ ]", "[x]").replace(
            "<!-- register:end -->",
            "- [ ] **main-card** — Main added this after the fork.\n<!-- register:end -->",
        );
        tree.write("main/docs/TOPIC.md", &main);
        git(&root, &["add", "docs/TOPIC.md"]);
        git(&root, &["commit", "-m", "Main moves after fork"]);
        assert_eq!(
            symphony::branches::branch_cards(&root)
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
        assert_eq!(
            before.tasks, after.tasks,
            "task changes must not trigger this case"
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
    }

    #[test]
    fn owner_cancellation_sends_zero_for_the_last_ready_task() {
        let tree = Tree::new();
        let root = repository(&tree);
        let f = Fixture::new(Seed::default().task("tools/owner-controls", "ready"));
        let hub = Hub::new(&root, &root.join("build/client"));
        hub.refresh_page("", &root).unwrap();
        observe(&hub, &f.store);
        let before = hub.task_snapshot().unwrap();
        assert_eq!(before.tasks.len(), 1);
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
            ("/repo/.git/sf-symphony/server.log", false),
            ("/repo/.git/index.lock", false),
        ] {
            assert_eq!(relevant(Some(common), Path::new(path)), expected, "{path}");
        }
        assert!(relevant(None, Path::new("/preview/docs/SPEC.md")));
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
        tree.write("served/view/models/lamps/A.glb", "model fixture");
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
