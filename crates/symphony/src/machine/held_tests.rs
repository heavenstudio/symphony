//! The retry of a held update, as far as no running board is needed: which update a round has to
//! do with, what holds every move (the machine's updates lock, a release run of an older build,
//! `current` naming another release), the Symphony's own board, and a board whose database is down,
//! failed once and posted once however many rounds find it so. Its moves, on scratch boards, are
//! `crates/symphony-build/tests/release/held.rs`.

use std::cell::RefCell;
use std::fs;
use std::rc::Rc;

use super::*;
use crate::machine::Entry;
use crate::machine::updates::{Failure, Task};
use crate::test_tree::Tree;

/// The release installed in each machine folder here, serving [`SCHEMA`].
const RELEASE: &str = "release-2026-11-20";
/// The board schema [`RELEASE`] serves.
const SCHEMA: i64 = 20;
/// The time the fake host says it is.
const NOW: f64 = 1_800_000_000.0;

fn git(at: &Path, args: &[&str]) {
    let done = std::process::Command::new("git")
        .args(["-c", "init.defaultBranch=main"])
        .args(args)
        .current_dir(at)
        .output()
        .unwrap();
    assert!(
        done.status.success(),
        "{}",
        String::from_utf8_lossy(&done.stderr)
    );
}

/// A git checkout at `root`, made.
fn checkout(root: &Path) -> PathBuf {
    fs::create_dir_all(root).unwrap();
    git(root, &["init", "-q"]);
    root.canonicalize().unwrap()
}

/// A machine folder in `tree` with [`RELEASE`] installed and named by `current`, `alpha` registered
/// at its own checkout and the Symphony at `symphony`, keeping the release run's record, and the
/// ledger holding `alpha@SCHEMA` open, failed once, its task on the Symphony's board: the machine
/// folder and alpha's checkout.
fn machine(tree: &Tree) -> (PathBuf, PathBuf) {
    let machine = tree.path("machine");
    let folder = release::folder(&machine, RELEASE);
    fs::create_dir_all(folder.join("bin")).unwrap();
    fs::write(
        folder.join(release::MANIFEST),
        serde_json::json!({
            "release": RELEASE, "commit": "0123456", "schema": SCHEMA,
            "protocol": {"major": 0, "minor": 3}, "tasks_api": {"major": 2, "minor": 2}, "pin": 1,
            "client": "client/x", "artifacts": [],
        })
        .to_string(),
    )
    .unwrap();
    std::os::unix::fs::symlink(
        Path::new(release::RELEASES).join(RELEASE),
        machine.join("current"),
    )
    .unwrap();
    let alpha = checkout(&tree.path("alpha"));
    let symphony = checkout(&tree.path("symphony"));
    fs::create_dir_all(symphony.join(".git").join(symphony_board::folders::RELEASE)).unwrap();
    let entry = |id: &str, root: &Path| -> Entry {
        serde_json::from_value(serde_json::json!({"id": id, "name": id, "root": root, "pin": 1}))
            .unwrap()
    };
    Registry {
        projects: vec![entry("alpha", &alpha), entry("symphony", &symphony)],
    }
    .write(&machine)
    .unwrap();
    let mut update = Update::new("alpha", SCHEMA, 1, None);
    update.failures.push(Failure {
        at: NOW - 3600.0,
        run: "2026-11-20T0004-1".into(),
        why: "its board stays at schema 19: a writer seen there in the last week is behind".into(),
    });
    update.task = Some(Task {
        project: "symphony".into(),
        ours: true,
        slug: updates::slug("alpha", SCHEMA, 1),
        closed: false,
    });
    Ledger {
        version: updates::VERSION,
        updates: vec![update],
    }
    .write(&machine)
    .unwrap();
    (machine, alpha)
}

/// A board that records what it was told, and refuses nothing.
#[derive(Clone, Default)]
struct Told(Rc<RefCell<Vec<String>>>);

