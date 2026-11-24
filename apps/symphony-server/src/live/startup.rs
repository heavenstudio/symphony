//! The hub's start and its first board read: armed when the hub is built, activated once the URL
//! is announced or a data request needs it, run in the hub's owned worker, awaited by data reads,
//! released by stop.

use super::{Hub, measured, run};
use crate::backstop::Sweep;
use notify::Watcher;
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::{Arc, Weak, mpsc};
use std::time::{Duration, Instant};
use symphony::taskview;
use symphony_board::board_health::hours;
use symphony_board::lock::Recovered as _;

impl Hub {
    /// The hub of the project at `root`, serving the client in `ui_dist`. `one_server`, the
    /// machine's one server's hub, does what its projects share and a private server's must not:
    /// it drains the chat journal, closes the owner's asks and the blocks the owner's word ends,
    /// and keeps the board's health record and hours, the day files and the agents' activity.
    pub fn start(
        root: &Path,
        ui_dist: &Path,
        embedded_ui: bool,
        one_server: bool,
    ) -> Result<Arc<Self>, String> {
        // The worker's reach is held to the repository the hub holds its folder to, as it is made
        // ([`Hub::new`]).
        Self::start_watching::<notify::RecommendedWatcher>(
            root,
            ui_dist,
            embedded_ui,
            one_server,
            crate::backstop::PERIOD,
            taskview::connect,
        )
    }

    /// Finish the current refresh (including its children) and tick, then release the watcher and
    /// board.
    pub fn stop(&self) {
        self.draining.send_replace(true);
        // A notice's program under way ends now; the record keeps it to hand again.
        self.stop_notices();
        self.disarm_initial_board();
        // The ticks' wait between two reads ends at once.
        self.ticks.lock().recover().take();
        let mut worker = self.worker.lock().recover();
        if let Some(worker) = worker.take()
            && worker.join().is_err()
        {
            crate::front::tell(format_args!(
                "· Symphony data worker panicked while stopping"
            ));
        }
        drop(worker);
        // The worker has ended, so no tick records after the hour so far is written. A project
        // removed under the server records nothing: its records would make its folders anew.
        if self.holds_its_project() {
            self.stop_agents();
            self.health.stop_hours(self.now());
        }
    }

