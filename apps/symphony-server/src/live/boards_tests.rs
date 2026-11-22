//! Each project's hub on its own board: two fixture projects, each its own SurrealDB server in its
//! repository's common dir in a temporary folder, started by its first caller. A hub reconnects
//! when its board's server restarts, and reads on when a newer build migrates its board, while
//! every write the Symphony would send there is refused naming the project. A board behind the
//! hub's build it never moves forward: it says why it reads nothing until the board's command has.

use super::Hub;
use crate::front::{Shared, router};
use crate::lock::Held as _;
use crate::testing::{Tree, board::surreal, git, repository_at};
use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::Request;
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;
use symphony_board::cli::Env;
use symphony_board::rpc::Client;
use symphony_board::server::Home;
use symphony_board::{NewTask, Opening, Program, Schema, Store};
use tower::ServiceExt;

/// The wall-clock backstop of a wait here on the hub's read: only a stall reaches it (RULES.md
/// `tests/no-test-fails-on-load`).
const BACKSTOP: Duration = test_time::backstop(Duration::from_secs(30));

/// What the hub's reach of a fixture's board says while its case keeps it from the board
/// ([`Project::unreached`]).
const UNREACHED: &str = "the case keeps its hub from its board while it ends";

/// A fixture project: its checkout, holding `symphony.toml` with its id, its hub, whether that
/// hub's reach of its board goes on ([`Project::unreached`]), and where its next reach waits
/// ([`Project::paused`]).
pub(super) struct Project {
    pub(super) root: PathBuf,
    pub(super) hub: Arc<Hub>,
    reaches: Arc<Mutex<bool>>,
    pause: Arc<Mutex<Option<Pause>>>,
}

/// Where a hub's reach waits, once its tick's check that its folder holds its project has passed:
/// it says it is there, and goes on once it hears.
struct Pause {
    there: mpsc::Sender<()>,
    on: mpsc::Receiver<()>,
}

impl Project {
    /// The project `id` in `tree`, its board holding the task `<id>/one`, its hub reading it.
    fn new(tree: &Tree, id: &str) -> Project {
        Project::made_by(tree, id, writer)
    }

    /// As `new`, its board made by `make`, which opens it to write.
    pub(super) fn made_by(tree: &Tree, id: &str, make: impl FnOnce(&Path) -> Store) -> Project {
        Project::made_in(tree, id, id, make)
    }

    /// As `made_by`, its checkout in the folder `folder` of `tree`.
    fn made_in(tree: &Tree, folder: &str, id: &str, make: impl FnOnce(&Path) -> Store) -> Project {
        let root = repository_at(tree, folder);
        std::fs::write(
            root.join("symphony.toml"),
            format!("[project]\nid = \"{id}\"\n"),
        )
        .unwrap();
        make(&root)
            .create(&NewTask::new(&format!("{id}/one"), "Made first", "owner"))
            .unwrap();
        let home = Home::in_common(&root.join(".git"), Some(&surreal()));
        if home.backend().unwrap() == symphony_board::server::Engine::PostgreSql {
            home.project_postgresql(&root).unwrap().stop().unwrap();
        }
        let reach = env(&root);
        let reaches = Arc::new(Mutex::new(true));
        let gate = Arc::clone(&reaches);
        let pause = Arc::new(Mutex::new(None::<Pause>));
        let paused = Arc::clone(&pause);
        let hub = Hub::start_watching::<notify::NullWatcher>(
            &root,
            &root.join("build/client"),
            false,
            false,
            crate::backstop::PERIOD,
            move |root| {
                // Held through the reach, so a case that closes the gate waits out one under way.
                let open = gate.lock().held();
                if !*open {
                    return Err(UNREACHED.to_string());
                }
                if let Some(pause) = paused.lock().unwrap().take() {
                    let _ = pause.there.send(());
                    let _ = pause.on.recv();
                }
                // Held to the repository found as the hub was made, as every reach from its
                // folder in this process is.
                symphony::taskview::connect_in(root, reach.clone())
            },
        )
        .unwrap();
        hub.start_initial_board();
        Project {
            root,
            hub,
            reaches,
            pause,
        }
    }

    /// Hold the hub's next reach of its board where it waits, once its check has passed: what
    /// returns says when it is there, and lets it go on.
    fn paused(&self) -> (mpsc::Receiver<()>, mpsc::Sender<()>) {
        let (there, arrived) = mpsc::channel();
        let (go, on) = mpsc::channel();
        *self.pause.lock().unwrap() = Some(Pause { there, on });
        (arrived, go)
    }

    /// What `end` returns, run while the hub's reach of the board is refused, once a reach under
    /// way has ended: a case's end, its board's server stopped and its folder removed. Between the
    /// two the hub's next read would start the server again, as it starts a stopped board
    /// (`each_projects_hub_reads_its_own_board_and_reconnects_when_its_server_restarts`), and that
    /// start would write into the folder while it is removed.
    fn unreached<T>(&self, end: impl FnOnce() -> T) -> T {
        *self.reaches.lock().unwrap() = false;
        let ended = end();
        *self.reaches.lock().unwrap() = true;
        ended
    }

