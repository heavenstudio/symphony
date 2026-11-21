//! A refresh's reading of the machine's updates, on a board that keeps what it is told: what
//! completes an update, what fails it, the one task it files and how that task is told and closed,
//! its generations, and the times its records give.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use super::*;
use crate::test_tree::Tree;

const RELEASE: &str = "release-2026-11-20";
const NOW: f64 = 1_795_000_000.0;

/// A board that keeps each task's status and posts, and the sessions it registered.
#[derive(Default)]
struct Kept {
    tasks: RefCell<BTreeMap<String, (String, Vec<String>)>>,
    sessions: RefCell<u32>,
    /// While set, every write is refused, as a board that cannot be written.
    refusing: std::cell::Cell<bool>,
}

#[derive(Clone, Default)]
struct Fake(Rc<Kept>);

impl Fake {
    fn status(&self, slug: &str) -> Option<String> {
        self.0
            .tasks
            .borrow()
            .get(slug)
            .map(|(status, _)| status.clone())
    }

    fn posts(&self, slug: &str) -> Vec<String> {
        (self.0.tasks.borrow().get(slug)).map_or_else(Vec::new, |(_, posts)| posts.clone())
    }

    fn slugs(&self) -> Vec<String> {
        self.0.tasks.borrow().keys().cloned().collect()
    }

    fn refused(&self) -> Result<(), String> {
        if self.0.refusing.get() {
            Err("the board refused the write".into())
        } else {
            Ok(())
        }
    }
}

impl Reds for Fake {
    fn register(&self) -> Result<String, String> {
        *self.0.sessions.borrow_mut() += 1;
        Ok(format!("filer-{}", self.0.sessions.borrow()))
    }

    fn leave(&self, _: &str) -> Result<(), String> {
        Ok(())
    }

    fn status(&self, slug: &str) -> Result<Option<String>, String> {
        Ok(Fake::status(self, slug))
    }

    fn create(&self, slug: &str, title: &str, body: &str, _: &str) -> Result<(), String> {
        self.refused()?;
        let first = format!("{title}\n{body}");
        (self.0.tasks.borrow_mut()).insert(slug.into(), ("ready".into(), vec![first]));
        Ok(())
    }

    fn file(&self, slug: &str, title: &str, body: &str, by: &str) -> Result<(), String> {
        self.create(slug, title, body, by)
    }

    fn say(&self, slug: &str, _: &str, text: &str) -> Result<(), String> {
        self.refused()?;
        let mut tasks = self.0.tasks.borrow_mut();
        let (_, posts) = tasks.get_mut(slug).ok_or("no such task")?;
        posts.push(text.into());
        Ok(())
    }

    fn close(&self, slug: &str, _: &str, why: &str) -> Result<(), String> {
        self.refused()?;
        let mut tasks = self.0.tasks.borrow_mut();
        let (status, posts) = tasks.get_mut(slug).ok_or("no such task")?;
        *status = "cancelled".into();
        posts.push(why.into());
        Ok(())
    }
}

/// This repository's board and every project's, each a [`Fake`].
#[derive(Default)]
struct Fakes {
    ours: Fake,
    theirs: Fake,
}

impl Boards for Fakes {
    fn ours(&self, _: &Path) -> Box<dyn Reds> {
        Box::new(self.ours.clone())
    }

    fn theirs(&self, _: &Path, _: &Path) -> Box<dyn Reds> {
        Box::new(self.theirs.clone())
    }
}

/// A machine folder and a project `alpha`, its checkout a folder with no git.
struct World {
    tree: Tree,
    entry: Entry,
    registry: Registry,
    release: Manifest,
}

impl World {
    /// With a release serving `schema` named [`RELEASE`].
    fn new(schema: i64) -> World {
        let tree = Tree::new();
        let entry: Entry = serde_json::from_value(serde_json::json!({
            "id": "alpha", "name": "alpha", "root": tree.path("alpha"), "pin": 1,
        }))
        .unwrap();
        World {
            registry: Registry {
                projects: vec![entry.clone()],
            },
            entry,
            release: manifest(RELEASE, schema),
            tree,
        }
    }

    /// The same machine and boards, refreshed to a release serving `schema`.
    fn at(mut self, schema: i64) -> World {
        self.release = manifest(RELEASE, schema);
        self
    }

    fn machine(&self) -> PathBuf {
        self.tree.path("machine")
    }

    /// What one refresh does with alpha's board at `live`, as the refresh ended it (`end`), the
    /// machine's server answering as `front` and its project list saying `writes` of alpha (none:
    /// a server that says nothing of writes): the lines said, the ledger written.
    fn refresh(
        &self,
        fakes: &Fakes,
        live: Option<i64>,
        end: Option<End>,
        front: Option<Front>,
        writes: Option<bool>,
        at: f64,
    ) -> Vec<String> {
        self.refresh_as(fakes, live, end, front, writes, at, &Shape::default())
    }

