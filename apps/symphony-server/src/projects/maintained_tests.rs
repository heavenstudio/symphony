//! The maintenance of each project the one server serves beside its starter's: started as its state
//! is first served, stopped as it is retired or the server stops, each project on a schedule of its
//! own, so one project's failing or hung board never holds up another's; the registry read again on
//! the server's own look, so registering or removing a project needs no request. Every project is a
//! checkout in a temporary folder registered in a temporary machine folder, its board a scratch one
//! and its `tasks` a stand-in: nothing here reaches the shared server or a live board.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use symphony::machine::{Entry, Registry};

use super::{Maintain, Projects, Warming};
use crate::front::{Shared, Stamp};
use crate::gate::Gate;
use crate::testing::Tree;

/// The backstop of every wait here: rounds that take milliseconds on a quiet machine.
const WAIT: Duration = test_time::backstop(Duration::from_secs(15));

/// A machine folder and the checkouts it registers, each holding a stand-in for its build of
/// `tasks` (`.target/bin/tasks`) that runs the script given for it.
struct Machine {
    tree: Tree,
    starter: Arc<Shared>,
}

impl Machine {
    /// `projects`, each an id and its stand-in's script, the first the starter, all registered.
    fn new(projects: &[(&str, &str)]) -> Machine {
        let tree = Tree::new();
        for (id, script) in projects {
            Machine::checkout(&tree, id, script);
        }
        let mut registry = Registry::default();
        for (id, _) in projects {
            registry.add(Entry::of(&tree.path(id)).unwrap()).unwrap();
        }
        registry.write(&tree.path("machine")).unwrap();
        tree.write("install/client/public/index.html", "<!DOCTYPE html>");
        let (starter, _) = projects[0];
        let starter = Shared::project(
            1,
            1,
            &tree.path(starter).canonicalize().unwrap(),
            Some(&tree.path("install")),
            Gate::new(Arc::new(|_| Ok(None)), Arc::new(Instant::now)),
            &format!("/{starter}"),
        );
        Machine { tree, starter }
    }

    /// The checkout of project `id`, a repository whose stand-in runs `script`.
    fn checkout(tree: &Tree, id: &str, script: &str) {
        crate::testing::repository_at(tree, id);
        tree.write(
            &format!("{id}/symphony.toml"),
            &format!("[project]\nname = \"{id}\"\nid = \"{id}\"\n"),
        );
        let tasks = tree.path(&format!("{id}/.target/bin/tasks"));
        std::fs::create_dir_all(tasks.parent().unwrap()).unwrap();
        crate::fixture_script::write(&tasks, &format!("#!/bin/sh\n{script}\n"), false);
    }

    /// Its projects, served as the server serves them, each other project's maintenance started by
    /// `maintain`.
    fn projects(&self, maintain: Box<Maintain>) -> Arc<Projects> {
        Projects::with(
            &self.tree.path("machine"),
            &self.starter,
            Warming::OnRequest,
            Some(maintain),
        )
    }

    /// Register the checkout of `id` too.
    fn register(&self, id: &str, script: &str) {
        Machine::checkout(&self.tree, id, script);
        Registry::change(&self.tree.path("machine"), |registry| {
            registry.add(Entry::of(&self.tree.path(id)).unwrap())
        })
        .unwrap();
    }

    /// Take `id` out of the registry.
    fn remove(&self, id: &str) {
        Registry::change(&self.tree.path("machine"), |registry| {
            registry.projects.retain(|entry| entry.id != id);
            Ok(())
        })
        .unwrap();
    }

    /// The lines of `file` in the checkout of `id`, none while it is not there.
    fn lines(&self, id: &str, file: &str) -> Vec<String> {
        std::fs::read_to_string(self.tree.path(&format!("{id}/{file}")))
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect()
    }
}

/// The state `projects` serves `id` from now.
fn state(projects: &Projects, id: &str) -> Arc<Shared> {
    projects
        .served()
        .into_iter()
        .find(|served| served.entry.id == id)
        .unwrap_or_else(|| panic!("{id} is not served"))
        .shared
}

/// What the recording maintenance said, in order: `started ID` as a project's state starts it and
/// `ended ID` as its worker ends.
type Said = Arc<Mutex<Vec<String>>>;

