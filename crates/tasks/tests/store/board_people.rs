//! Who is on a board (`symphony_board::people`, `Store::add_person` and the changes beside it): its
//! people, groups and powers kept in the tables of `schema::PEOPLE`, each change a numbered line of
//! the people log by short names alone, every change fenced on the newest so none passes another,
//! and a refusal that writes nothing. A board made without the step reads nobody and takes no
//! change; a session's handle is never a person's name, nor theirs a handle.

use crate::common::{Fixture, HOST, NOW, Seed, database, refused};
use symphony_board::people::{Did, Holder, People, Power};
use symphony_board::schema::{ADDED, PEOPLE};
use symphony_board::workflow::{Host, local_owner, person};
use symphony_board::{NewSession, NewTask, Opening, Pick, Schema, Store};

/// The session every change is recorded as made by.
const BY: &str = "amber-heron";

fn board() -> Fixture {
    Fixture::new(Seed::default().session(BY))
}

/// `f`'s people as `(name, held powers, groups)`.
fn listed(store: &Store) -> Vec<(String, Vec<Power>, Vec<String>)> {
    let people = store.people().unwrap();
    (people.members.iter())
        .map(|member| {
            let held = people.powers_of(&member.name).held();
            (member.name.clone(), held, member.groups.clone())
        })
        .collect()
}

/// The log as `(n, did, name)`.
fn logged(store: &Store) -> Vec<(i64, Did, String)> {
    (store.people_log().unwrap().into_iter())
        .map(|line| (line.n, line.did, line.name))
        .collect()
}

#[test]
fn a_board_never_told_of_anyone_holds_nobody_and_no_change() {
    let f = board();
    let people = f.store.people().unwrap();
    assert_eq!(
        (people.members.len(), people.groups.len(), people.revision),
        (0, 0, 0)
    );
    assert_eq!(f.store.people_log().unwrap(), []);
    assert!(f.store.deciders().is_empty());
    assert_eq!(f.store.host_for(&local_owner()).unwrap(), Host::local());
}

#[test]
fn people_and_groups_are_kept_with_their_powers_and_every_change_is_a_numbered_line() {
    let f = board();
    let staff = f.store.add_group("staff", &[Power::Speak], BY).unwrap();
    assert_eq!(
        (staff.n, staff.did, staff.powers.clone()),
        (1, Did::GroupAdd, vec![Power::Speak])
    );
    let ana = f
        .store
        .add_person(
            "ana",
            &["staff".into()],
            &[Power::Decide, Power::Decide],
            BY,
        )
        .unwrap();
    assert_eq!(
        (ana.n, ana.did, ana.powers.clone()),
        (2, Did::Add, vec![Power::Decide])
    );
    f.advance(60.0);
    f.store.add_person("bob", &[], &[], BY).unwrap();
    assert_eq!(
        listed(&f.store),
        [
            (
                "ana".to_string(),
                vec![Power::Read, Power::Speak, Power::Decide],
                vec!["staff".to_string()]
            ),
            ("bob".to_string(), vec![], vec![]),
        ]
    );
    let people = f.store.people().unwrap();
    assert_eq!(people.revision, 3);
    assert_eq!(people.member("ana").unwrap().revision, 2);
    assert_eq!(people.group("staff").unwrap().revision, 1);

    let joined = f.store.join_group("bob", "staff", BY).unwrap().unwrap();
    assert_eq!((joined.n, joined.group.as_deref()), (4, Some("staff")));
    assert_eq!(
        f.store.join_group("bob", "staff", BY).unwrap(),
        None,
        "in it already"
    );
    let granted = f
        .store
        .grant(Holder::Person("bob"), &[Power::Speak, Power::Decide], BY);
    assert_eq!(
        granted.unwrap().unwrap().powers,
        [Power::Speak, Power::Decide]
    );
    assert_eq!(
        f.store
            .grant(Holder::Person("bob"), &[Power::Decide], BY)
            .unwrap(),
        None,
        "a power held already writes nothing"
    );
    let revoked = f.store.revoke(
        Holder::Person("bob"),
        &[Power::Decide, Power::Administer],
        BY,
    );
    assert_eq!(
        revoked.unwrap().powers,
        [Power::Decide],
        "the line names what it took"
    );
    let group_grant = f
        .store
        .grant(Holder::Group("staff"), &[Power::Administer], BY);
    let group_grant = group_grant.unwrap().unwrap();
    assert_eq!(
        (group_grant.name.as_str(), group_grant.group.as_deref()),
        ("staff", Some("staff"))
    );
    f.store.leave_group("bob", "staff", BY).unwrap();
    assert_eq!(f.store.deciders(), ["ana"]);
    assert_eq!(
        listed(&f.store)[1],
        ("bob".to_string(), vec![Power::Read, Power::Speak], vec![])
    );
    f.store.remove_person("bob", BY).unwrap();
    assert!(f.store.people().unwrap().member("bob").is_none());

    let log = f.store.people_log().unwrap();
    assert_eq!(
        (log.iter().map(|line| line.n)).collect::<Vec<_>>(),
        (1..=9).collect::<Vec<_>>(),
        "a change of nothing is no line"
    );
    assert!(log.iter().all(|line| line.by == BY));
    assert_eq!(log[2].at, NOW + 60.0);
    assert_eq!(
        (log.iter().map(|line| line.did)).collect::<Vec<_>>(),
        [
            Did::GroupAdd,
            Did::Add,
            Did::Add,
            Did::Join,
            Did::Grant,
            Did::Revoke,
            Did::Grant,
            Did::Leave,
            Did::Remove,
        ]
    );
}

