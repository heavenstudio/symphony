//! Each project's hub on its own board: two fixture projects, each its own SurrealDB server in its
//! repository's common dir in a temporary folder, started by its first caller. A hub reconnects
//! when its board's server restarts, and reads on when a newer build migrates its board, while
//! every write the Symphony would send there is refused naming the project. A board behind the
//! hub's build it never moves forward: it says why it reads nothing until the board's command has.

use super::Hub;
use crate::testing::{Tree, board::surreal, git, repository_at};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use symphony_board::cli::Env;
use symphony_board::rpc::Client;
use symphony_board::server::Home;
use symphony_board::{NewTask, Opening, Schema, Store};

/// The wall-clock backstop of a wait here on the hub's read: only a stall reaches it (RULES.md
/// `tests/no-test-fails-on-load`).
const BACKSTOP: Duration = test_time::backstop(Duration::from_secs(30));

/// A fixture project: its checkout, holding `symphony.toml` with its id, and its hub.
struct Project {
    root: PathBuf,
    hub: Arc<Hub>,
}

impl Project {
    /// The project `id` in `tree`, its board holding the task `<id>/one`, its hub reading it.
    fn new(tree: &Tree, id: &str) -> Project {
        Project::made_by(tree, id, writer)
    }

    /// As `new`, its board made by `make`, which opens it to write.
    fn made_by(tree: &Tree, id: &str, make: impl FnOnce(&Path) -> Store) -> Project {
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
        let reach = env(&root);
        let hub = Hub::start_watching::<notify::NullWatcher>(
            &root,
            &root.join("build/client"),
            false,
            false,
            crate::backstop::PERIOD,
            move |root| symphony::taskview::connect_in(root, reach.clone()),
        )
        .unwrap();
        hub.start_initial_board();
        Project { root, hub }
    }

    fn home(&self) -> Home {
        Home::in_common(&self.root.join(".git"), Some(&surreal()))
    }

    /// The schema version the project's board records.
    fn version(&self) -> Option<i64> {
        Client::new(&self.home().recorded().unwrap().addr, "sf", "tasks")
            .query(
                "RETURN (SELECT VALUE version FROM ONLY meta:schema);",
                serde_json::json!({}),
            )
            .unwrap()
            .last()
            .and_then(serde_json::Value::as_i64)
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
    fn reads(&self, slugs: &[&str]) {
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
        let _ = self.home().stop();
    }
}

/// What a caller in `root` reads: the project's own board, its server started if it is down.
fn env(root: &Path) -> Env {
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
fn writer(root: &Path) -> Store {
    symphony_board::cli::connect(&env(root)).unwrap()
}

/// The board of `root`'s project, opened to write by a build one schema version behind this one:
/// the project's board command while this build's schema has not landed.
fn older_writer(root: &Path) -> Store {
    let reached = symphony_board::cli::board(&env(root)).unwrap();
    Store::opening(
        reached.client,
        Box::new(|| 1_000_000.0),
        Box::new(|_| false),
        "this-host",
        &Opening::write()
            .at(Schema::this_build().before())
            .of(reached.project.as_deref()),
    )
    .unwrap()
}

/// The board of `root`'s project, moved forward to `schema` by a newer build as its `tasks migrate`
/// in the project's main checkout moves it, which alone moves the live board of a project that
/// commits no board source: reached as that command reaches it, and opened to migrate.
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
            .migrating(),
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
    assert_eq!(
        why,
        format!(
            "the beta board is at schema {}, and this build writes only a board at its own \
             schema, {}: nothing was written; `just symphony` in beta brings its Symphony to the \
             board's schema",
            newer.version(),
            Schema::this_build().version()
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
        let _ = self.0.stop();
    }
}

/// A project in its own repository under another checkout's artifacts, as a test's scratch
/// projects are, removed under its hub, or left without its `.git`, once its board's server has
/// stopped (the case that ended it stopped it): the hub says once that its folder is gone and
/// reads nothing more, as a project with no board, so the server's maintenance of it waits. It
/// starts no board's server, the outer checkout's least of all, whose board is made and down, and
/// so would be started by a lookup that walked up to it.
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
        inner.home().stop().unwrap();
        remove(&inner.root).unwrap();

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
        assert_eq!(
            inner.home().recorded(),
            None,
            "{case}: its own board started"
        );
    }
}

/// What a hub holds its folder to: there, and leading the board's lookup where it led when the hub
/// was made. A folder removed, emptied, or left without its `.git` leads that lookup up into the
/// checkout that holds it, and is held no more; a project's folder whose project file went is
/// held, its `.git` still stopping the lookup there; so is a folder below a checkout's top, whose
/// lookup always led up to it, and a folder no repository holds, while it is there.
#[test]
fn a_hub_holds_its_folder_while_the_boards_lookup_from_it_stops_where_it_did() {
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

    let below = outer.join("docs");
    std::fs::create_dir_all(&below).unwrap();
    assert!(hub(&below).holds_its_project(), "below the checkout's top");

    let plain = tree.path("plain");
    std::fs::create_dir_all(&plain).unwrap();
    let plain_hub = hub(&plain);
    assert!(plain_hub.holds_its_project(), "in no repository");
    std::fs::remove_dir_all(&plain).unwrap();
    assert!(!plain_hub.holds_its_project(), "in no repository, removed");
}
