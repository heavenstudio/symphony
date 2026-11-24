//! The recovery generation and the writes bound to it (`store/recovery.rs`):
//! a token read before a copy of the whole board, or before a rebind, is refused after it, with
//! nothing written, though the copy gave the same number and version to that task or another.

use crate::common::{Fixture, HOST, NOW, Seed, database, refused};
use crate::copy::everything;
use std::sync::{Arc, Barrier, Mutex};
use symphony_board::recovery::{Authority, Bound, BoundTask, COMPAT};
use symphony_board::{Error, NewSession, Opening, Schema, Store};

/// An authority a test holds: what it binds the board to, the call it fails, and the calls made.
#[derive(Default)]
pub(crate) struct Memory {
    bound: Mutex<Option<String>>,
    fails: Mutex<&'static str>,
    calls: Mutex<Vec<String>>,
}

impl Memory {
    fn failing(&self, call: &'static str) {
        *self.fails.lock().unwrap() = call;
    }

    fn now(&self) -> Option<String> {
        self.bound.lock().unwrap().clone()
    }

    fn call(&self, name: &str) -> Result<(), String> {
        self.calls.lock().unwrap().push(name.into());
        if *self.fails.lock().unwrap() == name {
            return Err(format!("{name} failed"));
        }
        Ok(())
    }

    fn calls(&self) -> Vec<String> {
        std::mem::take(&mut self.calls.lock().unwrap())
    }
}

impl Authority for Memory {
    fn bound(&self) -> Result<Option<String>, String> {
        if *self.fails.lock().unwrap() == "read" {
            return Err("read failed".into());
        }
        Ok(self.now())
    }

    fn revoke(&self) -> Result<(), String> {
        self.call("revoke")?;
        *self.bound.lock().unwrap() = None;
        Ok(())
    }

    fn publish(&self, generation: &str) -> Result<(), String> {
        self.call("publish")?;
        *self.bound.lock().unwrap() = Some(generation.into());
        Ok(())
    }
}

/// A board on `backend` seeded with `seed`, a store on it answering to an authority, and the
/// board bound.
pub(crate) fn bound_board(seed: Seed) -> (Fixture, Arc<Memory>, Store) {
    let f = Fixture::new(seed);
    let authority = Arc::new(Memory::default());
    let store = f.other().recovering(authority.clone());
    store.rebind().unwrap();
    authority.calls();
    (f, authority, store)
}

pub(crate) fn original() -> Seed {
    Seed::default().task("probe/original", "ready")
}

/// The title the seed gives the task.
pub(crate) const TITLE: &str = "Task probe/original";
pub(crate) const RESTORED: &str = "the board was copied or restored since this was read";
pub(crate) const STALE: &str = "the task changed since this was read";

fn retitle(store: &Store, slug: &str, at: &Bound, title: &str) -> Result<BoundTask, Error> {
    store.amend_bound(slug, at, title, &[], "owner")
}

#[test]
fn a_token_read_before_a_copy_never_writes_a_number_and_version_the_copy_gave_again() {
    let seed = original();
    let snapshot = seed.dump.clone();
    let (f, authority, store) = bound_board(seed);
    let place = (
        f.store.database().addr(),
        f.store.database().database().to_string(),
    );
    let lost = f.task("probe/lost");
    let lost_at = store.bound("probe/lost").unwrap().bound;
    let first = store.bound("probe/original").unwrap();
    let edited = retitle(&store, "probe/original", &first.bound, "Before restore").unwrap();
    assert_eq!((edited.bound.id, edited.bound.version), (first.bound.id, 1));

    store.copy(&snapshot).unwrap();
    let replacement = f.task("probe/replacement");
    assert_eq!(
        replacement.id, lost.id,
        "the copy gives the lost task's number again"
    );
    f.store
        .amend("probe/original", "After restore", &[], "owner")
        .unwrap();
    let now = store.bound("probe/original").unwrap();
    assert_eq!(
        (now.bound.id, now.bound.version),
        (edited.bound.id, edited.bound.version),
        "the same number at the same version, on another history"
    );
    assert_ne!(now.bound.generation, edited.bound.generation);
    assert_eq!(
        authority.now().as_deref(),
        Some(now.bound.generation.as_str())
    );
    assert_eq!(
        (
            f.store.database().addr(),
            f.store.database().database().to_string()
        ),
        place,
        "the same server and database"
    );

    let before = f.store.dump().unwrap();
    let tried = Store::tried_again_here();
    for (slug, at) in [
        ("probe/original", &edited.bound),
        ("probe/original", &first.bound),
        ("probe/replacement", &lost_at),
    ] {
        let why = refused(retitle(&store, slug, at, "Written by a stale token"));
        assert!(why.contains(RESTORED), "{slug}: {why}");
        // The fence itself, by a writer whose authority still names the generation it read.
        let behind = Arc::new(Memory::default());
        behind.publish(&at.generation).unwrap();
        let why = refused(retitle(
            &f.other().recovering(behind),
            slug,
            at,
            "Written late",
        ));
        assert!(why.contains(RESTORED), "{slug}: {why}");
    }
    assert_eq!(
        Store::tried_again_here(),
        tried,
        "a mismatch is refused, never read again"
    );
    assert_eq!(f.store.dump().unwrap(), before, "nothing was written");
    let again = retitle(&store, "probe/original", &now.bound, "On this history").unwrap();
    assert_eq!(
        (again.title.as_str(), again.bound.version),
        ("On this history", 2)
    );
}

