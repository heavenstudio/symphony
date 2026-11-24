//! Which group a thing belongs to (`symphony_board::people::ownership`, `Store::map_place`,
//! `Store::hand_task`, `Store::hand_card`): the map an admin keeps, each change a numbered line of
//! the board's ownership log, read back as the map; a task filed for a group, the group its place
//! maps to else; a hand-over by a member of its group, recorded with who made it, and one refused
//! to a non-member with nothing written. A board with nobody named gives every thing to the person
//! at the machine, and a board made without the step reads no map and takes no change.

use crate::common::{Fixture, HOST, NOW, Seed, database, refused};
use std::time::Duration;
use symphony_board::people::Power;
use symphony_board::people::ownership::{By, Did, Kind, Line, MACHINE, Place, Thing};
use symphony_board::schema::{ADDED, OWNERSHIP};
use symphony_board::{NewTask, Opening, Schema, Store};

/// The session every change of the map is recorded as made by.
const BY: &str = "amber-heron";

/// A board naming ana (in design) and bo (in engineering), with both groups, and the area `app`
/// mapped to engineering.
fn board() -> Fixture {
    let f = Fixture::new(Seed::default().session(BY));
    for group in ["design", "engineering"] {
        f.store.add_group(group, &[Power::Decide], BY).unwrap();
    }
    f.store
        .add_person("ana", &["design".into()], &[], BY)
        .unwrap();
    f.store
        .add_person("bo", &["engineering".into()], &[], BY)
        .unwrap();
    f.store
        .map_place(&Place::Area("app".into()), "engineering", BY)
        .unwrap();
    f
}

/// A store on `f`'s board writing for the person `name`.
fn person(f: &Fixture, name: &str) -> Store {
    f.other().for_person(name)
}

/// Whose the task `slug` is, as `(name, kind, by)`.
fn whose(f: &Fixture, slug: &str) -> (String, Kind, By) {
    let task = f.store.get(slug).unwrap();
    let owning = f.store.task_owning(&task).unwrap();
    (owning.name, owning.kind, owning.by)
}

/// The board's line on `slug`'s thread, newest.
fn newest_line(f: &Fixture, slug: &str) -> String {
    let thread = f.store.thread(slug).unwrap();
    thread.last().expect("a line").text.clone()
}

#[test]
fn a_board_with_nobody_named_gives_every_card_and_task_to_the_person_at_the_machine() {
    let f = Fixture::new(Seed::default().session(BY));
    f.store.add_group("design", &[], BY).unwrap();
    f.store
        .map_place(&Place::Area("app".into()), "design", BY)
        .unwrap();
    let filed = f.task_with(NewTask {
        group: Some("design".into()),
        ..NewTask::new("app/x", "X", BY)
    });
    assert_eq!(
        filed.group(),
        Some("design"),
        "the name is written all the same"
    );
    assert_eq!(
        whose(&f, "app/x"),
        (MACHINE.into(), Kind::Machine, By::Default)
    );
    let card = Thing::Card {
        slug: "new-palette".into(),
        route: "docs/design/palette.md".into(),
    };
    let owning = f.store.owning(&card, Some("design")).unwrap();
    assert_eq!(
        (owning.name.as_str(), owning.kind),
        (MACHINE, Kind::Machine)
    );
    let why = refused(f.store.hand_task("app/x", "design", "owner"));
    assert!(why.contains("names nobody"), "{why}");
    assert!(f.store.task_deciders(&filed).is_empty());
    assert_eq!(f.store.refused_task_decide("app/x", "ana").unwrap(), None);
    let stamp = f.store.task_stamp("app/x").unwrap().unwrap();
    assert!(stamp.task_deciders(&filed).is_empty());
}