    /// Install watcher `W` and documents. The owned worker starts its board read when activated,
    /// then runs the board's ticks on a thread of its own and sweeps the watched files on its own
    /// every `period` ([`run`]).
    pub(super) fn start_watching<W: Watcher + Send + 'static>(
        root: &Path,
        ui_dist: &Path,
        embedded_ui: bool,
        one_server: bool,
        period: Duration,
        mut connect: impl FnMut(&Path) -> Result<symphony_board::Store, String> + Send + 'static,
    ) -> Result<Arc<Self>, String> {
        let started = Instant::now();
        let mut hub = measured("hub new", || Self::new(root, ui_dist));
        hub.embedded_ui = embedded_ui;
        if one_server {
            *hub.callbacks.get_mut().recover() = Some(symphony::callbacks::Flusher::new(root));
            eprintln!("→ chat journal: draining enabled for the shared server");
            // The owner's asks are the one server's to close, as it alone hands saves to the board.
            *hub.owner_asks.get_mut().recover() = Some(super::owner_asks::Kept::default());
            // So are the blocks the owner's word ends: one server moves them, from its pages.
            *hub.decided.get_mut().recover() = Some(symphony::decided::Kept::default());
            // The board's health record is the one server's alone: `tasks status` reads it,
            // and a private server's passes would write over it.
            let mut env = symphony::children::board_env();
            env.cwd = root.to_path_buf();
            match symphony_board::board_health::file(&env) {
                Ok(file) => hub.health = symphony_board::board_health::Recorder::writing(file),
                Err(why) => eprintln!("→ board health: kept in memory alone ({why})"),
            }
            // Its hours too, a line each in the day files the Reports page reads.
            match hours::folder(&env) {
                Ok(folder) => hub.health.keep_hours(folder, hours::awake, |line| {
                    crate::front::tell(format_args!("{line}"));
                }),
                Err(why) => {
                    crate::front::tell(format_args!("→ board health: no hours kept ({why})"))
                }
            }
            // So are the project's day files, rolled up once an hour, and its runs' beside them.
            match symphony_board::reports::folder(&env) {
                Ok(folder) => {
                    let runs = symphony_board::runs::folder(&env);
                    *hub.reports.get_mut().recover() =
                        Some(super::reports::Rollup::new(folder, runs));
                }
                Err(why) => eprintln!("→ reports: no day files written ({why})"),
            }
            // So are the agents' day files, which a private server's ticks would say twice.
            match super::agent_activity::folder(&env) {
                Ok(folder) => {
                    *hub.agent_activity.get_mut().recover() =
                        Some(super::agent_activity::Recorder::new(folder));
                }
                Err(why) => eprintln!("→ agent activity: not recorded ({why})"),
            }
        } else {
            eprintln!("→ chat journal: left untouched by this private server");
        }
        // Said once, at the hub's one start: the documents come in when the folder does.
        if let Some(folder) = symphony::project::Project::of(root).missing_documents(root) {
            eprintln!(
                "→ documents: no {folder}/ directory under {}; the board and agents are served without documents",
                root.display()
            );
        }
        let starting = hub.arm_initial_board();
        let hub = Arc::new(hub);
        // Install watches before the first read, so an edit during that read is queued too, as is
        // a rescan the read asks for.
        let backstop = crate::backstop::Backstop::new(period);
        let _ = hub.documents_waker.set(backstop.waker());
        let changed = backstop.waker();
        let changed_hub = Arc::downgrade(&hub);
        let mut watcher = W::new(
            move |event: notify::Result<notify::Event>| {
                if let Some(hub) = changed_hub.upgrade()
                    && hub.record_event(event)
                {
                    changed.wake();
                }
            },
            notify::Config::default(),
        )
        .map_err(|e| format!("cannot watch Symphony data: {e}"))?;
        let mut watched = BTreeSet::new();
        measured("initial watches", || hub.watch(&mut watcher, &mut watched));
        // The sweep's first stamps come before the first read too.
        *hub.swept.lock().recover() = measured("initial sweep", || Sweep::new(&hub.swept_paths()));
        measured("initial documents", || hub.refresh_docs(None))?;
        let docs_ready = started.elapsed();
        let weak = Arc::downgrade(&hub);
        *hub.worker.lock().recover() = Some(std::thread::spawn(move || {
            let first = initial_board(&weak, &starting, &mut connect, docs_ready, started);
            if let Some(board) = first {
                run(weak, watcher, watched, backstop, board, connect);
            }
        }));
        Ok(hub)
    }

    /// Arm the activation the owned worker waits on; data reads wait until its first result.
    fn arm_initial_board(&mut self) -> mpsc::Receiver<()> {
        let (start, starting) = mpsc::channel();
        // Each holder of this lock only takes or puts the sender, so a panic under it leaves either
        // the sender or nothing: both are a whole state, and the poisoned value is used as it is.
        *self.initial_start.get_mut().recover() = Some(start);
        self.initial_board.send_replace(false);
        starting
    }

    /// Activate the owned read once, after announcement or when a data caller needs it.
    pub(crate) fn start_initial_board(&self) {
        // A panic under this lock leaves the sender or nothing (see `arm_initial_board`).
        if let Some(start) = self.initial_start.lock().recover().take() {
            let _ = start.send(());
        }
    }

    /// Data requests wait for the first board attempt; shutdown releases them with an error.
    pub(crate) async fn wait_initial_board(&self) -> Result<(), String> {
        let mut ready = self.initial_board.subscribe();
        let mut draining = self.draining.subscribe();
        tokio::select! {
            biased;
            _ = draining.wait_for(|stopping| *stopping) => Err("the Symphony server is stopping".into()),
            _ = ready.wait_for(|ready| *ready) => Ok(()),
        }
    }

    /// Drop an activation that never came, so a worker still waiting on it ends.
    pub(super) fn disarm_initial_board(&self) {
        // A panic under this lock leaves the sender or nothing (see `arm_initial_board`).
        self.initial_start.lock().recover().take();
    }
}

/// The board's error after a panic in the first board read.
pub(super) const PANICKED: &str = "the first board read panicked; restart the Symphony server";

/// What a panic in the first board read (connecting, reading, probing agents) leaves: the board's
/// error, and the waiting reads released to answer it, as a failed read does. It acts only while
/// unwinding; a read that returns sets its own result.
struct Unwinding<'a>(&'a Hub);

impl Drop for Unwinding<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.0.board_error(PANICKED.into());
            self.0.initial_board.send_replace(true);
            self.0.board_moved();
        }
    }
}

/// The owned worker's first board read, once activated: `None` when the hub stopped or went
/// before it was, else the store the read opened (`None` inside when it failed).
fn initial_board(
    weak: &Weak<Hub>,
    starting: &mpsc::Receiver<()>,
    connect: &mut impl FnMut(&Path) -> Result<symphony_board::Store, String>,
    docs_ready: Duration,
    started: Instant,
) -> Option<Option<symphony_board::Store>> {
    starting.recv().ok()?;
    let hub = weak.upgrade()?;
    if *hub.draining.borrow() {
        return None;
    }
    let board_started = Instant::now();
    let unwinding = Unwinding(&hub);
    let mut board = None;
    hub.board_tick(&mut board, connect);
    drop(unwinding);
    // Logged before the waiters are released, so the log orders this read against what waited.
    eprintln!(
        "→ native read data warmed: documents {:.1} ms, board {:.1} ms, total {:.1} ms",
        docs_ready.as_secs_f64() * 1000.0,
        board_started.elapsed().as_secs_f64() * 1000.0,
        started.elapsed().as_secs_f64() * 1000.0
    );
    hub.initial_board.send_replace(true);
    // Whether the project has a board is known from here ([`Hub::has_board`]).
    hub.board_moved();
    Some(board)
}