    /// [`World::refresh`] by a refresh of `shape`.
    #[expect(clippy::too_many_arguments, reason = "a refresh's look, part by part")]
    fn refresh_as(
        &self,
        fakes: &Fakes,
        live: Option<i64>,
        end: Option<End>,
        front: Option<Front>,
        writes: Option<bool>,
        at: f64,
        shape: &Shape,
    ) -> Vec<String> {
        let dir = self.tree.path("");
        let machine = self.machine();
        let logs = self.tree.path("logs");
        let against = Against {
            run: "2026-11-20T1000-1",
            main: &dir,
            root: &dir,
            ours: &dir,
            machine: &machine,
            tag: RELEASE,
            commit: "0000000",
            logs: &logs,
            surreal: None,
            until: super::super::waits::far(),
        };
        let look = Look {
            against: &against,
            registry: &self.registry,
            release: &self.release,
            kept: shape.kept,
            pointed: !shape.unpointed,
            ends: &[],
            front,
            writes: if shape.unsure {
                Some(vec![("alpha".into(), None)])
            } else {
                writes.map(|writes| vec![("alpha".into(), Some(writes))])
            },
            server: None,
            now: at,
        };
        let seen = Seen {
            entry: &self.entry,
            common: self.tree.path("alpha/.git"),
            ours: shape.ours,
            live,
            end: end.as_ref(),
        };
        let mut ledger = Ledger::read(&machine).unwrap();
        // A board is read only while its project is registered.
        let mut lines = if self.registry.projects.is_empty() {
            Vec::new()
        } else {
            one(&look, fakes, &mut ledger, &seen)
        };
        lines.extend(unregistered(&look, fakes, &mut ledger));
        ledger.write(&machine).unwrap();
        lines
    }

    fn ledger(&self) -> Ledger {
        Ledger::read(&self.machine()).unwrap()
    }

    /// The release `name` installed, serving `schema`.
    fn installed(&self, name: &str, schema: i64) {
        let manifest = serde_json::to_string(&manifest(name, schema)).unwrap();
        self.tree
            .write(&format!("machine/releases/{name}/manifest.json"), &manifest);
    }
}

/// What a refresh did besides its boards: `current` as it named it, whether it pointed it, whether
/// the board is this repository's, and whether the server's list cannot say yet if it writes it.
#[derive(Default)]
struct Shape {
    kept: Option<i64>,
    unpointed: bool,
    ours: bool,
    unsure: bool,
}

fn manifest(name: &str, schema: i64) -> Manifest {
    serde_json::from_value(serde_json::json!({
        "release": name, "commit": "0123456789ab", "schema": schema,
        "protocol": {"major": 0, "minor": 3}, "tasks_api": {"major": 1, "minor": 14}, "pin": 1,
        "client": "client/0", "artifacts": [],
    }))
    .unwrap()
}

/// The machine's server as process `pid`, the release `release` at `schema`.
fn front(pid: i64, release: &str, schema: i64) -> Option<Front> {
    Some(
        serde_json::from_value(serde_json::json!({
            "pid": pid, "port": 8765, "root": "/machine", "ready": true, "app": null,
            "tailnet": null, "schema": schema, "release": release,
        }))
        .unwrap(),
    )
}

fn failures(lines: &[String]) -> Vec<&String> {
    lines
        .iter()
        .filter(|line| line.contains(" failed: "))
        .collect()
}

#[test]
fn a_first_success_files_nothing() {
    let world = World::new(19);
    let fakes = Fakes::default();
    let moved = End::Moved {
        from: 18,
        at: NOW - 30.0,
    };
    let lines = world.refresh(
        &fakes,
        Some(19),
        Some(moved),
        front(7, RELEASE, 19),
        Some(true),
        NOW,
    );
    assert!(failures(&lines).is_empty(), "{lines:?}");
    assert!(fakes.ours.slugs().is_empty() && fakes.theirs.slugs().is_empty());
    let ledger = world.ledger();
    let update = ledger.open("alpha", 19);
    assert_eq!(update, None, "served at once");
    let update = ledger.newest("alpha", 19).unwrap();
    assert_eq!(
        update.reached.as_ref().unwrap().at,
        NOW - 30.0,
        "the move's own time"
    );
    assert!(update.failures.is_empty() && update.task.is_none());
    assert_eq!(update.total(), Some(30.0));
}

