//! The machine's ledger of schema updates: names, slugs, generations, latency and its file.

use super::*;
use crate::test_tree::Tree;

fn stamp(at: f64, source: &str) -> Stamp {
    Stamp {
        at,
        source: source.into(),
    }
}

#[test]
fn an_update_is_named_by_its_board_and_schema_and_its_generation_from_the_second() {
    assert_eq!(name("symphony", 23, 1), "symphony@23");
    assert_eq!(name("symphony", 23, 0), "symphony@23", "no generation 0");
    assert_eq!(name("alpha", 24, 2), "alpha@24.2");
    assert_eq!(
        slug("symphony", 23, 1),
        "tests/release-red-update-symphony-23"
    );
    assert_eq!(slug("alpha", 24, 3), "tests/release-red-update-alpha-24-g3");
    assert_eq!(
        slug("Alpha_Beta..x", 7, 1),
        "tests/release-red-update-alpha-beta-x-7",
        "a slug keeps letters and digits, in lower case, each other run one dash"
    );
    assert_eq!(slug("--", 7, 1), "tests/release-red-update-project-7");
    for (project, schema, generation) in [("alpha", 23, 1), ("a.b", 5, 4)] {
        let slug = slug(project, schema, generation);
        assert!(
            symphony_board::red::filed(&slug),
            "{slug} is filed in a red series, so it takes the red mark"
        );
    }
}

#[test]
fn its_latency_is_from_its_board_reaching_the_schema_and_from_a_release_serving_it() {
    let mut update = Update::new("symphony", 23, 1, Some(stamp(1000.0, "schema-seen.log")));
    assert!(update.open());
    assert_eq!(update.total(), None, "not served yet");
    update.available = Some(stamp(1600.0, "the release's manifest"));
    assert_eq!(update.after_available(), None);
    update.served = Some(stamp(4000.0, "the server's hand-off"));
    assert!(!update.open());
    assert_eq!(update.total(), Some(3000.0));
    assert_eq!(update.after_available(), Some(2400.0));
    // A release installed before the board reached the schema: what came after is from the board.
    update.available = Some(stamp(500.0, "the release's manifest"));
    assert_eq!(update.after_available(), Some(3000.0));
    // With no record of when the board reached it, the total is unknown, never a guess.
    update.reached = None;
    assert_eq!(update.total(), None);
    assert_eq!(update.after_available(), Some(3500.0));
    let mut left = Update::new("alpha", 23, 1, None);
    left.left = Some(stamp(10.0, "a refresh's look"));
    assert!(!left.open(), "a board that went back left its update");
}

#[test]
fn a_ledger_reads_empty_where_there_is_none_and_back_as_it_was_written() {
    let tree = Tree::new();
    let machine = tree.path("machine");
    assert_eq!(Ledger::read(&machine).unwrap().updates, Vec::new());
    let mut first = Update::new("symphony", 23, 1, Some(stamp(1.0, "schema-seen.log")));
    first.served = Some(stamp(5.0, "the server's hand-off"));
    let mut second = Update::new("symphony", 23, 2, None);
    second.failures.push(Failure {
        at: 9.0,
        run: "2026-11-19T0849-1".into(),
        why: "the server answers as release-1".into(),
    });
    second.task = Some(Task {
        project: "symphony".into(),
        ours: true,
        slug: slug("symphony", 23, 2),
        closed: false,
    });
    let other = Update::new("alpha", 22, 1, None);
    let ledger = Ledger {
        version: 0,
        updates: vec![first.clone(), second.clone(), other],
    };
    ledger.write(&machine).unwrap();
    let read = Ledger::read(&machine).unwrap();
    assert_eq!(read.version, VERSION, "written at this build's format");
    assert_eq!(read.updates, ledger.updates);
    assert_eq!(read.newest("symphony", 23), Some(&second));
    assert_eq!(read.open("symphony", 23), Some(&second));
    assert_eq!(read.open("symphony", 24), None);
    let open: Vec<&str> = read.open_of("symphony").map(|u| u.id.as_str()).collect();
    assert_eq!(open, ["symphony@23.2"]);
    assert!(read.open_of("beta").next().is_none());
}

#[test]
fn a_ledger_a_later_build_wrote_or_that_does_not_read_is_refused_never_taken_as_empty() {
    let tree = Tree::new();
    let machine = tree.path("machine");
    tree.write("machine/updates.json", "{\"version\": 99, \"updates\": []}");
    let refused = Ledger::read(&machine).unwrap_err();
    assert!(
        refused.contains("format 99 is a later build's"),
        "{refused}"
    );
    tree.write("machine/updates.json", "{\"version\": 1, \"upd");
    assert!(Ledger::read(&machine).is_err());
    // A ledger that is there and cannot be read, a folder in its place, is no empty one either.
    std::fs::remove_file(machine.join(FILE)).unwrap();
    std::fs::create_dir_all(machine.join(FILE)).unwrap();
    assert!(Ledger::read(&machine).unwrap_err().contains("updates.json"));
}