impl TaskBoard for Told {
    fn register(&self) -> Result<String, String> {
        Ok("filer-1".into())
    }
    fn leave(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn status(&self, _: &str) -> Result<Option<String>, String> {
        Ok(Some("ready".into()))
    }
    fn say(&self, slug: &str, _: &str, text: &str) -> Result<(), String> {
        self.0.borrow_mut().push(format!("say {slug}: {text}"));
        Ok(())
    }
    fn close(&self, slug: &str, _: &str, why: &str) -> Result<(), String> {
        self.0.borrow_mut().push(format!("close {slug}: {why}"));
        Ok(())
    }
}

/// A host whose clock is [`NOW`], which runs nothing, writes no board, and whose boards are one
/// [`Told`].
#[derive(Default)]
struct Fake {
    told: Told,
    ran: RefCell<usize>,
}

impl Host for Fake {
    fn now(&self) -> f64 {
        NOW
    }
    fn run(&self, _: Command, _: Duration) -> Result<Output, String> {
        *self.ran.borrow_mut() += 1;
        Err("nothing runs here".into())
    }
    fn writes(&self, _: i64) -> Result<(), String> {
        Err("no server here".into())
    }
    fn ours(&self, _: &Path) -> Box<dyn TaskBoard> {
        Box::new(self.told.clone())
    }
    fn theirs(&self, _: &Path, _: &Path) -> Box<dyn TaskBoard> {
        Box::new(self.told.clone())
    }
}

fn round(machine: &Path, host: &Fake) -> Vec<String> {
    retry(
        &Retry {
            machine,
            release: RELEASE,
            project: "alpha",
            bound: Duration::from_secs(240),
        },
        host,
    )
}

#[test]
fn a_round_has_to_do_with_the_newest_update_to_the_schema_while_open_or_its_task_is() {
    let mut ledger = Ledger::default();
    assert_eq!(due(&ledger, "alpha", 20), None);
    ledger.updates.push(Update::new("alpha", 19, 1, None));
    ledger.updates.push(Update::new("beta", 20, 1, None));
    assert_eq!(
        due(&ledger, "alpha", 20),
        None,
        "another schema, another board"
    );
    ledger.updates.push(Update::new("alpha", 20, 1, None));
    assert_eq!(due(&ledger, "alpha", 20), Some(2));
    let stamp = Stamp {
        at: NOW,
        source: "x".into(),
    };
    // Its first generation ended when the board went back; the second is the one.
    ledger.updates[2].left = Some(stamp.clone());
    assert_eq!(due(&ledger, "alpha", 20), None, "an update that ended");
    ledger.updates.push(Update::new("alpha", 20, 2, None));
    assert_eq!(due(&ledger, "alpha", 20), Some(3));
    // Served: nothing to do, unless its task is still open.
    ledger.updates[3].served = Some(stamp);
    assert_eq!(due(&ledger, "alpha", 20), None);
    ledger.updates[3].task = Some(Task {
        project: "alpha".into(),
        ours: false,
        slug: "tests/release-red-update-alpha-20-g2".into(),
        closed: false,
    });
    assert_eq!(due(&ledger, "alpha", 20), Some(3), "a close to finish");
    ledger.updates[3].task.as_mut().unwrap().closed = true;
    assert_eq!(due(&ledger, "alpha", 20), None);
}

#[test]
fn a_round_with_nothing_due_reads_two_files_and_takes_no_lock() {
    let tree = Tree::new();
    let (machine, _) = machine(&tree);
    let host = Fake::default();
    // Another project's update is not this one's.
    let other = retry(
        &Retry {
            machine: &machine,
            release: RELEASE,
            project: "beta",
            bound: Duration::from_secs(240),
        },
        &host,
    );
    assert_eq!(other, Vec::<String>::new());
    // A server of a release that is not installed here tries nothing.
    let unknown = retry(
        &Retry {
            machine: &machine,
            release: "release-2026-11-21",
            project: "alpha",
            bound: Duration::from_secs(240),
        },
        &host,
    );
    assert_eq!(unknown, Vec::<String>::new());
    assert!(!machine.join(updates::LOCK).exists(), "no lock was taken");
}

#[test]
fn nothing_moves_while_the_machines_updates_lock_is_held_as_a_release_run_holds_it() {
    let tree = Tree::new();
    let (machine, alpha) = machine(&tree);
    fs::create_dir_all(alpha.join(".git").join(symphony_board::server::FOLDER)).unwrap();
    let before = fs::read_to_string(updates::path(&machine)).unwrap();
    let host = Fake::default();
    let run = updates::lock(&machine).unwrap();
    assert_eq!(
        round(&machine, &host),
        [format!(
            "alpha@{SCHEMA}: the machine's updates lock is held (a release run, a refresh or \
             another project's retry): nothing moves until a round finds it free"
        )]
    );
    assert_eq!(fs::read_to_string(updates::path(&machine)).unwrap(), before);
    assert!(host.told.0.borrow().is_empty() && *host.ran.borrow() == 0);
    // Once it is let go, a round takes it, and the next run waits for that round's end.
    drop(run);
    assert!(round(&machine, &host)[0].contains("not moved"));
    assert!(
        updates::try_lock(&machine).unwrap().is_some(),
        "let go at the round's end"
    );
}

#[test]
fn nothing_moves_while_a_release_run_of_a_build_before_the_lock_says_it_runs() {
    let tree = Tree::new();
    let (machine, alpha) = machine(&tree);
    fs::create_dir_all(alpha.join(".git").join(symphony_board::server::FOLDER)).unwrap();
    let record = tree
        .path("symphony/.git")
        .join(symphony_board::folders::RELEASE);
    let before = fs::read_to_string(updates::path(&machine)).unwrap();
    let host = Fake::default();
    // A living process says it runs, holding the run's lock and not the machine's.
    fs::write(
        record.join(RUNNING),
        serde_json::json!({"id": "2026-11-20T1000-7", "pid": std::process::id(), "started_at": NOW, "until": NOW + 3600.0}).to_string(),
    )
    .unwrap();
    let said = round(&machine, &host);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].starts_with(&format!(
            "alpha@{SCHEMA}: a release run holds its lock (run 2026-11-20T1000-7, process {}, by ",
            std::process::id()
        )),
        "{said:?}"
    );
    assert_eq!(fs::read_to_string(updates::path(&machine)).unwrap(), before);
    assert!(running(&Registry::read(&machine).unwrap()).is_some());
    // A run killed leaves what it said: a process that is gone holds nothing.
    let mut gone = std::process::Command::new("true").spawn().unwrap();
    let pid = gone.id();
    gone.wait().unwrap();
    fs::write(
        record.join(RUNNING),
        serde_json::json!({"id": "2026-11-20T1000-8", "pid": pid}).to_string(),
    )
    .unwrap();
    assert_eq!(running(&Registry::read(&machine).unwrap()), None);
    fs::write(record.join(RUNNING), "{ not json").unwrap();
    assert_eq!(running(&Registry::read(&machine).unwrap()), None);
}