#[test]
fn a_failed_update_files_one_task_each_later_failure_posts_on_it_and_success_closes_it() {
    let world = World::new(19);
    let fakes = Fakes::default();
    let slug = "tests/release-red-update-alpha-19";
    // The board at the release's schema, the server at it too, but its list says it does not write
    // alpha's board: no completion, a failure, one task on this repository's board.
    for (n, at) in [(1, NOW), (2, NOW + 60.0)] {
        let lines = world.refresh(
            &fakes,
            Some(19),
            None,
            front(7, RELEASE, 19),
            Some(false),
            at,
        );
        let failed = failures(&lines);
        assert_eq!(failed.len(), 1, "{lines:?}");
        let told = if n == 1 {
            format!("; filed {slug} on this repository's board")
        } else {
            format!("; heard on {slug}, this repository's board")
        };
        assert!(failed[0].ends_with(&told), "{failed:?}");
        assert!(
            failed[0].contains("does not write its board at schema 19"),
            "{failed:?}"
        );
        assert_eq!(fakes.ours.slugs(), [slug], "one task, whatever fails");
        let posts = fakes.ours.posts(slug);
        assert_eq!(
            posts.len(),
            1 + n,
            "its body, then a post a failure: {posts:?}"
        );
        assert!(posts[n].starts_with(&format!("Failure {n} of alpha@19 (run ")));
    }
    assert!(fakes.ours.posts(slug)[0].contains("update: alpha@19\nkey: update-alpha-19"));
    assert!(fakes.theirs.slugs().is_empty());
    // Served: closed, with the times, and nothing filed again.
    let lines = world.refresh(
        &fakes,
        Some(19),
        None,
        front(7, RELEASE, 19),
        Some(true),
        NOW + 120.0,
    );
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("updates: alpha@19 served: ")),
        "{lines:?}"
    );
    assert!(lines.contains(&format!(
        "updates: closed {slug} on this repository's board: alpha@19"
    )));
    assert_eq!(fakes.ours.status(slug).as_deref(), Some("cancelled"));
    let closing = fakes.ours.posts(slug).pop().unwrap();
    assert!(
        closing.starts_with("alpha@19: it is served (run "),
        "{closing}"
    );
    let update = world.ledger().newest("alpha", 19).unwrap().clone();
    assert_eq!(update.failures.len(), 2);
    assert!(update.task.unwrap().closed);
    // Served stays served: a refresh after it says and files nothing.
    let lines = world.refresh(
        &fakes,
        Some(19),
        None,
        front(7, RELEASE, 19),
        Some(true),
        NOW + 180.0,
    );
    assert!(lines.is_empty(), "{lines:?}");
}

#[test]
fn a_server_ahead_that_writes_the_board_completes_it_and_one_that_says_nothing_of_writes_must_be_at_its_schema()
 {
    // A server at a later schema whose list says it writes alpha's board: complete.
    let world = World::new(19);
    let fakes = Fakes::default();
    let lines = world.refresh(
        &fakes,
        Some(19),
        None,
        front(7, "release-2026-11-21", 20),
        Some(true),
        NOW,
    );
    assert!(failures(&lines).is_empty(), "{lines:?}");
    assert!(lines[0].contains("(its project list says it writes the board)"));
    let update = world.ledger().newest("alpha", 19).unwrap().clone();
    assert_eq!(update.served_by.as_deref(), Some("release-2026-11-21"));

    // A server that says nothing of writes: at the board's schema it completes it, ahead it does
    // not, as nothing says it writes it.
    let world = World::new(19);
    let lines = world.refresh(&fakes, Some(19), None, front(7, RELEASE, 19), None, NOW);
    assert!(lines[0].contains("(at the board's schema; its project list says nothing of writes)"));
    let world = World::new(19);
    let lines = world.refresh(
        &fakes,
        Some(19),
        None,
        front(7, "release-2026-11-21", 20),
        None,
        NOW,
    );
    assert_eq!(failures(&lines).len(), 1, "{lines:?}");
}

#[test]
fn an_update_waits_without_a_release_serving_it_or_a_server_and_a_failed_move_fails_whatever_serves()
 {
    let world = World::new(19);
    let fakes = Fakes::default();
    // No server answers: it waits, filing nothing.
    let lines = world.refresh(&fakes, Some(18), None, None, None, NOW);
    assert_eq!(
        lines,
        ["updates: alpha@19 waits: no server answers on this machine to write it"]
    );
    assert!(fakes.ours.slugs().is_empty());
    // Its own move failed: a failure though no server answers, its board's schema unread.
    let red = End::Red {
        why: "the board's database is not running".into(),
        down: true,
    };
    let lines = world.refresh(&fakes, None, Some(red), None, None, NOW + 60.0);
    assert_eq!(failures(&lines).len(), 1, "{lines:?}");
    assert!(
        lines[0]
            .starts_with("updates: alpha@19 failed: the board's database is not running; filed")
    );
    let body = &fakes.ours.posts("tests/release-red-update-alpha-19")[0];
    assert!(
        body.contains("now at a schema that cannot be read"),
        "{body}"
    );
    assert!(body.contains("its board's database is down"), "{body}");
    let update = world.ledger().newest("alpha", 19).unwrap().clone();
    assert_eq!(
        update.reached, None,
        "never at 19 yet: no time to give, and none guessed"
    );
    assert_eq!(update.total(), None);
}