/// A maintenance that runs nothing but says when each project's starts and ends.
fn recording(said: &Said) -> Box<Maintain> {
    let said = said.clone();
    Box::new(move |shared: &Arc<Shared>| {
        let id = shared.base.trim_start_matches('/').to_string();
        said.lock().unwrap().push(format!("started {id}"));
        let (said, shared) = (said.clone(), shared.clone());
        vec![std::thread::spawn(move || {
            while !shared.wait(Duration::from_millis(20)) {}
            said.lock().unwrap().push(format!("ended {id}"));
        })]
    })
}

/// Whether `said` holds `line`.
fn says(said: &Said, line: &str) -> bool {
    said.lock().unwrap().iter().any(|said| said == line)
}

/// A project registered while the server runs has its maintenance started, and one removed has it
/// stopped, by the server's own look at the registry, with no request; the starter's checkout is
/// never maintained here (the server's own workers maintain it), and the server's stop ends every
/// other project's.
#[test]
fn registering_or_removing_a_project_starts_or_stops_its_maintenance_with_no_request() {
    let machine = Machine::new(&[("alpha", "exit 0"), ("beta", "exit 0")]);
    let said = Said::default();
    let projects = machine.projects(recording(&said));
    assert_eq!(*said.lock().unwrap(), ["started beta"]);
    let beta = state(&projects, "beta");
    assert_eq!(beta.maintaining(), 1);
    let look = projects.watch();

    machine.register("gamma", "exit 0");
    test_time::until_true("gamma's maintenance started", WAIT, || {
        says(&said, "started gamma")
    })
    .unwrap();
    machine.remove("beta");
    test_time::until_true("beta's maintenance ended", WAIT, || {
        says(&said, "ended beta")
    })
    .unwrap();
    test_time::until_true("beta's state stopped, its workers joined", WAIT, || {
        beta.stopping() && beta.maintaining() == 0
    })
    .unwrap();
    let served: Vec<String> = projects
        .served()
        .into_iter()
        .map(|served| served.entry.id)
        .collect();
    assert_eq!(served, ["alpha", "gamma"]);

    // The server's stop: the starter first, its look at the registry, then every other project.
    machine.starter.stop();
    look.join().unwrap();
    let gamma = state(&projects, "gamma");
    projects.stop();
    assert!(says(&said, "ended gamma"), "{said:?}");
    assert_eq!(gamma.maintaining(), 0);
    assert!(
        !said
            .lock()
            .unwrap()
            .iter()
            .any(|line| line.ends_with("alpha")),
        "the starter's checkout is the server's own to maintain: {said:?}"
    );
}

/// A registered checkout of the starter's under another id is not maintained twice: the server's
/// own workers maintain that board, under whatever id.
#[test]
fn the_starters_checkout_under_another_id_is_not_maintained_beside_it() {
    let machine = Machine::new(&[("alpha", "exit 0")]);
    let said = Said::default();
    let projects = machine.projects(recording(&said));
    machine.tree.write(
        "alpha/symphony.toml",
        "[project]\nname = \"alpha\"\nid = \"renamed\"\n",
    );
    Registry::change(&machine.tree.path("machine"), |registry| {
        registry.projects.clear();
        registry.add(Entry::of(&machine.tree.path("alpha")).unwrap())
    })
    .unwrap();
    let renamed = state(&projects, "renamed");
    assert!(!Arc::ptr_eq(&renamed, &machine.starter));
    assert_eq!(renamed.maintaining(), 0);
    projects.stop();
    assert_eq!(*said.lock().unwrap(), Vec::<String>::new());
}