    fn home(&self) -> Home {
        Home::in_common(&self.root.join(".git"), Some(&surreal()))
    }

    /// The schema version the project's board records.
    fn version(&self) -> Option<i64> {
        let client = Client::new(&self.home().recorded().unwrap().addr, "sf", "tasks");
        symphony_board::Raw::on(&client)
            .row("meta", "schema")
            .field("version")
            .unwrap()
            .as_i64()
    }

    /// Wait for the hub to say why it reads no board in words that start with `start` (not that it
    /// has read none yet, or that its board's server is starting): those words. Only a stall
    /// reaches the wait's backstop.
    fn refused(&self, start: &str) -> String {
        let mut read = self.read();
        let refused = test_time::until(
            format!("{}: the hub to say {start:?}", self.root.display()),
            BACKSTOP,
            || {
                read = self.read();
                read.as_ref()
                    .err()
                    .filter(|why| why.starts_with(start))
                    .cloned()
            },
        );
        refused.unwrap_or_else(|stall| panic!("{stall}; it read {read:?}"))
    }

    /// The slugs of the tasks the hub last read, or why it reads no board.
    fn read(&self) -> Result<Vec<String>, String> {
        let state = self.hub.tasks.lock().unwrap();
        match (&state.error, &state.raw) {
            (Some(error), _) => Err(error.clone()),
            (None, Some(board)) => Ok(board.tasks.iter().map(|task| task.slug.clone()).collect()),
            (None, None) => Err("no read yet".into()),
        }
    }

    /// Wait for the hub to read `slugs`; only a stall reaches the wait's backstop.
    pub(super) fn reads(&self, slugs: &[&str]) {
        let mut read = self.read();
        let waited = test_time::until_true(
            format!("{}: the hub to read {slugs:?}", self.root.display()),
            BACKSTOP,
            || {
                read = self.read();
                read.as_ref().is_ok_and(|read| read == slugs)
            },
        );
        if let Err(stall) = waited {
            panic!("{stall}; it read {read:?}");
        }
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        self.hub.stop();
        let home = self.home();
        if home.backend().ok() == Some(symphony_board::server::Engine::PostgreSql) {
            if let Ok(pg) = home.project_postgresql(&self.root) {
                let _ = pg.stop();
            }
        } else {
            let _ = home.stop();
        }
    }
}

/// What a caller in `root` reads: the project's own board, its server started if it is down.
pub(super) fn env(root: &Path) -> Env {
    Env {
        vars: [(
            "SYMPHONY_SURREAL".to_string(),
            surreal().display().to_string(),
        )]
        .into(),
        cwd: root.to_path_buf(),
    }
}

/// As `env`, its clock pinned at `now` (`TASKS_NOW`).
fn pinned(root: &Path, now: &str) -> Env {
    let mut env = env(root);
    env.vars.insert("TASKS_NOW".into(), now.into());
    env
}

/// The board of `root`'s project, opened to write as its command line opens it.
pub(super) fn writer(root: &Path) -> Store {
    on_surreal(root);
    symphony_board::cli::connect(&env(root)).unwrap()
}

/// The board of `root`'s project, when it is not made yet, named SurrealDB's: these cases start,
/// stop and read the record of its SurrealDB server, and its first command would otherwise make
/// it on PostgreSQL. A board that is made, or named another engine, is left as it is.
pub(super) fn on_surreal(root: &Path) {
    let home = Home::in_common(&root.join(".git"), Some(&surreal()));
    if home.unmade().unwrap() {
        home.initialize_backend(symphony_board::server::Engine::SurrealDb)
            .unwrap();
    }
}