#[test]
fn a_board_its_project_holds_files_on_its_own_board_through_the_release_that_writes_its_schema() {
    let world = World::new(19);
    let theirs = End::Theirs("its board stays at schema 18: a writer is behind".into());
    // No release installed writes schema 18: on this repository's board.
    let fakes = Fakes::default();
    world.refresh(
        &fakes,
        Some(18),
        Some(End::Theirs("held".into())),
        front(7, RELEASE, 19),
        None,
        NOW,
    );
    assert_eq!(fakes.ours.slugs(), ["tests/release-red-update-alpha-19"]);
    assert!(fakes.theirs.slugs().is_empty());

    // With one installed that does: on alpha's own board, and its next failure there too.
    let world = World::new(19);
    world.installed("release-2026-11-12", 18);
    world.tree.write(
        "machine/releases/release-2026-11-12/bin/tasks",
        "#!/bin/sh\n",
    );
    let fakes = Fakes::default();
    let lines = world.refresh(
        &fakes,
        Some(18),
        Some(theirs),
        front(7, RELEASE, 19),
        None,
        NOW,
    );
    assert!(
        lines[0].ends_with("; filed tests/release-red-update-alpha-19 on alpha's board"),
        "{lines:?}"
    );
    assert_eq!(fakes.theirs.slugs(), ["tests/release-red-update-alpha-19"]);
    assert!(fakes.ours.slugs().is_empty(), "never on both boards");
    let task = world
        .ledger()
        .newest("alpha", 19)
        .unwrap()
        .task
        .clone()
        .unwrap();
    assert!(!task.ours && task.project == "alpha");
}

#[test]
fn a_board_that_went_back_ends_its_update_and_its_next_arrival_is_a_new_generation() {
    let world = World::new(19);
    let fakes = Fakes::default();
    world.refresh(
        &fakes,
        Some(19),
        None,
        front(7, RELEASE, 19),
        Some(true),
        NOW,
    );
    // Rolled back to 18 under a release at 18: the served update is left, kept as it was.
    let back = world.at(18);
    let lines = back.refresh(
        &fakes,
        Some(18),
        None,
        front(7, RELEASE, 18),
        Some(true),
        NOW + 60.0,
    );
    assert!(
        lines.contains(
            &"updates: alpha@19 ended after it was served: its board went back: it is at schema 18"
                .to_string()
        ),
        "{lines:?}"
    );
    // At 19 again: a second generation, the first kept as it was.
    let again = back.at(19);
    let lines = again.refresh(
        &fakes,
        Some(19),
        None,
        front(7, RELEASE, 19),
        Some(false),
        NOW + 120.0,
    );
    assert!(
        lines.iter().any(|line| line
            .ends_with("; filed tests/release-red-update-alpha-19-g2 on this repository's board")),
        "{lines:?}"
    );
    let ledger = again.ledger();
    let ids: Vec<&str> = ledger.updates.iter().map(|u| u.id.as_str()).collect();
    assert_eq!(ids, ["alpha@19", "alpha@18", "alpha@19.2"]);
    assert!(ledger.updates[0].served.is_some() && ledger.updates[0].left.is_some());
    assert_eq!(ledger.open("alpha", 19).unwrap().id, "alpha@19.2");
}

#[test]
fn an_arrival_its_record_dates_after_the_served_one_is_a_new_generation_unseen_rollback_or_not() {
    let world = World::new(19);
    let fakes = Fakes::default();
    world.tree.write(
        "alpha/.git/symphony/tasks/schema-seen.log",
        &format!("19 {} seen\n", NOW - 100.0),
    );
    world.refresh(
        &fakes,
        Some(19),
        None,
        front(7, RELEASE, 19),
        Some(true),
        NOW,
    );
    let first = world.ledger().newest("alpha", 19).unwrap().clone();
    assert_eq!(first.reached.as_ref().unwrap().at, NOW - 100.0);
    assert!(first.reached.unwrap().source.starts_with("schema-seen.log"));
    // The same record again: the same update, nothing new.
    world.refresh(
        &fakes,
        Some(19),
        None,
        front(7, RELEASE, 19),
        Some(true),
        NOW + 60.0,
    );
    assert_eq!(world.ledger().updates.len(), 1);
    // The record started again from an accepted rollback and saw 19 later: a new arrival.
    world.tree.write(
        "alpha/.git/symphony/tasks/schema-seen.log",
        &format!(
            "19 {} seen\n18 {} accepted\n19 {} seen\n",
            NOW - 100.0,
            NOW,
            NOW + 90.0
        ),
    );
    world.refresh(
        &fakes,
        Some(19),
        None,
        front(7, RELEASE, 19),
        Some(true),
        NOW + 120.0,
    );
    let ids: Vec<String> = (world.ledger().updates.iter())
        .map(|u| u.id.clone())
        .collect();
    assert_eq!(ids, ["alpha@19", "alpha@19.2"]);
}