#[test]
fn the_map_is_kept_a_line_at_a_time_and_read_back() {
    let f = board();
    let design = Place::Folder("docs/design".into());
    let line = f.store.map_place(&design, "design", BY).unwrap().unwrap();
    assert_eq!((line.n, line.did, line.by.as_str()), (2, Did::Map, BY));
    assert_eq!(line.place.as_deref(), Some("folder:docs/design"));
    assert_eq!(line.at, NOW);
    assert_eq!(
        f.store.map_place(&design, "design", BY).unwrap(),
        None,
        "mapped so already: nothing written"
    );
    let map = f.store.ownership_map().unwrap();
    assert_eq!(map.group(&design), Some("design"));
    assert_eq!(map.group(&Place::Area("app".into())), Some("engineering"));
    assert_eq!(map.revision, 2);
    let unmapped = f.store.unmap_place(&design, BY).unwrap();
    assert_eq!((unmapped.n, unmapped.did), (3, Did::Unmap));
    assert_eq!(f.store.ownership_map().unwrap().group(&design), None);
    let why = refused(f.store.unmap_place(&design, BY));
    assert!(
        why.contains("names no group for the folder docs/design"),
        "{why}"
    );
    assert!(why.ends_with("nothing was written"), "{why}");
    let why = refused(f.store.map_place(&Place::Settings, "marketing", BY));
    assert!(why.contains("keeps no group marketing"), "{why}");
    let why = refused(f.store.map_place(&Place::Settings, "design", " "));
    assert!(why.contains("says who made it"), "{why}");
    assert_eq!(
        f.store.ownership_log().unwrap().len(),
        3,
        "no refusal wrote a line"
    );
}

#[test]
fn a_task_belongs_to_its_areas_group_else_the_default_else_the_machine() {
    let f = board();
    f.task("app/mapped");
    f.task("tools/unmapped");
    assert_eq!(
        whose(&f, "app/mapped"),
        ("engineering".into(), Kind::Group, By::Map)
    );
    assert_eq!(
        whose(&f, "tools/unmapped"),
        (MACHINE.into(), Kind::Machine, By::Default)
    );
    f.store.map_place(&Place::Default, "design", BY).unwrap();
    assert_eq!(
        whose(&f, "tools/unmapped"),
        ("design".into(), Kind::Group, By::Default)
    );
}

#[test]
fn a_name_written_on_a_task_overrides_the_map_and_one_the_board_lacks_falls_back() {
    let f = board();
    for (slug, group) in [
        ("app/to-design", "design"),
        ("app/to-ana", "ana"),
        ("app/to-nobody", "marketing"),
    ] {
        f.task_with(NewTask {
            group: Some(group.into()),
            ..NewTask::new(slug, "X", BY)
        });
    }
    assert_eq!(
        whose(&f, "app/to-design"),
        ("design".into(), Kind::Group, By::Named)
    );
    assert_eq!(
        whose(&f, "app/to-ana"),
        ("ana".into(), Kind::Person, By::Named)
    );
    let task = f.store.get("app/to-nobody").unwrap();
    let owning = f.store.task_owning(&task).unwrap();
    assert_eq!((owning.name.as_str(), owning.by), ("engineering", By::Map));
    assert_eq!(owning.unheld.as_deref(), Some("marketing"));
    let why = refused(f.store.create(&NewTask {
        group: Some("Not A Name".into()),
        ..NewTask::new("app/bad", "X", BY)
    }));
    assert!(why.contains("short name, or owner"), "{why}");
}

#[test]
fn a_member_hands_a_task_on_and_the_board_says_who_did() {
    let f = board();
    f.task("app/x");
    let bo = person(&f, "bo");
    let handed = bo.hand_task("app/x", "design", "bo").unwrap().unwrap();
    assert_eq!(handed.group(), Some("design"));
    assert_eq!(
        whose(&f, "app/x"),
        ("design".into(), Kind::Group, By::Named)
    );
    assert_eq!(
        newest_line(&f, "app/x"),
        "bo handed it to design (it was engineering, from the area app)"
    );
    let line = f.store.ownership_log().unwrap().pop().unwrap();
    assert_eq!(
        (
            line.did,
            line.by.as_str(),
            line.thing.as_deref(),
            line.name.as_deref(),
            line.from.as_deref()
        ),
        (
            Did::Hand,
            "bo",
            Some("task app/x"),
            Some("design"),
            Some("engineering")
        )
    );
    // Hers now: ana hands it to a person, and handing it where it is already writes nothing.
    let ana = person(&f, "ana");
    assert_eq!(ana.hand_task("app/x", "design", "ana").unwrap(), None);
    ana.hand_task("app/x", "bo", "ana").unwrap().unwrap();
    assert_eq!(whose(&f, "app/x"), ("bo".into(), Kind::Person, By::Named));
}

