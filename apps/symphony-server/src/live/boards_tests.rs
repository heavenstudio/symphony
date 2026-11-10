//! Each project's hub on its own board: two fixture projects, each its own SurrealDB server in its
//! repository's common dir in a temporary folder, started by its first caller. A hub reconnects
//! when its board's server restarts, and reads on when a newer build migrates its board, while
//! every write the Symphony would send there is refused naming the project.

use super::Hub;
use crate::testing::{Tree, board::surreal, repository_at};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use symphony_board::cli::Env;
use symphony_board::rpc::Client;
use symphony_board::server::Home;
use symphony_board::{NewTask, Opening, Schema, Store};

/// A fixture project: its checkout, holding `symphony.toml` with its id, and its hub.
struct Project {
    root: PathBuf,
    hub: Arc<Hub>,
}

impl Project {
    /// The project `id` in `tree`, its board holding the task `<id>/one`, its hub reading it.
    fn new(tree: &Tree, id: &str) -> Project {
        let root = repository_at(tree, id);
        std::fs::write(
            root.join("symphony.toml"),
            format!("[project]\nid = \"{id}\"\n"),
        )
        .unwrap();
        writer(&root)
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

    /// The slugs of the tasks the hub last read, or why it reads no board.
    fn read(&self) -> Result<Vec<String>, String> {
        let state = self.hub.tasks.lock().unwrap();
        match (&state.error, &state.raw) {
            (Some(error), _) => Err(error.clone()),
            (None, Some(board)) => Ok(board.tasks.iter().map(|task| task.slug.clone()).collect()),
            (None, None) => Err("no read yet".into()),
        }
    }

    /// Wait for the hub to read `slugs`, for at most 30 s.
    fn reads(&self, slugs: &[&str]) {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let read = self.read();
            if read.as_ref().is_ok_and(|read| read == slugs) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "{}: {read:?}",
                self.root.display()
            );
            std::thread::sleep(Duration::from_millis(50));
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
        vars: [("SF_SURREAL".to_string(), surreal().display().to_string())].into(),
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
    let migrated = Store::opening(
        Client::new(&beta.home().recorded().unwrap().addr, "sf", "tasks"),
        Box::new(|| 1_000_000.0),
        Box::new(|_| false),
        "this-host",
        &Opening::write().at(newer.clone()),
    )
    .unwrap();
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