#[test]
fn an_open_update_a_higher_goal_supersedes_is_served_where_written_or_ended_with_its_task() {
    // alpha@18 failed, its task filed; then a release at 19: alpha@18, written at 18 by the
    // server there, is served and its task closed.
    let world = World::new(18);
    let fakes = Fakes::default();
    world.refresh(
        &fakes,
        Some(18),
        None,
        front(7, RELEASE, 18),
        Some(false),
        NOW,
    );
    assert_eq!(fakes.ours.slugs(), ["tests/release-red-update-alpha-18"]);
    let next = world.at(19);
    let lines = next.refresh(
        &fakes,
        Some(18),
        None,
        front(7, RELEASE, 18),
        Some(true),
        NOW + 60.0,
    );
    assert!(
        lines[0].starts_with("updates: alpha@18 served: "),
        "{lines:?}"
    );
    assert_eq!(
        fakes
            .ours
            .status("tests/release-red-update-alpha-18")
            .as_deref(),
        Some("cancelled")
    );

    // Not written there: superseded, its task closed, alpha@19 its own.
    let world = World::new(18);
    let fakes = Fakes::default();
    world.refresh(
        &fakes,
        Some(18),
        None,
        front(7, RELEASE, 18),
        Some(false),
        NOW,
    );
    let next = world.at(19);
    let lines = next.refresh(&fakes, Some(18), None, None, None, NOW + 60.0);
    assert_eq!(
        lines[0],
        "updates: alpha@18 ended unserved: superseded by alpha@19: its board is to be written at \
         schema 19"
    );
    assert_eq!(
        fakes
            .ours
            .status("tests/release-red-update-alpha-18")
            .as_deref(),
        Some("cancelled")
    );
    let ids: Vec<String> = (next.ledger().updates.iter())
        .map(|u| u.id.clone())
        .collect();
    assert_eq!(ids, ["alpha@18", "alpha@19"]);
}

#[test]
fn an_update_of_a_project_no_longer_registered_ends_with_its_task() {
    let mut world = World::new(19);
    let fakes = Fakes::default();
    world.refresh(
        &fakes,
        Some(19),
        None,
        front(7, RELEASE, 19),
        Some(false),
        NOW,
    );
    world.registry.projects.clear();
    let lines = world.refresh(
        &fakes,
        Some(19),
        None,
        front(7, RELEASE, 19),
        Some(true),
        NOW + 60.0,
    );
    assert!(
        lines.contains(
            &"updates: alpha@19 ended unserved: its project is no longer registered on this machine"
                .to_string()
        ),
        "{lines:?}"
    );
    assert_eq!(
        fakes
            .ours
            .status("tests/release-red-update-alpha-19")
            .as_deref(),
        Some("cancelled")
    );
}

#[test]
fn the_server_began_writing_at_its_hand_off_line_when_after_the_board_reached_it_else_at_the_look()
{
    let world = World::new(19);
    world.tree.write(
        "machine/server.log",
        "[2026-11-19T10:00:00.000Z] · Symphony reload: replacement pid 7 ready; draining pid 6\n\
         [2026-11-19T11:51:19.778Z] · Symphony reload: replacement pid 8 ready; draining pid 7\n\
         [2026-11-19T12:00:00.000Z] · something else\n",
    );
    let machine = world.machine();
    assert_eq!(hand_off(&machine, 8), Some(1_795_089_079.778));
    assert_eq!(hand_off(&machine, 7), utc("2026-11-19T10:00:00.000Z"));
    assert_eq!(hand_off(&machine, 9), None);
    assert_eq!(hand_off(&world.tree.path("nowhere"), 8), None);

    let fakes = Fakes::default();
    world.tree.write(
        "alpha/.git/symphony/tasks/schema-seen.log",
        "19 1795080000 seen\n",
    );
    world.refresh(
        &fakes,
        Some(19),
        None,
        front(8, RELEASE, 19),
        Some(true),
        NOW,
    );
    let update = world.ledger().newest("alpha", 19).unwrap().clone();
    let served = update.served.unwrap();
    assert_eq!(served.at, 1_795_089_079.778);
    assert!(served.source.starts_with("the server's hand-off"));

    // A hand-off before the board reached the schema is not when it began writing it.
    let world = World::new(19);
    world.tree.write(
        "machine/server.log",
        "[2026-11-19T10:00:00.000Z] · Symphony reload: replacement pid 7 ready; draining pid 6\n",
    );
    world.tree.write(
        "alpha/.git/symphony/tasks/schema-seen.log",
        "19 1795089000 seen\n",
    );
    world.refresh(
        &fakes,
        Some(19),
        None,
        front(7, RELEASE, 19),
        Some(true),
        NOW,
    );
    let served = world
        .ledger()
        .newest("alpha", 19)
        .unwrap()
        .served
        .clone()
        .unwrap();
    assert_eq!(served.at, NOW);
    assert!(
        served
            .source
            .starts_with("this refresh found the server writing it")
    );
}

#[test]
fn a_time_the_servers_log_writes_reads_as_seconds_since_the_epoch_and_nothing_else_does() {
    assert_eq!(utc("1970-01-01T00:00:00Z"), Some(0.0));
    assert_eq!(utc("2026-11-19T11:51:19.778Z"), Some(1_795_089_079.778));
    assert_eq!(
        utc("2000-02-29T12:00:00Z"),
        Some(951_825_600.0),
        "a leap day"
    );
    for text in [
        "2026-11-19T11:51:19",
        "2026-11-19 11:51:19Z",
        "2026-13-01T00:00:00Z",
        "2026-11-19T24:00:00Z",
        "garbage",
        "",
    ] {
        assert_eq!(utc(text), None, "{text:?}");
    }
}