#[test]
fn a_machine_hub_starts_its_selected_postgresql_board_and_keeps_its_reader_read_only() {
    let tree = Tree::new();
    let project = Project::made_by(&tree, "pg", |root| {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let programs = symphony_board::server::postgres::discover_programs(&workspace).unwrap();
        std::fs::write(
            root.join("symphony.toml"),
            format!(
                "[project]\nid = \"pg\"\n[board]\npostgresql = {:?}\n",
                programs.display().to_string()
            ),
        )
        .unwrap();
        let home = Home::in_common(&root.join(".git"), None);
        home.initialize_backend(symphony_board::server::Engine::PostgreSql)
            .unwrap();
        writer(root)
    });
    // The constructor stops the seeded cluster before starting its hub. Its read starts it.
    project.reads(&["pg/one"]);
    let home = project.home();
    let pg = home.project_postgresql(&project.root).unwrap();
    assert!(pg.current().unwrap().is_some());
    assert!(!home.dir().join("data").exists());
    let shared = Shared::new(1, 1234, &project.root, None, crate::testing::open_gate());
    assert!(shared.api.set(project.hub.clone()).is_ok());
    let (status, body) = answer(&shared, asked("GET", "/api/tasks", None));
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["tasks"][0]["slug"], "pg/one", "{body}");
    let reader = symphony::taskview::connect_in(&project.root, env(&project.root)).unwrap();
    assert!(reader.database().is_postgresql());
    assert_eq!(reader.get("pg/one").unwrap().title, "Made first");
    // A web reader cannot migrate a newer board or write through it.
    drop(reader);
    let schema = Schema::this_build().then_each("", "CREATE TABLE fixture_next (id int);");
    let reached = symphony_board::cli::board(&env(&project.root)).unwrap();
    let how = reached.opening(Opening::write().at(schema).migrating().leave_behind());
    Store::opening_on(
        reached.database(),
        Box::new(|| 1_000_000.0),
        Box::new(|_| false),
        "test",
        &how,
    )
    .unwrap();
    let reader = symphony::taskview::connect_in(&project.root, env(&project.root)).unwrap();
    assert!(
        reader
            .create(&NewTask::new("pg/refused", "Cannot write", "owner"))
            .is_err()
    );
    assert!(reader.get("pg/refused").is_err());
}

/// The board of `root`'s project, opened to write by a build one schema version behind this one:
/// the project's board command while this build's schema has not landed, on this machine, so the
/// line it writes (a build at schema 18 or later keeps one) is that of the migrate run here after,
/// which leaves it out.
fn older_writer(root: &Path) -> Store {
    on_surreal(root);
    let reached = symphony_board::cli::writable_board(&env(root)).unwrap();
    Store::opening(
        reached.client,
        Box::new(|| 1_000_000.0),
        Box::new(|_| false),
        &symphony_board::sys::hostname(),
        &Opening::write()
            .at(Schema::this_build().before())
            .of(reached.project.as_deref()),
    )
    .unwrap()
}

/// The board of `root`'s project, moved forward to `schema` by a newer build as its `tasks migrate
/// --leave-behind` in the project's main checkout moves it, which alone moves the live board of a
/// project that commits no board source: reached as that command reaches it, and opened to migrate,
/// leaving this build's writers behind.
fn newer_migrate(root: &Path, schema: Schema) -> Store {
    let reached = symphony_board::cli::board(&env(root)).unwrap();
    Store::opening(
        reached.client,
        Box::new(|| 1_000_000.0),
        Box::new(|_| false),
        "this-host",
        &Opening::write()
            .at(schema)
            .of(reached.project.as_deref())
            .live(reached.live)
            .migrating()
            .leave_behind(),
    )
    .unwrap()
}

#[test]
fn each_projects_hub_reads_its_own_board_and_reconnects_when_its_server_restarts() {
    let tree = Tree::new();
    let alpha = Project::new(&tree, "alpha");
    let beta = Project::new(&tree, "beta");
    alpha.reads(&["alpha/one"]);
    beta.reads(&["beta/one"]);
    let (first, beside) = (
        alpha.home().recorded().unwrap(),
        beta.home().recorded().unwrap(),
    );
    assert_ne!(first.addr, beside.addr);

    assert_eq!(
        alpha.home().stop().unwrap().map(|ran| ran.pid),
        Some(first.pid)
    );
    // The next caller starts it again, the hub or this write, whichever comes first.
    writer(&alpha.root)
        .create(&NewTask::new(
            "alpha/two",
            "Made after the restart",
            "owner",
        ))
        .unwrap();
    alpha.reads(&["alpha/one", "alpha/two"]);
    assert_ne!(alpha.home().recorded().unwrap().pid, first.pid);
    beta.reads(&["beta/one"]);
    assert_eq!(beta.home().recorded().unwrap().pid, beside.pid);
}