#[test]
fn every_copy_makes_a_generation_no_earlier_token_holds() {
    let seed = original();
    let snapshot = seed.dump.clone();
    let (f, authority, store) = bound_board(seed);
    let mut seen = vec![store.bound("probe/original").unwrap().bound];
    for _ in 0..3 {
        // A store with no authority copies too, and the generation is the database's own.
        f.store.copy(&snapshot).unwrap();
        let why = refused(store.bound("probe/original"));
        assert!(why.contains(RESTORED), "{why}");
        store.rebind().unwrap();
        let now = store.bound("probe/original").unwrap().bound;
        assert_eq!((now.id, now.version), (seen[0].id, seen[0].version));
        for earlier in &seen {
            assert_ne!(earlier.generation, now.generation);
            let why = refused(retitle(&store, "probe/original", earlier, "Stale"));
            assert!(why.contains(RESTORED), "{why}");
        }
        seen.push(now);
    }
    assert_eq!(
        seen.last().unwrap().generation.len(),
        36,
        "{:?}",
        seen.last()
    );
    assert_eq!(authority.now(), f.store.recovery_generation().unwrap());
    assert_eq!(f.store.get("probe/original").unwrap().title, TITLE);
}

#[test]
fn a_copy_that_fails_keeps_the_generation_and_every_token_read_under_it() {
    let (f, authority, store) = bound_board(Seed { dump: everything() });
    let slug = everything().tasks[0].slug.clone();
    let at = store.bound(&slug).unwrap();
    let generation = f.store.recovery_generation().unwrap();
    let mut bad = everything();
    bad.messages[0].kind = "shout".into();
    bad.tasks[0].title = "Not this one".into();
    assert!(matches!(store.copy(&bad), Err(Error::Db(_))));
    assert_eq!(f.store.recovery_generation().unwrap(), generation);
    assert_eq!(authority.now(), generation);
    assert_eq!(
        authority.calls(),
        Vec::<String>::new(),
        "nothing was published"
    );
    assert_eq!(f.store.dump().unwrap(), everything());
    assert_eq!(store.bound(&slug).unwrap(), at);
    let edited = retitle(&store, &slug, &at.bound, "Still this board").unwrap();
    assert_eq!(edited.bound.version, at.bound.version + 1);
}

#[test]
fn a_bound_write_and_a_copy_at_once_never_leave_the_write_on_the_copied_board() {
    let seed = original();
    let snapshot = seed.dump.clone();
    let (f, authority, store) = bound_board(seed);
    for round in 0..6 {
        let at = store.bound("probe/original").unwrap().bound;
        // The writer's authority still names the generation it read: only the fence in its
        // transaction stands between it and the copied board.
        let behind = Arc::new(Memory::default());
        behind.publish(&at.generation).unwrap();
        let writer = f.other().recovering(behind);
        let copier = f.other().recovering(authority.clone());
        let barrier = Barrier::new(2);
        let wrote = std::thread::scope(|threads| {
            let wrote = threads.spawn(|| {
                barrier.wait();
                retitle(&writer, "probe/original", &at, "Written across the copy")
            });
            let copied = threads.spawn(|| {
                barrier.wait();
                copier.copy(&snapshot)
            });
            copied.join().unwrap().expect("the copy lands");
            wrote.join().unwrap()
        });
        if let Err(why) = &wrote {
            assert!(why.to_string().contains(RESTORED), "round {round}: {why}");
        }
        // Written before the swap and replaced by it, or refused after it: never on the board
        // the copy made, whose rows are the dump's.
        assert_eq!(
            f.store.dump().unwrap(),
            snapshot,
            "round {round}: {wrote:?}"
        );
        let why = refused(retitle(&writer, "probe/original", &at, "Written after"));
        assert!(why.contains(RESTORED), "round {round}: {why}");
    }
}