#[test]
fn a_groups_removal_takes_its_members_out_of_it_in_the_same_change() {
    let f = board();
    f.store.add_group("staff", &[Power::Decide], BY).unwrap();
    f.store.add_group("crew", &[Power::Speak], BY).unwrap();
    f.store
        .add_person("ana", &["staff".into(), "crew".into()], &[], BY)
        .unwrap();
    assert_eq!(f.store.deciders(), ["ana"]);
    let removed = f.store.remove_group("staff", BY).unwrap();
    assert_eq!((removed.n, removed.did), (4, Did::GroupRemove));
    let people = f.store.people().unwrap();
    assert!(people.group("staff").is_none());
    let ana = people.member("ana").unwrap();
    assert_eq!(
        (ana.groups.clone(), ana.revision),
        (vec!["crew".to_string()], 4)
    );
    assert!(f.store.deciders().is_empty());
}

#[test]
fn a_refused_change_says_why_and_writes_nothing() {
    let f = board();
    f.store.add_group("staff", &[], BY).unwrap();
    f.store.add_person("ana", &[], &[Power::Speak], BY).unwrap();
    let before = (f.store.people().unwrap(), logged(&f.store));
    let refusals = [
        refused(f.store.add_person("Ana", &[], &[], BY)),
        refused(f.store.add_person("ana", &[], &[], BY)),
        refused(f.store.add_person("staff", &[], &[], BY)),
        refused(f.store.add_person(BY, &[], &[], BY)),
        refused(f.store.add_person("owner", &[], &[], BY)),
        refused(f.store.add_person("bob", &["crew".into()], &[], BY)),
        refused(f.store.add_person("bob", &[], &[], " ")),
        refused(f.store.add_group("ana", &[], BY)),
        refused(f.store.remove_person("bob", BY)),
        refused(f.store.join_group("ana", "crew", BY)),
        refused(f.store.join_group("bob", "staff", BY)),
        refused(f.store.leave_group("ana", "staff", BY)),
        refused(f.store.grant(Holder::Person("ana"), &[], BY)),
        refused(f.store.grant(Holder::Group("crew"), &[Power::Decide], BY)),
        refused(f.store.revoke(Holder::Person("ana"), &[Power::Decide], BY)),
        refused(f.store.remove_group("crew", BY)),
        refused(f.store.revoke(Holder::Group("staff"), &[Power::Speak], BY)),
    ];
    for why in &refusals {
        assert!(why.ends_with("nothing was written"), "{why}");
    }
    assert!(refusals[0].contains("is no short name"), "{}", refusals[0]);
    assert!(refusals[1].starts_with("ana is a person on this board already"));
    assert!(refusals[2].starts_with("staff is a group of this board"));
    assert!(
        refusals[3].starts_with("amber-heron is a name a session's handle can take"),
        "{}",
        refusals[3]
    );
    assert!(refusals[5].starts_with("this board keeps no group crew"));
    assert!(refusals[6].contains("says who made it"));
    assert!(refusals[7].starts_with("ana is a person on this board already"));
    assert!(refusals[8].starts_with("no person bob is on this board"));
    assert!(refusals[11].starts_with("ana is in no group staff"));
    assert!(refusals[12].starts_with("name at least one power"));
    assert!(refusals[14].starts_with("the person ana holds none of those powers"));
    assert!(refusals[16].starts_with("the group staff holds none of those powers"));
    assert_eq!((f.store.people().unwrap(), logged(&f.store)), before);
}