#[test]
fn a_wait_is_said_in_hours_and_minutes_or_minutes_and_seconds() {
    assert_eq!(said(0.0), "0 s");
    assert_eq!(said(-5.0), "0 s");
    assert_eq!(said(40.4), "40 s");
    assert_eq!(said(190.0), "3 min 10 s");
    assert_eq!(said(17_170.0), "4 h 46 min");
    assert_eq!(said(3600.0), "1 h 0 min");
    assert_eq!(
        said(11_810.0),
        "3 h 17 min",
        "from an hour on, to the nearest minute"
    );
    assert_eq!(said(11_789.0), "3 h 16 min");
    assert_eq!(said(3599.4), "59 min 59 s", "under an hour, to the second");
    assert_eq!(said(3599.6), "1 h 0 min");
    assert_eq!(
        said(7_170.0),
        "2 h 0 min",
        "a round up carries into the hour"
    );
}

/// A board whose task `slug` stands at `status` (none: no such task), or whose read is refused,
/// recording each call.
struct Standing {
    status: Result<Option<String>, String>,
    calls: std::cell::RefCell<Vec<String>>,
}

impl Standing {
    fn at(status: Result<Option<&str>, &str>) -> Standing {
        Standing {
            status: status
                .map(|s| s.map(str::to_string))
                .map_err(str::to_string),
            calls: std::cell::RefCell::default(),
        }
    }

    fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }
}

impl TaskBoard for Standing {
    fn register(&self) -> Result<String, String> {
        self.calls.borrow_mut().push("register".into());
        Ok("filer-1".into())
    }
    fn leave(&self, handle: &str) -> Result<(), String> {
        self.calls.borrow_mut().push(format!("leave {handle}"));
        Ok(())
    }
    fn status(&self, slug: &str) -> Result<Option<String>, String> {
        self.calls.borrow_mut().push(format!("status {slug}"));
        self.status.clone()
    }
    fn say(&self, slug: &str, by: &str, text: &str) -> Result<(), String> {
        self.calls
            .borrow_mut()
            .push(format!("say {slug} by {by}: {text}"));
        Ok(())
    }
    fn close(&self, slug: &str, by: &str, why: &str) -> Result<(), String> {
        self.calls
            .borrow_mut()
            .push(format!("close {slug} by {by}: {why}"));
        Ok(())
    }
}

#[test]
fn a_task_is_closed_once_while_it_is_an_open_red_and_its_session_always_leaves() {
    let open = Standing::at(Ok(Some("ready")));
    assert_eq!(close_task(&open, "tests/x", "served"), Ok(()));
    assert_eq!(
        open.calls(),
        [
            "register",
            "status tests/x",
            "close tests/x by filer-1: served",
            "leave filer-1"
        ]
    );
    // Closed already, or gone: left as it is, which is done.
    for standing in [Standing::at(Ok(Some("cancelled"))), Standing::at(Ok(None))] {
        assert_eq!(close_task(&standing, "tests/x", "served"), Ok(()));
        assert_eq!(
            standing.calls(),
            ["register", "status tests/x", "leave filer-1"]
        );
    }
    // A board whose read is refused: not closed, said so, and the session still leaves.
    let refused = Standing::at(Err("the board did not answer"));
    assert_eq!(
        close_task(&refused, "tests/x", "served"),
        Err("the board did not answer".to_string())
    );
    assert_eq!(
        refused.calls(),
        ["register", "status tests/x", "leave filer-1"]
    );
}

#[test]
fn a_post_is_made_by_a_session_of_its_own_which_leaves() {
    let board = Standing::at(Ok(Some("ready")));
    assert_eq!(post(&board, "tests/x", "Failure 2 of alpha@20"), Ok(()));
    assert_eq!(
        board.calls(),
        [
            "register",
            "say tests/x by filer-1: Failure 2 of alpha@20",
            "leave filer-1"
        ]
    );
}

#[test]
fn the_machines_updates_lock_is_one_holder_at_a_time_and_never_makes_the_folder() {
    let tree = Tree::new();
    let machine = tree.path("machine");
    assert!(lock(&machine).is_err() && try_lock(&machine).is_err());
    assert!(!machine.exists(), "no lock makes the machine folder");
    std::fs::create_dir_all(&machine).unwrap();
    let held = try_lock(&machine).unwrap().expect("free");
    assert!(try_lock(&machine).unwrap().is_none(), "held");
    // A second holder waits for the first: its wait ends with the first's drop.
    let (sent, taken) = crate::test_channel::channel();
    let waiting = {
        let machine = machine.clone();
        std::thread::spawn(move || {
            let lock = lock(&machine).unwrap();
            sent.send(()).unwrap();
            drop(lock);
        })
    };
    assert!(taken.try_recv().is_err(), "never taken while it is held");
    drop(held);
    taken
        .recv_timeout(test_time::backstop(std::time::Duration::from_secs(30)))
        .expect("the waiting holder took the lock once it was let go");
    waiting.join().unwrap();
    assert!(try_lock(&machine).unwrap().is_some(), "free again");
}