#[test]
fn a_hub_reads_on_when_a_newer_build_migrates_its_board_and_its_writes_there_are_refused() {
    let tree = Tree::new();
    let alpha = Project::new(&tree, "alpha");
    let beta = Project::new(&tree, "beta");
    beta.reads(&["beta/one"]);
    let newer = Schema::this_build().then("DEFINE TABLE IF NOT EXISTS fixture_next SCHEMALESS;");
    let migrated = newer_migrate(&beta.root, newer.clone());
    assert_eq!(beta.version(), Some(newer.version()));
    migrated
        .create(&NewTask::new("beta/newer", "By the newer build", "owner"))
        .unwrap();
    beta.reads(&["beta/one", "beta/newer"]);

    // What the Symphony's write routes open is read, and its writes refused before any write: its
    // clock pinned as a test pins it, the same.
    let opened = symphony::taskview::connect_in(&beta.root, pinned(&beta.root, "1000000")).unwrap();
    assert_eq!(
        opened.get("beta/newer").unwrap().title,
        "By the newer build"
    );
    let why = opened
        .say("beta/one", "owner", "from the Symphony", None)
        .unwrap_err()
        .to_string();
    let program = Program::this();
    assert_eq!(
        why,
        format!(
            "the beta board is at schema {}, and this build writes only a board at its own \
             schema, {}: nothing was written; rebuild {} ({}) at the board's schema, and `just \
             symphony` in beta brings its Symphony to it",
            newer.version(),
            Schema::this_build().version(),
            program.name,
            program.path
        )
    );
    assert_eq!(migrated.thread("beta/one").unwrap().len(), 1);
    // Alpha's board is this build's still, to read and to write.
    symphony::taskview::connect_in(&alpha.root, env(&alpha.root))
        .unwrap()
        .say("alpha/one", "owner", "from the Symphony", None)
        .unwrap();
    alpha.reads(&["alpha/one"]);
    let why = symphony::taskview::connect_in(&alpha.root, pinned(&alpha.root, "soon"))
        .err()
        .unwrap();
    assert_eq!(why, "TASKS_NOW=soon is not a time in seconds");
}

#[test]
fn a_hub_never_moves_a_board_behind_its_build_forward_and_reads_it_once_its_command_has() {
    let tree = Tree::new();
    let gamma = Project::made_by(&tree, "gamma", older_writer);
    let (before, this) = (
        Schema::this_build().before().version(),
        Schema::this_build().version(),
    );
    let why = gamma.refused("the gamma board");
    assert_eq!(
        why,
        format!(
            "the gamma board is at schema {before}, behind this build's {this}, and a reader \
             never moves a board forward: nothing was read or written; `just tasks migrate` in \
             the main checkout of gamma moves it once the project's board command is at schema \
             {this}, and a newer schema is tried on a scratch board (`TASKS_DATABASE=NAME`)"
        )
    );
    // What the Symphony's write routes open is refused the same way, before anything is written,
    // and so is what its command line opens in a linked worktree, which reaches the same board.
    let opened = symphony::taskview::connect_in(&gamma.root, env(&gamma.root));
    assert_eq!(opened.err().as_ref(), Some(&why));
    let worktree = tree.path("gamma-feature");
    git(
        &gamma.root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feature",
            worktree.to_str().unwrap(),
        ],
    );
    let opened = symphony::taskview::connect_in(&worktree, env(&worktree));
    assert_eq!(opened.err(), Some(why));
    assert_eq!(gamma.version(), Some(before));
    // The project's board command of the older build reads and writes it still.
    older_writer(&gamma.root)
        .say("gamma/one", "owner", "after the Symphony", None)
        .unwrap();
    // An ordinary command of this build does not move it: gamma's main checkout holds no board
    // source, so only `tasks migrate` there does.
    let refused = symphony_board::cli::connect(&env(&gamma.root))
        .err()
        .unwrap();
    assert!(
        refused.contains("only `just tasks migrate` run in its main checkout"),
        "{refused}"
    );
    assert_eq!(gamma.version(), Some(before));
    // Once `tasks migrate` of this build has moved it forward, the hub reads it.
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = symphony_board::cli::main(
        &["migrate".to_string()],
        &env(&gamma.root),
        &mut std::io::empty(),
        &mut out,
        &mut err,
    );
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&err));
    assert_eq!(
        String::from_utf8(out).unwrap(),
        format!("✔ moved the gamma board from schema {before} to {this}\n")
    );
    gamma.reads(&["gamma/one"]);
    assert_eq!(gamma.version(), Some(this));
}

/// A board's server, stopped when the case ends: one a hub that should start none started all the
/// same.
struct Stopped(Home);

impl Drop for Stopped {
    fn drop(&mut self) {
        let _ = self.0.stop_board();
    }
}

