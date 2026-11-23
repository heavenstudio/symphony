//! The maintenance of each project the one server serves beside its starter's: started as its state
//! is first served, stopped as it is retired or the server stops, each project on a schedule of its
//! own, so one project's failing or hung board never holds up another's; the registry read again on
//! the server's own look, so registering or removing a project needs no request. Every project is a
//! checkout in a temporary folder registered in a temporary machine folder, its board a scratch one
//! and its `tasks` a stand-in: nothing here reaches the shared server or a live board.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use symphony::machine::{Entry, Registry};

use super::{Maintain, Projects, Warming};
use crate::backstop::NO_PERIOD;
use crate::front::Shared;
use crate::front::test_clock::TestClock;
use crate::testing::handing::{Handing, event, renamed, tell_at, watching};
use crate::testing::{Tree, joined};

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
            crate::testing::open_gate(),
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
            shared.wait_for_stop();
            said.lock().unwrap().push(format!("ended {id}"));
        })]
    })
}

/// Whether `said` holds `line`.
fn says(said: &Said, line: &str) -> bool {
    said.lock().unwrap().iter().any(|said| said == line)
}

/// A project registered while the server runs has its maintenance started, and one removed has it
/// stopped, by the server's own look at the registry, with no request, woken by its watcher of the
/// machine folder (its backstop's period is past the test); the starter's checkout is never
/// maintained here (the server's own workers maintain it), and the server's stop ends every other
/// project's and the look, whose watcher goes with it. The test tells the watcher's events itself:
/// under load a real backend can lose one outright, which the look's period is for
/// (`backstop.rs`), and on no period a lost event would leave the look waiting for good.
#[test]
fn registering_or_removing_a_project_starts_or_stops_its_maintenance_with_no_request() {
    let machine = Machine::new(&[("alpha", "exit 0"), ("beta", "exit 0")]);
    let folder = machine.tree.path("machine");
    let said = Said::default();
    let projects = machine.projects(recording(&said));
    let look = projects.watch_with::<Handing<false>>(NO_PERIOD);
    assert_eq!(*said.lock().unwrap(), ["started beta"]);
    let beta = state(&projects, "beta");
    assert_eq!(beta.maintaining(), 1);
    test_time::until_true("the look to watch the machine folder", WAIT, || {
        watching(&folder) == Some(notify::RecursiveMode::NonRecursive)
    })
    .unwrap();

    machine.register("gamma", "exit 0");
    tell_at(&folder, renamed(folder.join(super::PROJECTS)));
    test_time::until_true("gamma's maintenance started", WAIT, || {
        says(&said, "started gamma")
    })
    .unwrap();
    machine.remove("beta");
    tell_at(&folder, renamed(folder.join(super::PROJECTS)));
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
    joined(look, "the stop to end the look at the registry", WAIT);
    assert_eq!(watching(&folder), None, "the watcher goes with the look");
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

#[test]
fn the_look_is_woken_by_a_change_of_the_registry_or_current_alone() {
    use notify::EventKind::{Access, Create, Modify, Remove};
    use notify::event::{AccessKind, CreateKind, ModifyKind, RemoveKind, RenameMode};
    let into_place = Modify(ModifyKind::Name(RenameMode::To));
    // The registry written beside itself and renamed into place, and `current` moved.
    for path in ["/m/projects.json", "/m/current"] {
        for kind in [
            Create(CreateKind::File),
            Modify(ModifyKind::Any),
            into_place,
            Remove(RemoveKind::Any),
        ] {
            assert!(
                super::registry_event(&event(kind, [path])),
                "{kind:?} {path}"
            );
        }
        assert!(!super::registry_event(&event(
            Access(AccessKind::Any),
            [path]
        )));
    }
    // The registry's lock, its write under way and a release folder say nothing yet.
    for path in ["/m/projects.lock", "/m/projects.json.4242", "/m/releases"] {
        assert!(!super::registry_event(&event(into_place, [path])), "{path}");
    }
    // What a lost or failing stream says: read it again.
    assert!(super::registry_event(&Ok(notify::Event::new(Modify(
        ModifyKind::Any
    ))
    .set_flag(notify::event::Flag::Rescan))));
    assert!(super::registry_event(&Err(notify::Error::generic(
        "stream ended"
    ))));
}

#[test]
fn the_registry_watch_wakes_the_look_once_a_burst_and_one_that_cannot_watch_wakes_nothing() {
    // A folder no other test watches (`testing::handing`).
    let machine = std::path::Path::new("/registry-watch");
    let wakes = crate::backstop::Wakes::new();
    let watcher = super::watch_registry::<Handing<false>>(machine, wakes.waker())
        .expect("the machine folder watched");
    assert_eq!(
        watcher.watched,
        vec![(machine.to_path_buf(), notify::RecursiveMode::NonRecursive)]
    );
    let tell = |name: &str| watcher.tell(renamed(machine.join(name)));
    tell("projects.json.4242");
    assert!(!wakes.wait(Some(Duration::ZERO)), "a write under way");
    tell("projects.json");
    tell("projects.json");
    assert!(
        wakes.wait(Some(Duration::ZERO)),
        "the registry renamed into place"
    );
    assert!(!wakes.wait(Some(Duration::ZERO)), "its burst is one wake");

    let unwatched = crate::backstop::Wakes::new();
    let failed = super::watch_registry::<Handing<true>>(machine, unwatched.waker());
    assert!(failed.is_none());
    assert!(
        !unwatched.wait(Some(Duration::ZERO)),
        "the backstop's period reads alone"
    );
}

/// The projects a server builds as it starts ([`Projects::new`]) hold no maintenance until its
/// generation is live, its socket hand-off committed, when the server starts their watch: a
/// replacement killed by a failed hand-off stops no state, so nothing it started may run until
/// then. A project a request serves before then waits too; from then, each has its sweep, and the
/// server's stop joins it.
#[test]
fn no_project_is_maintained_until_the_generation_is_live() {
    let machine = Machine::new(&[("alpha", "exit 0"), ("beta", "exit 0")]);
    let projects = Projects::new(&machine.tree.path("machine"), &machine.starter);
    // A request before the hand-off commits reads the registry again, a project registered since.
    machine.register("gamma", "exit 0");
    projects.served();
    let [beta, gamma] = ["beta", "gamma"].map(|id| state(&projects, id));
    assert_eq!((beta.maintaining(), gamma.maintaining()), (0, 0));

    let look = projects.watch();
    assert_eq!(
        (beta.maintaining(), gamma.maintaining()),
        (1, 1),
        "each its idle sweep"
    );
    machine.starter.stop();
    joined(look, "the stop to end the look at the registry", WAIT);
    projects.stop();
    assert_eq!((beta.maintaining(), gamma.maintaining()), (0, 0));
}

/// A generation whose hand-off failed (its starter stopped before it went live) starts no
/// project's maintenance, whatever it served.
#[test]
fn a_generation_stopped_before_it_went_live_maintains_no_project() {
    let machine = Machine::new(&[("alpha", "exit 0"), ("beta", "exit 0")]);
    let said = Said::default();
    let projects = machine.projects(recording(&said));
    machine.starter.stop();
    projects.watch().join().unwrap();
    projects.stop();
    assert_eq!(*said.lock().unwrap(), Vec::<String>::new());
    assert_eq!(state(&projects, "beta").maintaining(), 0);
}

/// A registered checkout of the starter's under another id is not maintained twice: the server's
/// own workers maintain that board, under whatever id.
#[test]
fn the_starters_checkout_under_another_id_is_not_maintained_beside_it() {
    let machine = Machine::new(&[("alpha", "exit 0")]);
    let said = Said::default();
    let projects = machine.projects(recording(&said));
    let look = projects.watch();
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
    machine.starter.stop();
    joined(look, "the stop to end the look at the registry", WAIT);
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
        waits.wait_for_stop();
    })]);
    assert_eq!(shared.maintaining(), 2);
    shared.stop();
    assert_eq!(shared.maintaining(), 0);

    let ended = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let late = ended.clone();
    let waits = shared.clone();
    shared.maintained(vec![std::thread::spawn(move || {
        waits.wait_for_stop();
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
    // A round each time the test moves the clock on by the interval, a hung one left hanging until
    // the stop: its timeout is past every move the test makes.
    const ROUND: Duration = Duration::from_secs(300);
    let clock = TestClock::new();
    let rounds = clock.clone();
    let projects = machine.projects(Box::new(move |shared: &Arc<Shared>| {
        vec![crate::front::maintenance_worker(
            shared.clone(),
            ROUND,
            Duration::from_secs(86_400),
            rounds.reader(),
            None,
        )]
    }));
    // Each project's next round: the clock moved on, once each project named has swept `more`
    // times since `since`, the counts `count` reads.
    let next_rounds = |what: &str, since: u64, count: &dyn Fn() -> u64| {
        for more in 1..=2 {
            clock.advance(ROUND);
            test_time::until_true(format!("{what}: round {more}"), WAIT, || {
                count() >= since + more
            })
            .unwrap();
        }
    };
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
    // The generation is live: every other project's sweep starts.
    let look = projects.watch();
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
    let (ok, failed) = (sweeps("beta").ok, sweeps("gamma").failed);
    next_rounds("beta swept and gamma failed", 0, &|| {
        (sweeps("beta").ok - ok).min(sweeps("gamma").failed - failed)
    });
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
    next_rounds("gamma swept", failed, &|| sweeps("gamma").failed);
    assert_eq!(machine.lines("beta", "swept").len(), runs);
    assert_eq!(sweeps("beta").ok, ok);

    // The server's stop ends delta's hung reap with its worker.
    machine.starter.stop();
    joined(look, "the stop to end the look at the registry", WAIT);
    projects.stop();
    assert!(!crate::sys::pid_alive(hung), "the stop ended the hung reap");
    assert_eq!(state(&projects, "delta").maintaining(), 0);
}