#[test]
fn two_bound_writes_of_one_token_at_once_write_once() {
    let (f, authority, store) = bound_board(original());
    let at = store.bound("probe/original").unwrap().bound;
    let (first, second) = (
        f.other().recovering(authority.clone()),
        f.other().recovering(authority.clone()),
    );
    let barrier = Barrier::new(2);
    let (a, b) = std::thread::scope(|threads| {
        let a = threads.spawn(|| {
            barrier.wait();
            retitle(&first, "probe/original", &at, "One")
        });
        let b = threads.spawn(|| {
            barrier.wait();
            retitle(&second, "probe/original", &at, "Two")
        });
        (a.join().unwrap(), b.join().unwrap())
    });
    let (won, lost) = match (a, b) {
        (Ok(won), Err(lost)) | (Err(lost), Ok(won)) => (won, lost),
        both => panic!("one of two writes of one token wins: {both:?}"),
    };
    assert!(lost.to_string().contains(STALE), "{lost}");
    assert_eq!(store.bound("probe/original").unwrap(), won);
    assert_eq!(
        f.said("probe/original").len(),
        1,
        "one retitle on the thread"
    );
}

#[test]
fn an_authority_rotation_cut_short_refuses_until_a_rebind_finishes_it() {
    let seed = original();
    let snapshot = seed.dump.clone();
    let (f, authority, store) = bound_board(seed);
    let at = store.bound("probe/original").unwrap().bound;

    // The copy's swap commits and the authority does not take its generation.
    authority.failing("publish");
    let why = store.copy(&snapshot).unwrap_err().to_string();
    assert!(
        why.contains("its authority did not take the new one (publish failed)"),
        "{why}"
    );
    let copied = f.store.recovery_generation().unwrap().unwrap();
    assert_ne!(copied, at.generation);
    assert_eq!(authority.now().as_deref(), Some(at.generation.as_str()));
    for why in [
        refused(store.bound("probe/original")),
        refused(retitle(&store, "probe/original", &at, "Across the cut")),
    ] {
        assert!(why.contains(RESTORED), "{why}");
    }

    // A rebind whose revoke fails changes nothing.
    authority.failing("revoke");
    let why = refused(store.rebind());
    assert!(
        why.contains("could not be revoked (revoke failed): nothing was changed"),
        "{why}"
    );
    assert_eq!(
        f.store.recovery_generation().unwrap().as_deref(),
        Some(copied.as_str())
    );
    assert_eq!(authority.calls(), ["publish", "revoke"]);

    // One whose publish fails has revoked first and replaced the generation: bound to none.
    authority.failing("publish");
    assert!(matches!(store.rebind(), Err(Error::Db(_))));
    assert_eq!(
        authority.calls(),
        ["revoke", "publish"],
        "revoked before it replaces"
    );
    assert_eq!(authority.now(), None);
    let replaced = f.store.recovery_generation().unwrap().unwrap();
    assert_ne!(replaced, copied, "the generation found is never adopted");
    let why = refused(store.bound("probe/original"));
    assert!(why.contains("bound to no recovery generation"), "{why}");
    let unbound = Bound {
        generation: replaced.clone(),
        ..at.clone()
    };
    let why = refused(retitle(&store, "probe/original", &unbound, "While unbound"));
    assert!(why.contains("bound to no recovery generation"), "{why}");

    // Run again, it binds a generation of its own, and the board writes.
    authority.failing("");
    let bound = store.rebind().unwrap();
    assert_ne!(bound, replaced);
    assert_eq!(authority.now().as_deref(), Some(bound.as_str()));
    assert_eq!(
        f.store.recovery_generation().unwrap().as_deref(),
        Some(bound.as_str())
    );
    let now = store.bound("probe/original").unwrap().bound;
    retitle(&store, "probe/original", &now, "Bound again").unwrap();
    assert_eq!(f.store.dump().unwrap().tasks[0].title, "Bound again");
}