#[test]
fn direct_agents_is_given_read_and_taken_as_every_power_is() {
    let f = board();
    let crew = f
        .store
        .add_group("crew", &[Power::DirectAgents], BY)
        .unwrap();
    assert_eq!(crew.powers, [Power::DirectAgents]);
    f.store
        .add_person("ana", &[], &[Power::Speak, Power::DirectAgents], BY)
        .unwrap();
    f.store
        .add_person("bob", &["crew".into()], &[Power::Speak], BY)
        .unwrap();
    f.store.add_person("cid", &[], &[Power::Speak], BY).unwrap();
    let holds = |name: &str| {
        let people = f.store.people().unwrap();
        people.powers_of(name).holds(Power::DirectAgents)
    };
    assert!(holds("ana"), "given her own");
    assert!(holds("bob"), "given by his group");
    assert!(!holds("cid"));
    let granted = f
        .store
        .grant(Holder::Person("cid"), &[Power::DirectAgents], BY);
    let granted = granted
        .unwrap()
        .expect("a grant of a power not held changes the board");
    assert_eq!(
        (granted.did, granted.powers),
        (Did::Grant, vec![Power::DirectAgents])
    );
    assert!(holds("cid"));
    for name in ["ana", "cid"] {
        let taken = f
            .store
            .revoke(Holder::Person(name), &[Power::DirectAgents], BY);
        assert_eq!(taken.unwrap().powers, [Power::DirectAgents]);
        assert!(!holds(name), "{name} lost it");
    }
    f.store
        .revoke(Holder::Group("crew"), &[Power::DirectAgents], BY)
        .unwrap();
    assert!(!holds("bob"), "his group lost it");
    assert_eq!(
        listed(&f.store),
        [
            ("ana".into(), vec![Power::Read, Power::Speak], vec![]),
            (
                "bob".into(),
                vec![Power::Read, Power::Speak],
                vec!["crew".into()]
            ),
            ("cid".into(), vec![Power::Read, Power::Speak], vec![]),
        ]
    );
}

#[test]
fn a_change_another_passed_reads_the_board_again_and_loses_nothing() {
    let f = board();
    let other = f.other();
    let late = f.late(move || {
        other.add_person("bob", &[], &[Power::Speak], BY).unwrap();
    });
    let line = late.add_person("ana", &[], &[Power::Decide], BY).unwrap();
    assert_eq!(line.n, 2, "numbered after the change that passed it");
    assert_eq!(
        logged(&f.store),
        [
            (1, Did::Add, "bob".to_string()),
            (2, Did::Add, "ana".to_string())
        ]
    );
}

#[test]
fn changes_made_at_once_each_take_a_number_of_their_own() {
    let f = board();
    let stores: Vec<Store> = (0..6).map(|_| f.other()).collect();
    std::thread::scope(|scope| {
        for (at, store) in stores.iter().enumerate() {
            scope.spawn(move || {
                let name = format!("p{}", char::from(b'a' + u8::try_from(at).unwrap()));
                store.add_person(&name, &[], &[Power::Read], BY).unwrap();
            });
        }
    });
    let log = logged(&f.store);
    assert_eq!(
        (log.iter().map(|(n, ..)| *n)).collect::<Vec<_>>(),
        (1..=6).collect::<Vec<_>>()
    );
    assert_eq!(f.store.people().unwrap().members.len(), 6);
}

/// Every handle the first of its kind: the board must number it once it is taken.
struct First;

impl Pick for First {
    fn index(&mut self, _len: usize) -> usize {
        0
    }

    fn number(&mut self, from: i64, _to: i64) -> i64 {
        from
    }
}