#[test]
fn a_hand_over_by_a_non_member_is_refused_with_nothing_written() {
    let f = board();
    f.task("app/x");
    let before = f.store.thread("app/x").unwrap().len();
    let ana = person(&f, "ana");
    let why = refused(ana.hand_task("app/x", "design", "ana"));
    assert_eq!(
        why,
        "ana is no member of the group engineering, whose it is, so may not hand it on: nothing \
         was written"
    );
    let why = refused(person(&f, "bo").hand_task("app/x", "marketing", "bo"));
    assert!(why.contains("holds no group or person marketing"), "{why}");
    let why = refused(f.store.hand_task("app/x", "design", BY));
    assert!(
        why.contains("is no person"),
        "a session hands nothing on: {why}"
    );
    assert_eq!(f.store.get("app/x").unwrap().group(), None);
    assert_eq!(f.store.thread("app/x").unwrap().len(), before);
    assert_eq!(
        f.store.ownership_log().unwrap().len(),
        1,
        "the map's own line alone"
    );
}

#[test]
fn the_person_at_the_machine_hands_any_open_task_and_none_closed() {
    let f = board();
    f.task("app/x");
    f.store
        .hand_task("app/x", "design", "owner")
        .unwrap()
        .unwrap();
    assert_eq!(
        newest_line(&f, "app/x"),
        "owner handed it to design (it was engineering, from the area app)"
    );
    let why = refused(f.store.hand_task("app/missing", "design", "owner"));
    assert!(why.contains("app/missing"), "{why}");
    f.store.move_to("app/x", "cancelled", "owner").unwrap();
    let why = refused(f.store.hand_task("app/x", "engineering", "owner"));
    assert!(why.contains("only an open task is handed on"), "{why}");
}

#[test]
fn a_card_is_handed_by_a_member_and_the_board_keeps_who_did() {
    let f = board();
    f.store
        .map_place(&Place::Folder("docs/design".into()), "design", BY)
        .unwrap();
    let card = Thing::Card {
        slug: "new-palette".into(),
        route: "docs/design/palette.md".into(),
    };
    let owning = f.store.owning(&card, None).unwrap();
    assert_eq!((owning.name.as_str(), owning.by), ("design", By::Map));
    let why = refused(person(&f, "bo").hand_card(&card, None, "engineering", "bo"));
    assert!(why.contains("bo is no member of the group design"), "{why}");
    let line = person(&f, "ana")
        .hand_card(&card, None, "engineering", "ana")
        .unwrap()
        .unwrap();
    assert_eq!(
        (
            line.did,
            line.by.as_str(),
            line.thing.as_deref(),
            line.name.as_deref(),
            line.from.as_deref()
        ),
        (
            Did::Hand,
            "ana",
            Some("card new-palette"),
            Some("engineering"),
            Some("design")
        )
    );
    // Written to engineering now, which its file says: bo's to hand, and ana's no more.
    let now = Some("engineering");
    assert_eq!(
        person(&f, "bo")
            .hand_card(&card, now, "engineering", "bo")
            .unwrap(),
        None
    );
    assert!(
        person(&f, "ana")
            .hand_card(&card, now, "design", "ana")
            .is_err()
    );
    let back = person(&f, "bo")
        .hand_card(&card, now, "design", "bo")
        .unwrap();
    assert!(back.is_some());
}

/// Two hand-overs for one line number: a card's hand-over line, written by another and not yet
/// committed, holds the number a task's hand-over decided on, so the task's write waits on it; once
/// it commits, the task's hand-over is decided again on the log it left and takes the next number,
/// with no database error to the person and its thread's line said once.
#[test]
fn a_hand_over_that_loses_its_line_number_to_another_is_decided_again() {
    let f = board();
    f.task("app/x");
    let next = f
        .store
        .ownership_log()
        .unwrap()
        .last()
        .map_or(0, |line| line.n)
        + 1;
    let card = Line {
        n: next,
        at: NOW,
        by: "ana".into(),
        did: Did::Hand,
        place: None,
        thing: Some("card new-palette".into()),
        name: Some("design".into()),
        from: Some("engineering".into()),
    };
    let doc = serde_json::to_string(&card).unwrap().replace('\'', "''");
    let holder = f.postgresql();
    holder
        .simple(&format!(
            "BEGIN; INSERT INTO ownership (id, doc) VALUES ('{next}'::jsonb, '{doc}'::jsonb)"
        ))
        .unwrap();
    let bo = person(&f, "bo");
    let handing = std::thread::spawn(move || bo.hand_task("app/x", "design", "bo"));
    let watcher = f.postgresql();
    let waiting = "SELECT count(*) FROM pg_stat_activity WHERE datname = current_database() \
                   AND wait_event_type = 'Lock'";
    test_time::until_true(
        "the task's hand-over waits on the held line",
        test_time::backstop(Duration::from_secs(30)),
        || watcher.simple(waiting).unwrap()[0].value() != Some("0"),
    )
    .unwrap_or_else(|stall| panic!("{stall}"));
    holder.simple("COMMIT").unwrap();
    let handed = (handing.join().unwrap())
        .unwrap_or_else(|why| panic!("decided again, never a database error: {why:?}"))
        .expect("the task changes hands");
    assert_eq!(handed.group(), Some("design"));
    let log = f.store.ownership_log().unwrap();
    let ns: Vec<i64> = log.iter().map(|line| line.n).collect();
    assert_eq!(&ns[ns.len() - 2..], [next, next + 1]);
    assert_eq!(log[log.len() - 2], card, "the other's line as it wrote it");
    let last = log.last().unwrap();
    assert_eq!(
        (
            last.by.as_str(),
            last.thing.as_deref(),
            last.from.as_deref()
        ),
        ("bo", Some("task app/x"), Some("engineering"))
    );
    let thread = f.store.thread("app/x").unwrap();
    let said: Vec<&str> = (thread.iter().map(|line| line.text.as_str()))
        .filter(|text| text.contains("handed it"))
        .collect();
    assert_eq!(
        said,
        ["bo handed it to design (it was engineering, from the area app)"]
    );
}