#[test]
fn a_latency_is_said_with_what_came_after_a_release_serving_it_and_nothing_unknown_is_guessed() {
    let mut update = Update::new("alpha", 19, 1, None);
    assert_eq!(latency(&update), "");
    update.served = Some(Stamp {
        at: 5000.0,
        source: "the server's hand-off".into(),
    });
    assert_eq!(
        latency(&update),
        "",
        "no time it reached the schema: no total"
    );
    update.reached = Some(Stamp {
        at: 1000.0,
        source: "schema-seen.log".into(),
    });
    assert_eq!(latency(&update), "It took 1 h 7 min. ");
    update.available = Some(Stamp {
        at: 4000.0,
        source: "the manifest".into(),
    });
    assert_eq!(
        latency(&update),
        "It took 1 h 7 min from the board reaching schema 19 to the server writing it, 16 min 40 s \
         of it after a release serving it was installed. "
    );
}

#[test]
fn the_status_names_each_open_update_with_its_task_and_the_newest_served_with_their_times() {
    let world = World::new(19);
    assert_eq!(
        status(&world.machine(), NOW),
        "schema updates: none recorded\n"
    );
    world
        .tree
        .write("machine/updates.json", "{\"version\": 99}");
    assert!(
        status(&world.machine(), NOW).starts_with("schema updates: the ledger cannot be read: ")
    );

    let stamp = |at: f64, source: &str| Stamp {
        at,
        source: source.into(),
    };
    let mut served = Update::new(
        "symphony",
        23,
        1,
        Some(stamp(
            1000.0,
            "schema-seen.log: the first store that saw it",
        )),
    );
    served.release = Some("release-2026-11-23.3".into());
    served.available = Some(stamp(
        6000.0,
        "release-2026-11-23.3's manifest, written by its install",
    ));
    served.served = Some(stamp(18_000.0, "the server's hand-off: its reload's line"));
    let mut unknown = Update::new("beta", 23, 1, None);
    unknown.served = Some(stamp(
        500.0,
        "this refresh found the server writing it: then or before",
    ));
    let mut open = Update::new("alpha", 23, 2, None);
    for (at, run) in [(NOW - 7200.0, "r1"), (NOW - 60.0, "r2")] {
        open.failures.push(Failure {
            at,
            run: run.into(),
            why: "held".into(),
        });
    }
    open.task = Some(Task {
        project: "alpha".into(),
        ours: false,
        slug: "tests/release-red-update-alpha-23-g2".into(),
        closed: false,
    });
    let waiting = Update::new(
        "gamma",
        24,
        1,
        Some(stamp(NOW - 30.0, "first found at it: then or earlier")),
    );
    Ledger {
        version: 1,
        updates: vec![served, unknown, open, waiting],
    }
    .write(&world.machine())
    .unwrap();
    let said = status(&world.machine(), NOW);
    let lines: Vec<&str> = said.lines().collect();
    assert_eq!(
        lines[0],
        format!(
            "schema updates: 2 open, 2 served (ledger {})",
            world.machine().join("updates.json").display()
        )
    );
    assert!(
        lines[1].starts_with("  open alpha@23.2: failed 2 times since ")
            && lines[1].ends_with(
                "(2 h ago); task tests/release-red-update-alpha-23-g2 on alpha's board; not at its \
                 schema yet"
            ),
        "{said}"
    );
    assert!(
        lines[2].starts_with("  open gamma@24: no failure yet; reached ")
            && lines[2].ends_with(" (first found at it)"),
        "{said}"
    );
    assert!(
        lines[3].starts_with("  served beta@23 ")
            && lines[3].ends_with(
                ": how long is not known; times from served by this refresh found the server \
                 writing it"
            ),
        "the newest first: {said}"
    );
    assert!(
        lines[4].starts_with("  served symphony@23 ")
            && lines[4].ends_with(
                ": 4 h 43 min from its board reaching it, 3 h 20 min of it after a release \
                 serving it; times from reached by schema-seen.log, served by the server's \
                 hand-off, release-2026-11-23.3 by release-2026-11-23.3's manifest, written by its \
                 install"
            ),
        "{said}"
    );
    assert_eq!(lines.len(), 5);
}

#[test]
fn this_repositorys_board_ahead_of_every_release_waits_for_one_and_one_written_as_before_is_none() {
    let world = World::new(19);
    let fakes = Fakes::default();
    let ours = Shape {
        kept: Some(19),
        unpointed: true,
        ours: true,
        ..Shape::default()
    };
    let lines = world.refresh_as(
        &fakes,
        Some(20),
        None,
        front(7, RELEASE, 19),
        None,
        NOW,
        &ours,
    );
    assert_eq!(
        lines,
        ["updates: alpha@20 waits: no release serving schema 20 is installed yet"]
    );
    // A board the server wrote before this refresh, which neither moved it nor pointed current:
    // no update, nothing to measure.
    let world = World::new(19);
    let lines = world.refresh_as(
        &fakes,
        Some(19),
        None,
        front(7, RELEASE, 19),
        Some(true),
        NOW,
        &ours,
    );
    assert!(lines.is_empty(), "{lines:?}");
    assert!(world.ledger().updates.is_empty());
}