/// A project in its own repository under another checkout's artifacts, as a test's scratch
/// projects are, removed under its hub, or left without its `.git`, once its board's server has
/// stopped (the case that ended it stopped it, nothing reaching the board in between): the hub
/// says once that its folder is gone and reads nothing more, as a project with no board, so the
/// server's maintenance of it waits. It starts no board's server, the outer checkout's least of
/// all, whose board is made and down, and so would be started by a lookup that walked up to it,
/// and none runs on its removed data.
#[test]
fn a_project_removed_under_its_hub_starts_and_reads_no_board_of_the_checkout_above_it() {
    type Remove = fn(&Path) -> std::io::Result<()>;
    for (case, remove) in [
        (
            "its folder removed",
            (|root: &Path| std::fs::remove_dir_all(root)) as Remove,
        ),
        ("its .git removed", |root: &Path| {
            std::fs::remove_dir_all(root.join(".git"))
        }),
    ] {
        let tree = Tree::new();
        let outer = repository_at(&tree, "outer");
        std::fs::write(outer.join("symphony.toml"), "[project]\nid = \"outer\"\n").unwrap();
        writer(&outer)
            .create(&NewTask::new("outer/one", "The outer checkout's", "owner"))
            .unwrap();
        let outer_board = Stopped(Home::in_common(&outer.join(".git"), Some(&surreal())));
        assert!(outer_board.0.stop().unwrap().is_some(), "{case}");
        let inner = Project::made_in(&tree, "outer/artifacts/scratch/inner", "inner", writer);
        inner.reads(&["inner/one"]);
        assert!(inner.hub.holds_its_project(), "{case}");
        let stopped = inner.unreached(|| {
            let stopped = inner.home().stop().unwrap();
            remove(&inner.root).unwrap();
            stopped
        });
        assert!(stopped.is_some(), "{case}: its board's server ran");

        let why = inner.refused("the project's folder");
        assert_eq!(why, inner.hub.gone(), "{case}");
        assert!(
            why.ends_with(symphony_board::project::GONE),
            "{case}: {why}"
        );
        assert_eq!(inner.hub.has_board(), Some(false), "{case}");
        assert_eq!(
            inner.hub.reader().err().as_ref(),
            Some(&why),
            "{case}: a page's read reaches no board either"
        );
        assert_eq!(
            outer_board.0.recorded(),
            None,
            "{case}: the outer board started"
        );
        // A server on its data that its removal left running has no record left to say so: the
        // process table is read.
        assert_eq!(
            inner.home().serving(),
            None,
            "{case}: its own board's server runs, started again before the removal or after it"
        );
    }
}

/// A project in its own repository under another checkout's artifacts, its `.git` removed after
/// its hub's tick found that its folder holds it and before that tick's reach of its board looked
/// the board up: the reach, held to the repository the hub was made for, is refused as gone, never
/// walks up to the outer checkout's board, whose server is down and would be started, and starts
/// its own project's none. The hub then says its folder is gone.
#[test]
fn a_reach_whose_projects_git_goes_after_its_hubs_check_starts_no_board_above_it() {
    let tree = Tree::new();
    let outer = repository_at(&tree, "outer");
    std::fs::write(outer.join("symphony.toml"), "[project]\nid = \"outer\"\n").unwrap();
    writer(&outer)
        .create(&NewTask::new("outer/one", "The outer checkout's", "owner"))
        .unwrap();
    let outer_board = Stopped(Home::in_common(&outer.join(".git"), Some(&surreal())));
    assert!(outer_board.0.stop().unwrap().is_some());
    let inner = Project::made_in(&tree, "outer/artifacts/scratch/inner", "inner", writer);
    inner.reads(&["inner/one"]);

    // Its board's server stopped, the hub's next tick reaches the board again.
    let (there, go) = inner.paused();
    assert!(inner.home().stop().unwrap().is_some());
    test_time::until_true("the hub to reach its board again", BACKSTOP, || {
        there.try_recv().is_ok()
    })
    .unwrap();
    std::fs::remove_dir_all(inner.root.join(".git")).unwrap();
    go.send(()).unwrap();

    let why = inner.refused(&inner.hub.gone());
    assert_eq!(why, inner.hub.gone());
    assert_eq!(outer_board.0.recorded(), None, "the outer board started");
    assert_eq!(inner.home().serving(), None, "its own board's server runs");
}

/// A served project left without its `.git` after its hub's check found that its folder holds it:
/// in its own repository under another checkout's artifacts, as a test's scratch projects are, its
/// `.git` removed once `before` has run on it and its board's server has stopped, its hub kept from
/// the board meanwhile and waited on until it says its folder is gone. So what a case calls next is
/// a reach made after that check, as a tick's, a refresh's or a route's is. The outer checkout's
/// board is made and down, served by the `surreal` pinned in its common dir as a live board's is,
/// which a reach in this process, with no `SYMPHONY_SURREAL`, would start it on.
struct Gitless {
    inner: Project,
    outer_board: Stopped,
    outer: PathBuf,
    /// Removed once the case's servers have stopped.
    _tree: Tree,
}