/// A card's hand-over line taken back, as a save does when the card's file is not written: gone
/// from the log while it is the line written, a line after it keeping its number and the map its
/// revision; nothing taken back twice, nor a line that is no hand-over.
#[test]
fn a_cards_hand_over_line_is_taken_back_and_nothing_else_is() {
    let f = board();
    f.store
        .map_place(&Place::Folder("docs/design".into()), "design", BY)
        .unwrap();
    let card = Thing::Card {
        slug: "new-palette".into(),
        route: "docs/design/palette.md".into(),
    };
    let ana = person(&f, "ana");
    let line = (ana.hand_card(&card, None, "engineering", "ana")).unwrap();
    let line = line.expect("design's card goes to engineering");
    let after = (f.store.map_place(&Place::Default, "design", BY)).unwrap();
    let after = after.expect("the default is mapped");
    assert!(ana.take_back(&line).unwrap());
    let log = f.store.ownership_log().unwrap();
    assert!(log.iter().all(|kept| kept.n != line.n), "{log:?}");
    assert_eq!(log.last(), Some(&after));
    assert_eq!(f.store.ownership_map().unwrap().revision, after.n);
    assert!(!ana.take_back(&line).unwrap(), "taken back once");
    assert!(
        !ana.take_back(&after).unwrap(),
        "a map line is no hand-over"
    );
    let changed = Line {
        name: Some("bo".into()),
        ..line.clone()
    };
    let again = (ana.hand_card(&card, None, "engineering", "ana"))
        .unwrap()
        .unwrap();
    assert!(
        !ana.take_back(&Line {
            n: again.n,
            ..changed
        })
        .unwrap(),
        "a line other than the one written stays"
    );
    assert_eq!(f.store.ownership_log().unwrap().last(), Some(&again));
}

#[test]
fn a_member_who_left_the_group_hands_none_of_its_things() {
    let f = board();
    f.task("app/x");
    f.store.leave_group("bo", "engineering", BY).unwrap();
    let why = refused(person(&f, "bo").hand_task("app/x", "design", "bo"));
    assert!(why.contains("no member of the group engineering"), "{why}");
}

#[test]
fn the_ownership_log_is_carried_by_a_copy() {
    let f = board();
    let carried = f.store.carried().unwrap();
    let ids: Vec<String> = (carried["ownership"].as_array().unwrap().iter())
        .map(|row| row["id"].to_string())
        .collect();
    assert_eq!(ids, ["1"]);
}