/// A name a register can give is no person's or group's, so a register and an add of it at the same
/// instant never both take it; and a person's or a group's row under one (written before the rule,
/// or a pair a later word list makes) still keeps a register off it.
#[test]
fn no_person_is_named_as_a_handle_and_no_session_is_given_a_persons_or_a_groups_name() {
    let first = Fixture::empty();
    let plain = (first.other().picking(Box::new(First)))
        .register(&NewSession::new("worker"))
        .unwrap()
        .row
        .handle;
    let f = Fixture::empty();
    for why in [
        refused(f.store.add_person(&plain, &[], &[Power::Read], BY)),
        refused(f.store.add_group(&plain, &[], BY)),
    ] {
        assert!(
            why.starts_with(&format!("{plain} is a name a session's handle can take")),
            "{why}"
        );
    }
    assert_eq!(f.store.people().unwrap(), People::default());
    for table in ["people", "people_group"] {
        let f = Fixture::empty();
        (f.store.raw().row(table, serde_json::json!(plain)))
            .create(serde_json::json!({"name": plain}))
            .unwrap();
        let handle = (f.other().picking(Box::new(First)))
            .register(&NewSession::new("worker"))
            .unwrap()
            .row
            .handle;
        assert_ne!(handle, plain, "{table}");
        assert!(
            handle.starts_with(&format!("{plain}-")),
            "{table}: {handle}"
        );
    }
}

#[test]
fn a_person_who_holds_decide_is_granted_the_workflows_owner_and_the_owner_reads_no_people() {
    let f = board();
    f.store.add_group("staff", &[Power::Decide], BY).unwrap();
    f.store
        .add_person("ana", &["staff".into()], &[], BY)
        .unwrap();
    f.store.add_person("bob", &[], &[Power::Speak], BY).unwrap();
    let ana = f.store.host_for(&person("ana")).unwrap();
    assert!(ana.grants.holds(&person("ana"), "owner"));
    assert!(!ana.grants.holds(&person("bob"), "owner"));
    assert!(ana.grants.holds(&local_owner(), "owner"));
    let bob = f.store.host_for(&person("bob")).unwrap();
    assert_eq!(bob, ana, "one host for every person: the board's");
    assert_eq!(f.store.host_for(&local_owner()).unwrap(), Host::local());
}

#[test]
fn a_copy_carries_who_is_on_the_board() {
    let f = board();
    f.store.add_group("staff", &[Power::Speak], BY).unwrap();
    f.store
        .add_person("ana", &["staff".into()], &[], BY)
        .unwrap();
    let carried = f.store.carried().unwrap();
    let ids = |table: &str| -> Vec<String> {
        (carried[table].as_array().unwrap().iter())
            .map(|row| row["id"].to_string())
            .collect()
    };
    assert_eq!(ids("people"), ["\"ana\""]);
    assert_eq!(ids("people_group"), ["\"staff\""]);
    assert_eq!(ids("people_log"), ["1", "2"]);
}

#[test]
fn a_board_made_without_the_step_reads_nobody_and_takes_no_change() {
    let db = database();
    let step = ADDED.iter().position(|step| *step == PEOPLE).unwrap();
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
    let additions = writer.additions().unwrap();
    assert_eq!(additions, i64::try_from(step).unwrap());
    for store in [&writer, &open(&Opening::read())] {
        let people = store.people().unwrap();
        assert_eq!((people.members.len(), people.revision), (0, 0));
        assert_eq!(store.people_log().unwrap(), []);
        assert!(store.deciders().is_empty());
        let why = refused(store.add_person("ana", &[], &[Power::Decide], BY));
        assert!(why.starts_with("people are not on this board yet"), "{why}");
        assert!(why.ends_with("nothing was written"), "{why}");
        assert_eq!(store.carried().unwrap()["people"], serde_json::json!([]));
    }
    // A session still registers, its handle read past the tables the board lacks, and a task's
    // stamp reads, naming no decider.
    let worker = writer.register(&NewSession::new("worker")).unwrap();
    writer
        .create(&NewTask::new("sim/look", "Look", &worker.row.handle))
        .unwrap();
    let stamp = writer.task_stamp("sim/look").unwrap().unwrap();
    assert!(stamp.deciders().is_empty());
    assert_eq!(writer.additions().unwrap(), additions);
}