/// A state's stop joins its maintenance even when a worker panicked, and maintenance handed to a
/// state that already stopped is joined at once, so none outlives it.
#[test]
fn a_stopped_state_joins_its_maintenance_a_panicked_or_a_late_one_too() {
    let machine = Machine::new(&[("alpha", "exit 0")]);
    let shared = machine.starter.clone();
    shared.maintained(vec![std::thread::spawn(|| {
        panic!("a maintenance worker's defect")
    })]);
    let waits = shared.clone();
    shared.maintained(vec![std::thread::spawn(move || {
        while !waits.wait(Duration::from_millis(20)) {}
    })]);
    assert_eq!(shared.maintaining(), 2);
    shared.stop();
    assert_eq!(shared.maintaining(), 0);

    let ended = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let late = ended.clone();
    let waits = shared.clone();
    shared.maintained(vec![std::thread::spawn(move || {
        while !waits.wait(Duration::from_millis(20)) {}
        late.store(true, std::sync::atomic::Ordering::SeqCst);
    })]);
    assert!(ended.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(shared.maintaining(), 0);
}

/// Each project's idle sweep runs its own `tasks reap` in its own checkout and is recorded in its
/// own board's health: a board whose sweep fails, or hangs, never stops another's; a project taken
/// out of the registry is swept no more; and the server's stop ends a hung sweep's child.
#[test]
fn every_project_is_swept_on_its_own_and_a_failing_or_removed_one_stops_no_other() {
    let machine = Machine::new(&[
        ("alpha", "printf '%s\\n' \"$1\" >> swept"),
        ("beta", "printf '%s\\n' \"$1\" >> swept"),
        (
            "gamma",
            "printf '%s\\n' \"$1\" >> swept; echo 'the board did not answer' >&2; exit 3",
        ),
        (
            "delta",
            "printf '%s' \"$$\" > entered.part; mv entered.part entered; exec sleep 86400",
        ),
    ]);
    // Rounds back to back, a hung one left hanging until the stop.
    let projects = machine.projects(Box::new(|shared: &Arc<Shared>| {
        vec![crate::front::maintenance_worker(
            shared.clone(),
            Duration::ZERO,
            Duration::from_secs(3600),
            Stamp::now,
        )]
    }));
    // Each project's hub reads a scratch board of its own: each has a board, so each is swept.
    let mut boards = Vec::new();
    let mut hubs = std::collections::BTreeMap::new();
    for id in ["alpha", "beta", "gamma", "delta"] {
        let shared = state(&projects, id);
        let hub = Arc::new(crate::live::Hub::new(
            &shared.root,
            &shared.root.join("build/client"),
        ));
        let board = crate::testing::board::Fixture::empty();
        hub.board_read(&board.store).unwrap();
        assert!(shared.api.set(hub.clone()).is_ok());
        boards.push(board);
        hubs.insert(id, hub);
    }
    let sweeps = |id: &str| {
        let hub = &hubs[id];
        hub.health.report(hub.now()).sweep
    };

    let hung: i64 = test_time::until("delta's sweep entered its reap", WAIT, || {
        machine
            .lines("delta", "entered")
            .first()
            .and_then(|pid| pid.parse().ok())
    })
    .unwrap();
    // While delta's reap hangs, beta is swept again and again, and gamma's fails each round.
    let ok = sweeps("beta").ok;
    test_time::until_true("beta swept twice more and gamma failed twice", WAIT, || {
        sweeps("beta").ok >= ok + 2 && sweeps("gamma").failed >= 2
    })
    .unwrap();
    assert!(crate::sys::pid_alive(hung), "delta's reap still hangs");
    let (beta, gamma) = (sweeps("beta"), sweeps("gamma"));
    assert_eq!(beta.failed, 0, "gamma's failures are its own: {beta:?}");
    assert_eq!(
        gamma.error.as_deref(),
        Some("exit status: 3: the board did not answer")
    );
    assert!(
        machine
            .lines("beta", "swept")
            .iter()
            .all(|run| run == "reap"),
        "{:?}",
        machine.lines("beta", "swept")
    );
    assert_eq!(
        machine.lines("alpha", "swept"),
        Vec::<String>::new(),
        "the starter's checkout is the server's own to sweep"
    );
    assert_eq!(sweeps("delta").ok + sweeps("delta").failed, 0);

    // Beta taken out: retired, its worker joined, it is swept no more while gamma goes on.
    let retired = state(&projects, "beta");
    machine.remove("beta");
    projects.served();
    test_time::until_true("beta's sweep stopped", WAIT, || {
        retired.stopping() && retired.maintaining() == 0
    })
    .unwrap();
    let (runs, ok) = (machine.lines("beta", "swept").len(), sweeps("beta").ok);
    let failed = sweeps("gamma").failed;
    test_time::until_true("gamma swept twice more", WAIT, || {
        sweeps("gamma").failed >= failed + 2
    })
    .unwrap();
    assert_eq!(machine.lines("beta", "swept").len(), runs);
    assert_eq!(sweeps("beta").ok, ok);

    // The server's stop ends delta's hung reap with its worker.
    projects.stop();
    assert!(!crate::sys::pid_alive(hung), "the stop ended the hung reap");
    assert_eq!(state(&projects, "delta").maintaining(), 0);
}