#[test]
fn an_open_update_its_board_moved_past_ends_and_a_current_not_pointed_leaves_the_server_behind() {
    let world = World::new(19);
    let fakes = Fakes::default();
    // `current` could not be pointed (the install's red): the server stays behind the board.
    let unpointed = Shape {
        unpointed: true,
        ..Shape::default()
    };
    let lines = world.refresh_as(
        &fakes,
        Some(19),
        None,
        front(7, RELEASE, 18),
        None,
        NOW,
        &unpointed,
    );
    assert!(
        lines[0].starts_with(&format!(
            "updates: alpha@19 failed: the machine's server, the release {RELEASE} at schema 18, \
             does not write its board at schema 19; filed "
        )),
        "{lines:?}"
    );
    // Behind its goal, the line says where it stays.
    let behind = World::new(19);
    let lines = behind.refresh_as(
        &fakes,
        Some(18),
        None,
        front(7, RELEASE, 18),
        None,
        NOW,
        &unpointed,
    );
    assert!(
        lines[0]
            .starts_with("updates: alpha@19 failed: its board is at schema 18, not moved to 19; "),
        "{lines:?}"
    );
    // The board moved on past it: ended, its task closed.
    let past = world.at(20);
    let lines = past.refresh(&fakes, Some(20), None, None, None, NOW + 60.0);
    assert_eq!(
        lines[0],
        "updates: alpha@19 ended unserved: its board moved on to schema 20"
    );
    assert_eq!(
        fakes
            .ours
            .status("tests/release-red-update-alpha-19")
            .as_deref(),
        Some("cancelled")
    );
}

#[test]
fn a_projects_build_as_the_server_is_named_by_its_checkout() {
    let world = World::new(19);
    let fakes = Fakes::default();
    let build: Option<Front> = Some(
        serde_json::from_value(serde_json::json!({
            "pid": 7, "port": 8765, "root": "/work/alpha", "ready": true, "app": null,
            "tailnet": null, "schema": 18,
        }))
        .unwrap(),
    );
    let lines = world.refresh(&fakes, Some(19), None, build, None, NOW);
    assert!(
        lines[0].starts_with(
            "updates: alpha@19 failed: the machine's server, the build of /work/alpha at schema \
             18, does not write its board at schema 19; filed "
        ),
        "{lines:?}"
    );
}

#[test]
fn a_task_whose_board_cannot_be_reached_or_written_is_said_and_tried_again_by_the_next_refresh() {
    // Filed on alpha's own board, which no release here writes once that release is gone.
    let world = World::new(19);
    world.installed("release-2026-11-12", 18);
    world.tree.write(
        "machine/releases/release-2026-11-12/bin/tasks",
        "#!/bin/sh\n",
    );
    let fakes = Fakes::default();
    let held = End::Theirs("held".into());
    world.refresh(
        &fakes,
        Some(18),
        Some(held),
        front(7, RELEASE, 19),
        None,
        NOW,
    );
    assert_eq!(fakes.theirs.slugs(), ["tests/release-red-update-alpha-19"]);
    // alpha's checkout is no git checkout here, so its board cannot be reached again: the failure
    // is recorded, its post not written, and said.
    let held = End::Theirs("held".into());
    let lines = world.refresh(
        &fakes,
        Some(18),
        Some(held),
        front(7, RELEASE, 19),
        None,
        NOW + 60.0,
    );
    assert!(
        lines[0].ends_with(
            "; its task could not be written: alpha's board, which holds \
             tests/release-red-update-alpha-19, cannot be reached now"
        ),
        "{lines:?}"
    );
    assert_eq!(
        world.ledger().newest("alpha", 19).unwrap().failures.len(),
        2
    );
    // Served: the close cannot reach it either, and the next refresh tries.
    let lines = world.refresh(
        &fakes,
        Some(19),
        None,
        front(7, RELEASE, 19),
        Some(true),
        NOW + 120.0,
    );
    assert!(
        lines.contains(
            &"updates: alpha@19 (it is served): alpha's board cannot be reached to close \
              tests/release-red-update-alpha-19; the next refresh does"
                .to_string()
        ),
        "{lines:?}"
    );
    assert!(!world_task_closed(&world));

    // On this repository's board, which refuses writes: said, never lost.
    let world = World::new(19);
    let fakes = Fakes::default();
    fakes.ours.0.refusing.set(true);
    let lines = world.refresh(
        &fakes,
        Some(19),
        None,
        front(7, RELEASE, 19),
        Some(false),
        NOW,
    );
    assert!(
        lines[0].ends_with("; its task could not be written: the board refused the write"),
        "{lines:?}"
    );
    assert_eq!(world.ledger().newest("alpha", 19).unwrap().task, None);
    fakes.ours.0.refusing.set(false);
    world.refresh(
        &fakes,
        Some(19),
        None,
        front(7, RELEASE, 19),
        Some(false),
        NOW + 60.0,
    );
    fakes.ours.0.refusing.set(true);
    let lines = world.refresh(
        &fakes,
        Some(19),
        None,
        front(7, RELEASE, 19),
        Some(true),
        NOW + 120.0,
    );
    assert!(
        lines.contains(
            &"updates: tests/release-red-update-alpha-19 could not be closed on this repository's \
              board (the board refused the write); the next refresh tries again"
                .to_string()
        ),
        "{lines:?}"
    );
    assert!(!world_task_closed(&world));
}