#[test]
fn nothing_moves_unless_current_still_names_this_servers_release() {
    let tree = Tree::new();
    let (machine, alpha) = machine(&tree);
    fs::create_dir_all(alpha.join(".git").join(symphony_board::server::FOLDER)).unwrap();
    let host = Fake::default();
    fs::remove_file(machine.join("current")).unwrap();
    std::os::unix::fs::symlink(
        Path::new(release::RELEASES).join("release-2026-11-21"),
        machine.join("current"),
    )
    .unwrap();
    assert_eq!(
        round(&machine, &host),
        [format!(
            "alpha@{SCHEMA}: current names release-2026-11-21, not this server's release {RELEASE}: \
             nothing moves"
        )]
    );
    fs::remove_file(machine.join("current")).unwrap();
    assert!(round(&machine, &host)[0].contains("current names no release"));
    assert!(host.told.0.borrow().is_empty());
}

#[test]
fn the_symphonys_own_board_moves_with_its_own_main_and_never_here() {
    let tree = Tree::new();
    let (machine, _) = machine(&tree);
    let mut ledger = Ledger::read(&machine).unwrap();
    ledger
        .updates
        .push(Update::new("symphony", SCHEMA, 1, None));
    ledger.write(&machine).unwrap();
    let host = Fake::default();
    let said = retry(
        &Retry {
            machine: &machine,
            release: RELEASE,
            project: "symphony",
            bound: Duration::from_secs(240),
        },
        &host,
    );
    assert_eq!(said, Vec::<String>::new());
    assert!(!machine.join(updates::LOCK).exists());
}

#[test]
fn a_project_that_keeps_no_board_has_none_to_move() {
    let tree = Tree::new();
    let (machine, _) = machine(&tree);
    let before = fs::read_to_string(updates::path(&machine)).unwrap();
    assert_eq!(round(&machine, &Fake::default()), Vec::<String>::new());
    assert_eq!(fs::read_to_string(updates::path(&machine)).unwrap(), before);
}

#[test]
fn a_board_whose_database_is_down_fails_once_and_is_heard_once_however_many_rounds_find_it() {
    let tree = Tree::new();
    let (machine, alpha) = machine(&tree);
    fs::create_dir_all(alpha.join(".git").join(symphony_board::server::FOLDER)).unwrap();
    let host = Fake::default();
    let first = round(&machine, &host);
    assert_eq!(first.len(), 1, "{first:?}");
    assert!(
        first[0].starts_with(&format!("alpha@{SCHEMA}: not moved: "))
            && first[0].contains("the machine's server moves it at its next round")
            && first[0].ends_with(&format!("; heard on {}", updates::slug("alpha", SCHEMA, 1))),
        "{first:?}"
    );
    let ledger = Ledger::read(&machine).unwrap();
    let update = &ledger.updates[0];
    assert_eq!(update.failures.len(), 2);
    assert_eq!(update.failures[1].at, NOW);
    assert_eq!(
        update.failures[1].run,
        format!("the machine's server, {RELEASE}")
    );
    assert!(update.open() && update.reached.is_none());
    let told = host.told.0.borrow().clone();
    assert_eq!(told.len(), 1);
    assert!(
        told[0].starts_with(&format!(
            "say {}: Failure 2 of alpha@{SCHEMA} (the machine's server, {RELEASE}, at its round): ",
            updates::slug("alpha", SCHEMA, 1)
        )),
        "{told:?}"
    );
    // The same again: no failure, no post, the ledger as it was.
    let written = fs::read_to_string(updates::path(&machine)).unwrap();
    for _ in 0..3 {
        assert_eq!(
            round(&machine, &host),
            [format!(
                "alpha@{SCHEMA}: still held, as its last failure says"
            )]
        );
    }
    assert_eq!(
        fs::read_to_string(updates::path(&machine)).unwrap(),
        written
    );
    assert_eq!(host.told.0.borrow().len(), 1);
    assert_eq!(
        *host.ran.borrow(),
        0,
        "nothing was run for a board that cannot be read"
    );
}