#[test]
fn a_board_made_without_the_step_reads_no_map_and_takes_no_change() {
    let db = database();
    let step = ADDED.iter().position(|step| *step == OWNERSHIP).unwrap();
    let before = Opening::write().at(Schema::of_this_contract(i64::try_from(step).unwrap()));
    let open = |how: &Opening| {
        Store::opening_on(
            db.another(),
            Box::new(|| NOW),
            Box::new(|_| false),
            HOST,
            how,
        )
        .unwrap()
    };
    let writer = open(&before);
    assert_eq!(writer.additions().unwrap(), i64::try_from(step).unwrap());
    let worker = writer
        .register(&symphony_board::NewSession::new("worker"))
        .unwrap();
    writer.add_group("design", &[], "amber-heron").ok();
    // A person holding decide decides no task there: with no map, each is the person at the
    // machine's, its stamp reading no map at all.
    writer
        .add_person("ana", &["design".into()], &[Power::Decide], BY)
        .unwrap();
    let task = writer
        .create(&NewTask::new("app/x", "X", &worker.row.handle))
        .unwrap();
    let stamp = writer.task_stamp("app/x").unwrap().unwrap();
    assert!(stamp.task_deciders(&task).is_empty());
    assert!(writer.task_deciders(&task).is_empty());
    for store in [&writer, &open(&Opening::read())] {
        assert_eq!(store.ownership_log().unwrap(), []);
        assert!(store.ownership_map().unwrap().is_empty());
        let why = refused(store.map_place(&Place::Settings, "design", BY));
        assert!(
            why.starts_with("the map of places is not on this board yet"),
            "{why}"
        );
        assert!(why.ends_with("nothing was written"), "{why}");
        let why = refused(store.hand_task("app/x", "design", "owner"));
        assert!(
            why.starts_with("the map of places is not on this board yet"),
            "{why}"
        );
    }
}

#[test]
fn a_tasks_deciders_are_those_of_its_owner_who_hold_decide_and_its_stamp_follows_the_map() {
    use symphony_board::workflow::{Host, local_owner, person as principal};
    let f = board();
    f.task("app/x");
    f.task("docs/y");
    let task = f.store.get("app/x").unwrap();
    assert_eq!(f.store.task_deciders(&task), ["bo"]);
    assert_eq!(f.store.task_deciders_of("app/x"), ["bo"]);
    assert!(
        f.store.task_deciders_of("docs/y").is_empty(),
        "an area the map does not name, with no default: the person at the machine's"
    );
    assert!(f.store.task_deciders_of("app/missing").is_empty());
    let stamp = f.store.task_stamp("app/x").unwrap().unwrap();
    assert_eq!(stamp.task_deciders(&task), ["bo"]);
    // Why one may not decide it, in the board's words: none for one who may.
    assert_eq!(f.store.refused_task_decide("app/x", "bo").unwrap(), None);
    assert_eq!(
        f.store
            .refused_task_decide("app/x", "ana")
            .unwrap()
            .as_deref(),
        Some("ana is no member of the group engineering, whose it is, so may not decide it")
    );
    assert!(f.store.refused_task_decide("app/missing", "bo").is_err());

    // A remap moves who decides it, and the stamp tells the task from the one read before.
    f.store
        .map_place(&Place::Area("app".into()), "design", BY)
        .unwrap();
    let remapped = f.store.task_stamp("app/x").unwrap().unwrap();
    assert_ne!(remapped, stamp);
    assert_eq!(remapped.task_deciders(&task), ["ana"]);
    assert_eq!(f.store.task_deciders(&task), ["ana"]);

    // The workflow's owner edges are granted to the task's deciders alone.
    let ana = f.store.host_on(&principal("ana"), "app/x").unwrap();
    assert!(ana.grants.holds(&principal("ana"), "owner"));
    assert!(!ana.grants.holds(&principal("bo"), "owner"));
    assert!(ana.grants.holds(&local_owner(), "owner"));
    assert_eq!(
        f.store.host_on(&local_owner(), "app/missing").unwrap(),
        Host::local(),
        "the person at the machine's host reads nothing"
    );
    let why = refused(f.store.host_on(&principal("ana"), "app/missing"));
    assert!(why.contains("app/missing"), "{why}");

    // A member who holds no decide is none, until it is given them.
    f.store.add_group("staff", &[Power::Speak], BY).unwrap();
    f.store
        .add_person("cy", &["staff".into()], &[], BY)
        .unwrap();
    f.store
        .map_place(&Place::Area("app".into()), "staff", BY)
        .unwrap();
    assert!(f.store.task_deciders(&task).is_empty());
    f.store
        .grant(
            symphony_board::people::Holder::Person("cy"),
            &[Power::Decide],
            BY,
        )
        .unwrap();
    assert_eq!(f.store.task_deciders(&task), ["cy"]);
    let granted = f.store.task_stamp("app/x").unwrap().unwrap();
    assert_eq!(granted.task_deciders(&task), ["cy"]);

    // A task handed to a person is theirs to decide, read from its own row.
    person(&f, "cy").hand_task("app/x", "ana", "cy").unwrap();
    let handed = f.store.get("app/x").unwrap();
    let after = f.store.task_stamp("app/x").unwrap().unwrap();
    assert_eq!(after.task_deciders(&handed), ["ana"]);
    assert_eq!(f.store.task_deciders(&handed), ["ana"]);
}