#[test]
fn a_store_with_no_authority_or_one_it_cannot_read_refuses_every_bound_call() {
    let (f, authority, store) = bound_board(original());
    let at = store.bound("probe/original").unwrap().bound;
    let before = f.store.dump().unwrap();
    let none = "opened with no recovery authority";
    for why in [
        refused(f.store.bound("probe/original")),
        refused(retitle(&f.store, "probe/original", &at, "No authority")),
        refused(f.store.rebind()),
    ] {
        assert!(
            why.contains(none) && why.contains("refused in this environment"),
            "{why}"
        );
    }
    authority.failing("read");
    for why in [
        refused(store.bound("probe/original")),
        refused(retitle(&store, "probe/original", &at, "Unread authority")),
    ] {
        assert!(why.contains("could not be read (read failed)"), "{why}");
    }
    authority.failing("");
    authority.revoke().unwrap();
    let why = refused(retitle(&store, "probe/original", &at, "Revoked"));
    assert!(why.contains("bound to no recovery generation"), "{why}");
    assert_eq!(f.store.dump().unwrap(), before);
    assert_eq!(
        f.store.recovery_generation().unwrap().as_deref(),
        Some(at.generation.as_str())
    );
    // What is bound to no generation is untouched by any of it.
    f.store
        .amend("probe/original", "An unbound retitle", &[], "owner")
        .unwrap();
}

#[test]
fn a_rebind_refuses_the_tokens_of_every_writer_still_open() {
    let (f, authority, store) = bound_board(original());
    let open = f.other().recovering(authority.clone());
    let at = open.bound("probe/original").unwrap().bound;
    let rebound = store.rebind().unwrap();
    assert_ne!(rebound, at.generation);
    let why = refused(retitle(
        &open,
        "probe/original",
        &at,
        "From before the rebind",
    ));
    assert!(why.contains(RESTORED), "{why}");
    let behind = Arc::new(Memory::default());
    behind.publish(&at.generation).unwrap();
    let why = refused(retitle(
        &f.other().recovering(behind),
        "probe/original",
        &at,
        "Late",
    ));
    assert!(why.contains(RESTORED), "{why}");
    assert_eq!(f.store.get("probe/original").unwrap().title, TITLE);
    let now = open.bound("probe/original").unwrap().bound;
    assert_eq!(
        (now.generation.as_str(), now.version),
        (rebound.as_str(), at.version)
    );
}

#[test]
fn a_writer_of_the_schema_before_cannot_copy_past_the_generation() {
    let database = database();
    let open = |how| {
        Store::opening_on(
            database.another(),
            Box::new(|| NOW),
            Box::new(|_| false),
            HOST,
            &how,
        )
        .unwrap()
    };
    let older = open(Opening::write().at(Schema::this_build().before()));
    older.copy(&original().dump).unwrap();
    let authority = Arc::new(Memory::default());
    let current = open(Opening::write()).recovering(authority.clone());
    current.rebind().unwrap();
    let at = current.bound("probe/original").unwrap();
    let generation = current.recovery_generation().unwrap();
    for refusal in [
        refused(older.copy(&original().dump)),
        refused(older.amend("probe/original", "By the older build", &[], "owner")),
    ] {
        assert!(
            refusal.contains("this build writes only a board at its own schema"),
            "{refusal}"
        );
    }
    assert_eq!(current.recovery_generation().unwrap(), generation);
    assert_eq!(current.bound("probe/original").unwrap(), at);
    assert_eq!(
        older.get("probe/original").unwrap().title,
        TITLE,
        "it still reads"
    );
}

#[test]
fn a_bound_retitle_writes_as_an_unbound_one_and_answers_the_task_it_left() {
    let (f, _, store) = bound_board(original());
    let worker = f.session("worker");
    let read = store.bound("probe/original").unwrap();
    assert_eq!(read.title, TITLE);
    assert_eq!(read.bound.id, f.store.get("probe/original").unwrap().id);
    assert_eq!(
        serde_json::to_value(&read.bound).unwrap(),
        serde_json::json!({
            "compat": COMPAT, "generation": read.bound.generation,
            "id": read.bound.id, "version": read.bound.version,
        })
    );
    f.advance(5.0);
    let links = ["docs/TASKS.md".to_string()];
    let wrote = store
        .amend_bound(
            "probe/original",
            &read.bound,
            "  A new title ",
            &links,
            &worker,
        )
        .unwrap();
    assert_eq!(wrote.title, "A new title");
    assert_eq!(wrote.body, read.body);
    assert_eq!(
        wrote.bound,
        Bound {
            version: read.bound.version + 1,
            ..read.bound.clone()
        }
    );
    assert_eq!(store.bound("probe/original").unwrap(), wrote);
    let task = f.store.get("probe/original").unwrap();
    assert_eq!((task.title.as_str(), task.links.len()), ("A new title", 1));
    assert_eq!(
        f.said("probe/original"),
        [format!("{worker} retitled it: {TITLE} → A new title")]
    );
    assert_eq!(f.store.session(&worker).unwrap().row.last_seen, f.now());
    // The token it answered writes next; the one before it does not.
    retitle(&store, "probe/original", &wrote.bound, "A newer title").unwrap();
    let why = refused(retitle(&store, "probe/original", &wrote.bound, "Twice"));
    assert!(why.contains(STALE), "{why}");
}