impl Gitless {
    fn new<T>(before: impl FnOnce(&Tree, &Project) -> T) -> (Gitless, T) {
        let tree = Tree::new();
        let outer = repository_at(&tree, "outer");
        std::fs::write(outer.join("symphony.toml"), "[project]\nid = \"outer\"\n").unwrap();
        writer(&outer)
            .create(&NewTask::new("outer/one", "The outer checkout's", "owner"))
            .unwrap();
        let outer_board = Stopped(Home::in_common(&outer.join(".git"), None));
        let pinned = outer_board.0.surreal();
        std::fs::create_dir_all(pinned.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(surreal(), pinned).unwrap();
        assert!(outer_board.0.stop().unwrap().is_some());
        let inner = Project::made_in(&tree, "outer/artifacts/scratch/inner", "inner", writer);
        inner.reads(&["inner/one"]);
        let made = before(&tree, &inner);
        inner.unreached(|| {
            inner.home().stop().unwrap();
            std::fs::remove_dir_all(inner.root.join(".git")).unwrap();
        });
        inner.refused(&inner.hub.gone());
        let case = Gitless {
            inner,
            outer_board,
            outer,
            _tree: tree,
        };
        (case, made)
    }

    /// Whether the outer checkout's board stayed down: no reach started it.
    fn outer_down(&self) -> bool {
        self.outer_board.0.recorded().is_none()
    }

    /// The words a reach of the inner project's board is refused in: its folder no longer holds
    /// the repository it was served from.
    fn held(&self) -> String {
        format!(
            "the project's folder {} no longer holds the repository it was served from, {}: {}",
            self.inner.root.display(),
            self.inner.root.join(".git").display(),
            symphony_board::project::GONE
        )
    }

    /// What the project's route answers `request` once front's check that its folder holds it has
    /// passed: the save or the read itself, as `front::answer` calls it after that check.
    fn past_the_check(&self, request: Request<Body>) -> (u16, Value) {
        let shared = Shared::new(1, 1234, &self.inner.root, None, crate::testing::open_gate());
        assert!(shared.api.set(self.inner.hub.clone()).is_ok());
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let response = if crate::write::handles(&request) {
                crate::write::answer(&shared, request).await
            } else {
                let (parts, _) = request.into_parts();
                crate::api::answer(&shared, Request::from_parts(parts, ()))
                    .await
                    .unwrap()
            };
            let status = response.status().as_u16();
            let bytes = to_bytes(response.into_body(), 1_000_000).await.unwrap();
            let said = serde_json::from_slice(&bytes)
                .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into()));
            (status, said)
        })
    }
}

/// The shared server's chat flush, run on its tick's store once the tick's check passed, reads the
/// journal its project had when the hub was made and reaches the board held to that project's
/// repository: with the `.git` gone it is refused as gone, so the owner's message waits, and it
/// neither reads the outer checkout's journal, which a path found from the folder again would be,
/// nor hands anything to the outer board, which it would start.
#[test]
fn a_chat_flush_after_its_projects_git_went_reaches_no_journal_or_board_above_it() {
    let (case, mut flusher) = Gitless::new(|_, inner| {
        let flusher = symphony::callbacks::Flusher::new(&inner.root);
        symphony::callbacks::publish(&inner.root, "From the owner").unwrap();
        flusher
    });
    assert_eq!(flusher.flush(&case.inner.root), Err(case.held()));
    assert!(case.outer_down(), "the outer board started");
    assert!(
        !case.outer.join("artifacts/asks").exists(),
        "the outer checkout's journal was read"
    );
}

/// The branch scan's read of a running board (`taskview::connect_running`), made after its check
/// on a project whose `.git` went, is refused as gone, and never reads the outer checkout's board,
/// running as it is.
#[test]
fn a_branch_scans_read_after_its_projects_git_went_reads_no_running_board_above_it() {
    let (case, ()) = Gitless::new(|_, _| ());
    writer(&case.outer)
        .get("outer/one")
        .expect("the outer board runs");
    assert!(!case.outer_down());
    let refused = symphony::taskview::connect_running(&case.inner.root)
        .err()
        .expect("the read is refused");
    assert_eq!(refused, case.held());
}

/// A page's save and its read of the asks, each of which reaches a board for itself once front's
/// check that the folder holds its project has passed, on a project whose `.git` went after that
/// check: the save is refused as the board's reach is, and writes nothing above it; the read, of a
/// hub that has no chat recipient from a board read yet, asks the board for none; neither starts
/// the outer checkout's board.
#[test]
fn a_pages_save_or_asks_read_past_its_check_after_its_projects_git_went_starts_no_board_above_it() {
    let (case, ()) = Gitless::new(|_, _| ());
    let save = json!({ "slug": "outer/one", "priority": 0 });
    assert_eq!(
        case.past_the_check(asked("POST", "/api/task-priority", Some(&save))),
        (
            503,
            json!({ "error": format!("the task board cannot be reached: {}", case.held()) })
        ),
        "the save"
    );
    assert!(case.outer_down(), "the save started the outer board");
    // As a hub that has not read its board yet: the read connects for the recipient itself.
    *case.inner.hub.chat.lock().unwrap() = None;
    let (status, said) = case.past_the_check(asked("GET", "/api/asks?path=docs/TOPIC.md", None));
    assert_eq!(status, 200, "{said}");
    assert!(case.outer_down(), "the asks read started the outer board");
}