fn world_task_closed(world: &World) -> bool {
    (world.ledger().updates.iter()).any(|update| update.task.as_ref().is_some_and(|t| t.closed))
}

#[test]
fn a_task_closed_by_hand_is_taken_as_closed_once_its_update_is_served() {
    let world = World::new(19);
    let fakes = Fakes::default();
    let slug = "tests/release-red-update-alpha-19";
    world.refresh(
        &fakes,
        Some(19),
        None,
        front(7, RELEASE, 19),
        Some(false),
        NOW,
    );
    fakes.ours.0.tasks.borrow_mut().get_mut(slug).unwrap().0 = "done".into();
    let lines = world.refresh(
        &fakes,
        Some(19),
        None,
        front(7, RELEASE, 19),
        Some(true),
        NOW + 60.0,
    );
    assert!(lines.contains(&format!(
        "updates: closed {slug} on this repository's board: alpha@19"
    )));
    assert_eq!(
        fakes.ours.status(slug).as_deref(),
        Some("done"),
        "left as the hand closed it"
    );
    assert!(world_task_closed(&world));
}

#[test]
fn the_status_says_a_single_failure_and_a_total_without_a_release_time() {
    let world = World::new(19);
    let mut open = Update::new("alpha", 19, 1, None);
    open.failures.push(Failure {
        at: NOW - 600.0,
        run: "r".into(),
        why: "w".into(),
    });
    let mut served = Update::new(
        "beta",
        19,
        1,
        Some(Stamp {
            at: 100.0,
            source: "schema-seen.log".into(),
        }),
    );
    served.served = Some(Stamp {
        at: 190.0,
        source: "the server's hand-off".into(),
    });
    Ledger {
        version: 1,
        updates: vec![open, served],
    }
    .write(&world.machine())
    .unwrap();
    let said = status(&world.machine(), NOW);
    assert!(
        said.contains("\n  open alpha@19: failed once, 10 min ago; not at its schema yet\n"),
        "{said}"
    );
    assert!(
        said.contains(": 1 min 30 s from its board reaching it; times from reached by schema-seen.log, served by the server's hand-off\n"),
        "{said}"
    );
}

#[test]
fn a_server_that_cannot_say_yet_whether_it_writes_the_board_neither_completes_nor_fails_it() {
    let world = World::new(19);
    let fakes = Fakes::default();
    let unsure = Shape {
        unsure: true,
        ..Shape::default()
    };
    // At the board's schema, as a server that says nothing of writes is taken to write it: still
    // not complete while its list says it cannot say.
    let lines = world.refresh_as(
        &fakes,
        Some(19),
        None,
        front(7, RELEASE, 19),
        None,
        NOW,
        &unsure,
    );
    assert_eq!(
        lines,
        [
            "updates: alpha@19 waits: the machine's server cannot say yet whether it writes its board"
        ]
    );
    assert!(fakes.ours.slugs().is_empty());
    // A failed move fails whatever the list says.
    let held = End::Theirs("held".into());
    let lines = world.refresh_as(
        &fakes,
        Some(18),
        Some(held),
        front(7, RELEASE, 19),
        None,
        NOW,
        &unsure,
    );
    assert!(
        lines[0].starts_with("updates: alpha@19 failed: held; filed "),
        "{lines:?}"
    );
}

#[test]
fn a_server_is_unsure_while_it_cannot_say_whether_it_writes_a_registered_board_that_is_there() {
    let tree = Tree::new();
    tree.write("alpha/.git/symphony/tasks/schema-seen.log", "");
    std::fs::create_dir_all(tree.path("bare/.git")).unwrap();
    let entry = |id: &str| -> Entry {
        serde_json::from_value(serde_json::json!({
            "id": id, "name": id, "root": tree.path(id), "pin": 1,
        }))
        .unwrap()
    };
    let registry = Registry {
        projects: vec![entry("alpha"), entry("bare"), entry("gone")],
    };
    let boards = with_boards(&registry);
    assert_eq!(
        boards,
        ["alpha"],
        "a project with no board, or no checkout, is not waited for"
    );
    let said = |alpha: Option<bool>| vec![("alpha".to_string(), alpha), ("bare".to_string(), None)];
    assert!(unsure(&boards, Some(&said(None))));
    assert!(!unsure(&boards, Some(&said(Some(true)))));
    assert!(!unsure(&boards, Some(&said(Some(false)))), "a no is a word");
    assert!(
        !unsure(&boards, Some(&[])),
        "a server that says nothing of writes is not waited for"
    );
    assert!(!unsure(&boards, None), "nor is none");
}