#[test]
fn a_token_that_names_another_task_version_or_shape_writes_nothing() {
    let (f, _, store) = bound_board(original().task("probe/other", "ready"));
    let gone = f.session_with(NewSession::new("worker"));
    let at = store.bound("probe/original").unwrap().bound;
    let other = store.bound("probe/other").unwrap().bound;
    f.advance(5.0);
    let before = f.store.dump().unwrap();
    let tried = Store::tried_again_here();
    let links = ["docs/TASKS.md".to_string()];
    let write = |slug: &str, at: &Bound, title: &str, by: &str| {
        refused(store.amend_bound(slug, at, title, &links, by))
    };
    let cases = [
        (
            write("probe/original", &other, "Another's number", &gone),
            STALE,
        ),
        (
            write(
                "probe/original",
                &Bound {
                    version: at.version + 1,
                    ..at.clone()
                },
                "Ahead",
                &gone,
            ),
            STALE,
        ),
        (
            write(
                "probe/original",
                &Bound {
                    version: -1,
                    ..at.clone()
                },
                "Behind",
                &gone,
            ),
            STALE,
        ),
        (
            write(
                "probe/original",
                &Bound {
                    generation: "another".into(),
                    ..at.clone()
                },
                "Elsewhere",
                &gone,
            ),
            RESTORED,
        ),
        (
            write(
                "probe/original",
                &Bound {
                    generation: String::new(),
                    ..at.clone()
                },
                "Nowhere",
                &gone,
            ),
            RESTORED,
        ),
        (
            write(
                "probe/original",
                &Bound {
                    compat: COMPAT + 1,
                    ..at.clone()
                },
                "Newer shape",
                &gone,
            ),
            "compatibility version 2, and this build checks version 1",
        ),
        (
            write(
                "probe/original",
                &Bound {
                    compat: 0,
                    ..at.clone()
                },
                "No shape",
                &gone,
            ),
            "compatibility version 0",
        ),
        (
            write("probe/none", &at, "No such task", &gone),
            "no task 'probe/none'",
        ),
        (
            write("probe/original", &at, "   ", &gone),
            "a task needs a title",
        ),
        (
            write("probe/original", &at, "By nobody", "nobody-there"),
            "nobody-there",
        ),
    ];
    for (why, expected) in &cases {
        assert!(why.contains(expected), "{expected}: {why}");
    }
    assert_eq!(refused(store.bound("probe/none")), "no task 'probe/none'");
    assert_eq!(Store::tried_again_here(), tried);
    assert_eq!(
        f.store.dump().unwrap(),
        before,
        "no title, link, line or act was written"
    );
    assert!(
        serde_json::from_value::<Bound>(serde_json::json!({
            "compat": 1, "generation": "g", "id": 1, "version": 0, "slug": "probe/original",
        }))
        .is_err(),
        "a token of another shape does not read as this one"
    );
    // An ordinary write moves the version on, and the token read before it is stale.
    f.store
        .amend("probe/original", "Retitled unbound", &[], "owner")
        .unwrap();
    assert!(write("probe/original", &at, "On the old base", &gone).contains(STALE));
    f.store.leave(&gone).unwrap();
    let now = store.bound("probe/original").unwrap().bound;
    let why = write("probe/original", &now, "By one who left", &gone);
    assert!(why.contains("left"), "{why}");
    assert_eq!(
        f.store.get("probe/original").unwrap().title,
        "Retitled unbound"
    );
}

#[test]
fn each_board_is_made_with_a_generation_of_its_own_that_no_ordinary_write_moves() {
    let (f, g) = (Fixture::new(original()), Fixture::new(original()));
    let made = f.store.recovery_generation().unwrap().unwrap();
    assert_eq!(made.len(), 36, "{made}");
    assert_ne!(Some(made.clone()), g.store.recovery_generation().unwrap());
    f.task("probe/another");
    f.store
        .amend("probe/original", "Retitled", &[], "owner")
        .unwrap();
    assert_eq!(f.other().recovery_generation().unwrap(), Some(made));
    let dump = serde_json::to_string(&f.store.dump().unwrap()).unwrap();
    assert!(
        !dump.contains("generation"),
        "a dump carries no generation: {dump}"
    );
}