/// A page's question and its read of the asks, past front's check on a project whose `.git` went
/// after that check, keep to the project's own journal, the one found when its server began to
/// serve it: the question is saved there, with the chat line its save journals for an agent, and
/// read back from there. Neither makes nor reads the outer checkout's journal, which a path found
/// from the folder again would be, and whose own server would hand that chat line to the outer
/// checkout's board.
#[test]
fn a_pages_ask_and_asks_read_after_its_projects_git_went_keep_to_its_own_journal() {
    let (case, ()) = Gitless::new(|_, _| ());
    let question = json!({ "path": "docs/TOPIC.md", "question": "Is it held?" });
    let (status, said) = case.past_the_check(asked("POST", "/api/ask", Some(&question)));
    assert_eq!(status, 200, "the ask: {said}");
    let id = said["id"].as_i64().expect("the ask's id");
    assert!(
        !case.outer.join("artifacts/asks").exists(),
        "the ask made the outer checkout's journal"
    );
    let (status, said) = case.past_the_check(asked("GET", "/api/asks?path=docs/TOPIC.md", None));
    assert_eq!(status, 200, "the asks read: {said}");
    let read: Vec<(i64, String)> = said["asks"]
        .as_array()
        .expect("the asks read lists the asks")
        .iter()
        .map(|ask| (ask["id"].as_i64().unwrap(), ask["question"].to_string()))
        .collect();
    assert_eq!(read, [(id, json!("Is it held?").to_string())]);
    assert!(
        !case.outer.join("artifacts/asks").exists(),
        "the asks read made the outer checkout's journal"
    );
    let journal =
        symphony::asks::AskStore::existing(&case.inner.root.join("artifacts/asks/asks.db"))
            .expect("the project's own journal");
    assert_eq!(journal.thread(id).unwrap().question, "Is it held?");
    assert!(case.outer_down(), "the ask started the outer board");
}

/// A page on this machine asking `method` at `uri`, with `body` as its JSON.
pub(super) fn asked(method: &str, uri: &str, body: Option<&Value>) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("Host", "localhost:1234")
        .header("Content-Type", "application/json")
        .extension(ConnectInfo("127.0.0.1:1234".parse::<SocketAddr>().unwrap()))
        .body(body.map_or_else(Body::empty, |body| Body::from(body.to_string())))
        .unwrap()
}

/// What the project's routes answer `request`: the status, and the JSON said, or else the text.
pub(super) fn answer(shared: &Arc<Shared>, request: Request<Body>) -> (u16, Value) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let response = router(shared.clone()).oneshot(request).await.unwrap();
        let status = response.status().as_u16();
        let bytes = to_bytes(response.into_body(), 1_000_000).await.unwrap();
        let said = serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into()));
        (status, said)
    })
}

/// A project in its own repository under another checkout's artifacts, emptied under its server or
/// left without its `.git`, once its board's server has stopped: a page's save on it and its read
/// of the task board's search, each of which reaches a board for itself, are refused before any
/// route runs, saying its folder no longer holds it. Neither starts the outer checkout's board,
/// whose pinned `surreal` is in its common dir as a live board's is, nor writes there: a lookup
/// from the folder that walks up to the outer checkout would do both.
#[test]
fn a_pages_save_or_read_on_a_project_emptied_under_its_server_reaches_no_board_above_it() {
    type Empty = fn(&Path) -> std::io::Result<()>;
    let emptied = (|root: &Path| {
        for entry in std::fs::read_dir(root)? {
            let path = entry?.path();
            if path.is_dir() {
                std::fs::remove_dir_all(&path)?;
            } else {
                std::fs::remove_file(&path)?;
            }
        }
        Ok(())
    }) as Empty;
    let gitless = |root: &Path| std::fs::remove_dir_all(root.join(".git"));
    for (case, empty) in [("emptied", emptied), ("its .git removed", gitless)] {
        let tree = Tree::new();
        let outer = repository_at(&tree, "outer");
        std::fs::write(outer.join("symphony.toml"), "[project]\nid = \"outer\"\n").unwrap();
        let priority = || writer(&outer).get("outer/one").unwrap().priority;
        writer(&outer)
            .create(&NewTask::new("outer/one", "The outer checkout's", "owner"))
            .unwrap();
        let before = priority();
        // Its board served by the `surreal` pinned in its common dir, which a reach with no
        // `SYMPHONY_SURREAL` (a route's, in this process) starts it on.
        let outer_board = Stopped(Home::in_common(&outer.join(".git"), None));
        let pinned = outer_board.0.surreal();
        std::fs::create_dir_all(pinned.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(surreal(), pinned).unwrap();
        assert!(outer_board.0.stop().unwrap().is_some(), "{case}");

        let inner = Project::made_in(&tree, "outer/artifacts/scratch/inner", "inner", writer);
        inner.reads(&["inner/one"]);
        let shared = Shared::new(1, 1234, &inner.root, None, crate::testing::open_gate());
        assert!(shared.api.set(inner.hub.clone()).is_ok());
        let save = |slug: &str| json!({ "slug": slug, "priority": (before + 1) % 5 });
        let save = |slug: &str| asked("POST", "/api/task-priority", Some(&save(slug)));
        let read = |words: &str| asked("GET", &format!("/api/task-board?q={words}"), None);
        assert_eq!(
            answer(&shared, save("inner/one")),
            (200, json!({ "ok": true })),
            "{case}: its own board saved to while its folder holds it"
        );
        assert_eq!(
            answer(&shared, read("made")).0,
            200,
            "{case}: its own board read while its folder holds it"
        );
        // Its hub kept from the board meanwhile: its next tick would start the stopped server
        // again, which writes into the folder as it is emptied.
        inner.unreached(|| {
            inner.home().stop().unwrap();
            empty(&inner.root).unwrap();
        });
        assert_eq!(
            symphony_board::project::locate_quick(&inner.root).map(|place| place.common),
            Some(outer.canonicalize().unwrap().join(".git")),
            "{case}: a reach from the folder now finds the outer checkout's repository"
        );

        let refused = json!({ "error": inner.hub.gone() });
        assert_eq!(
            answer(&shared, save("outer/one")),
            (503, refused.clone()),
            "{case}: the save"
        );
        assert_eq!(
            answer(&shared, read("one")),
            (503, refused),
            "{case}: the read"
        );
        // A browser loading one of its pages is told the same, as a page.
        let mut page = asked("GET", "/tasks", None);
        page.headers_mut()
            .insert("sec-fetch-mode", "navigate".parse().unwrap());
        let (status, said) = answer(&shared, page);
        let said = said.as_str().unwrap_or_default().to_string();
        assert_eq!(status, 503, "{case}: {said}");
        assert!(
            said.contains("does not serve this project now")
                && said.contains(&symphony::markup::escape(&inner.hub.gone())),
            "{case}: {said}"
        );
        assert_eq!(
            outer_board.0.recorded(),
            None,
            "{case}: the outer board started"
        );
        assert_eq!(priority(), before, "{case}: the save wrote the outer board");
        assert!(outer_board.0.stop().unwrap().is_some(), "{case}");
    }
}

/// What a hub holds its folder to: there, and leading the board's lookup to the repository it led
/// to when the hub was made. A folder removed, emptied, or left without its `.git` leads that
/// lookup up into the checkout that holds it, and is held no more; a project's folder whose project
/// file went is held, its `.git` still stopping the lookup there; so is a folder below a checkout's
/// top, whose lookup always led up to it, the more so once it has a project file of its own, and a
/// folder no repository holds, while it is there.
#[test]
fn a_hub_holds_its_folder_while_the_boards_lookup_from_it_finds_its_repository() {
    let tree = Tree::new();
    let outer = repository_at(&tree, "outer");
    let hub = |root: &Path| Hub::new(root, &root.join("build/client"));
    let project = |name: &str| {
        let root = repository_at(&tree, &format!("outer/artifacts/{name}"));
        std::fs::write(root.join("symphony.toml"), "[project]\nid = \"inner\"\n").unwrap();
        (hub(&root), root)
    };

    let (unfiled, root) = project("unfiled");
    std::fs::remove_file(root.join("symphony.toml")).unwrap();
    assert!(unfiled.holds_its_project(), "its .git stops the lookup");
    std::fs::remove_dir_all(root.join(".git")).unwrap();
    assert!(!unfiled.holds_its_project(), "no .git");

    let (emptied, root) = project("emptied");
    for entry in std::fs::read_dir(&root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            std::fs::remove_dir_all(&path).unwrap();
        } else {
            std::fs::remove_file(&path).unwrap();
        }
    }
    assert!(root.is_dir());
    assert!(!emptied.holds_its_project(), "emptied");

    let (removed, root) = project("removed");
    assert!(removed.holds_its_project());
    std::fs::remove_dir_all(&root).unwrap();
    assert!(!removed.holds_its_project(), "removed");

    // As a board's start under way makes it again: its store's folders alone.
    let (remade, root) = project("remade");
    std::fs::remove_dir_all(root.join(".git")).unwrap();
    std::fs::create_dir_all(root.join(".git").join(symphony_board::folders::TASKS)).unwrap();
    assert!(!remade.holds_its_project(), "its .git made again");

    let below = outer.join("docs");
    std::fs::create_dir_all(&below).unwrap();
    let below_hub = hub(&below);
    assert!(below_hub.holds_its_project(), "below the checkout's top");
    std::fs::write(below.join("symphony.toml"), "[project]\nid = \"docs\"\n").unwrap();
    assert!(
        below_hub.holds_its_project(),
        "below the checkout's top, with a project file of its own: the same repository"
    );

    let plain = tree.path("plain");
    std::fs::create_dir_all(&plain).unwrap();
    let plain_hub = hub(&plain);
    assert!(plain_hub.holds_its_project(), "in no repository");
    std::fs::remove_dir_all(&plain).unwrap();
    assert!(!plain_hub.holds_its_project(), "in no repository, removed");
}
