//! A person a board named writes as the owner does, under their own name (`Store::for_person`,
//! docs/product/analysis/users-and-identity.md): no session acts, and every line they make is marked
//! theirs (`MessageRow::person`), so their word reaches whom the owner's reaches, their line asks as
//! the owner's does, and their move reads `ana moved it …`. Only a store the Symphony makes for
//! them writes so: the same name from any other store is a session's, refused as an unknown one is,
//! and a name a session holds on the board is never a person's.

use super::common::{Fixture, HOST, NOW, Seed, database};
use symphony_board::rows::MessageRow;
use symphony_board::schema::{ADDED, PERSON_LINES};
use symphony_board::{Error, NewSession, NewTask, Opening, Schema, Store};

/// The person every case writes as.
const ANA: &str = "ana";

/// A board with `tools/x` on it, claimed by a worker: the worker's handle.
fn claimed(f: &Fixture) -> String {
    let worker = f.session("worker");
    f.task("tools/x");
    f.store.claim("tools/x", &worker).unwrap();
    worker
}

/// A store on `f`'s board writing for `name`.
fn person(f: &Fixture, name: &str) -> Store {
    f.other().for_person(name)
}

/// The newest line on `slug`'s thread, as the board holds it.
fn newest(f: &Fixture, slug: &str) -> MessageRow {
    let rows = f.store.thread_rows(slug).unwrap();
    rows.last().cloned().expect("a line")
}

/// The refusal `result` is, in its words.
fn refused<T: std::fmt::Debug>(result: Result<T, Error>) -> String {
    match result {
        Err(Error::Refused(why)) => why,
        other => panic!("not refused: {other:?}"),
    }
}

#[test]
fn a_persons_post_is_marked_theirs_asks_as_the_owners_does_and_reaches_the_executor() {
    let f = Fixture::new(Seed::default());
    let worker = claimed(&f);
    let ana = person(&f, ANA);
    assert!(ana.is_person(ANA) && ana.is_person("owner") && !ana.is_person(&worker));
    assert!(
        !f.store.is_person(ANA),
        "a store not made for her takes no person"
    );

    let asked = ana
        .say_asking_as("tools/x", ANA, "how far along?", None)
        .unwrap();
    assert_eq!(asked.sender, ANA);
    let line = newest(&f, "tools/x");
    assert!(line.person && line.by_person(), "{line:?}");
    let thread = f.store.thread_rows("tools/x").unwrap();
    let before = &thread[..thread.len() - 1];
    assert!(
        before.iter().all(|row| !row.person),
        "only her line is marked: {before:?}"
    );
    assert!(
        before.iter().any(MessageRow::by_person),
        "the owner's filing is a person's"
    );

    let asks = f.store.asks(None).unwrap();
    let [ask] = asks.as_slice() else {
        panic!("one ask: {asks:?}")
    };
    assert_eq!((ask.words.as_str(), ask.open), ("how far along?", true));
    // Her word reaches the executor as the owner's does, with no address.
    let heard: Vec<String> = (f.store.inbox(&worker).unwrap().into_iter())
        .map(|m| m.text)
        .collect();
    assert!(heard.contains(&"how far along?".to_string()), "{heard:?}");
    // The executor, holding a role there, answers her ask.
    let reply = f.store.say("tools/x", &worker, "halfway", None).unwrap();
    let answered = f.store.ask(ask.id).unwrap();
    assert_eq!(
        (answered.open, answered.answer),
        (false, Some(format!("#{}", reply.id)))
    );
    assert!(
        !newest(&f, "tools/x").person,
        "a session's line is never a person's"
    );
}

#[test]
fn a_persons_cancel_reads_their_name_and_tells_the_executor_it_moves_no_further() {
    let f = Fixture::new(Seed::default());
    let worker = claimed(&f);
    f.store.inbox(&worker).unwrap();
    let ana = person(&f, ANA);
    let moved = ana
        .move_with("tools/x", "cancelled", ANA, Some("not needed now"))
        .unwrap();
    assert_eq!(moved.status, "cancelled");
    let rows = f.store.thread_rows("tools/x").unwrap();
    let line = rows
        .iter()
        .find(|row| row.kind == "system" && row.text.starts_with("ana moved it"));
    let line = line.unwrap_or_else(|| panic!("her move: {rows:?}"));
    assert_eq!((line.sender.as_str(), line.person), (ANA, true));
    assert_eq!(line.text, "ana moved it in_progress → cancelled");
    let reason = rows.last().unwrap();
    assert_eq!(
        (reason.sender.as_str(), reason.person),
        (ANA, true),
        "{reason:?}"
    );
    let heard: Vec<String> = (f.store.inbox(&worker).unwrap().into_iter())
        .map(|m| m.text)
        .collect();
    assert!(
        heard.contains(
            &"ana moved it in_progress → cancelled; it moves no further: stop work on it".into()
        ),
        "{heard:?}"
    );
}

#[test]
fn a_person_files_a_task_by_a_receipt_only_their_store_finds_and_sets_its_priority() {
    let f = Fixture::new(Seed::default());
    let ana = person(&f, ANA);
    let filed = ana
        .create_owner_task(
            &NewTask::new("tools/filed", "Filed by a person", ANA),
            "receipt-ana",
        )
        .unwrap();
    assert_eq!(filed.created_by, ANA);
    assert!(
        f.store
            .thread_rows("tools/filed")
            .unwrap()
            .iter()
            .any(|row| row.sender == ANA && row.person)
    );
    assert_eq!(ana.owner_saved("receipt-ana").unwrap(), ["tools/filed"]);
    assert_eq!(
        f.store.owner_saved("receipt-ana").unwrap(),
        Vec::<String>::new(),
        "the owner's saves are hers alone"
    );
    assert_eq!(
        person(&f, "bea").owner_saved("receipt-ana").unwrap(),
        Vec::<String>::new()
    );
    // A session files no owner's task, and neither does a name the store is not made for.
    let refusal =
        refused(ana.create_owner_task(&NewTask::new("tools/bea", "Bea's", "bea"), "receipt-bea"));
    assert!(refusal.contains("requires actor owner"), "{refusal}");

    let set = ana.set_priority("tools/filed", 1, ANA).unwrap();
    assert_eq!(set.priority, 1);
    let line = newest(&f, "tools/filed");
    assert!(
        line.sender == ANA && line.person && line.text.starts_with(ANA),
        "{line:?}"
    );
}

#[test]
fn a_name_no_store_vouches_for_is_a_sessions_and_refused_as_an_unknown_one() {
    let f = Fixture::new(Seed::default());
    claimed(&f);
    let no_session = |name: &str| f.no_session(name);
    // Her name from a store not made for her, another name from hers, and a name that is no short
    // name from a store made for it: each a session's, none known.
    for (store, name) in [
        (f.other(), ANA),
        (person(&f, ANA), "bea"),
        (person(&f, "Ana Maria"), "Ana Maria"),
        (person(&f, "amber-heron-12"), "amber-heron-12"),
    ] {
        assert!(!store.is_person(name), "{name}");
        assert_eq!(
            refused(store.say("tools/x", name, "hello", None)),
            no_session(name),
            "{name}"
        );
        assert_eq!(
            refused(store.set_priority("tools/x", 2, name)),
            no_session(name),
            "{name}"
        );
    }
    assert!(
        f.store
            .thread_rows("tools/x")
            .unwrap()
            .iter()
            .all(|row| !row.person)
    );
}

#[test]
fn a_name_a_session_holds_live_or_left_is_never_a_persons() {
    let f = Fixture::new(Seed::default());
    let worker = claimed(&f);
    let other = f.session("worker");
    let lines = f.store.thread_rows("tools/x").unwrap().len();
    for handle in [&worker, &other] {
        let as_her = person(&f, handle);
        let why = refused(as_her.say("tools/x", handle, "as a person", None));
        assert!(
            why.contains(&format!(
                "{handle} is a session's handle on this board, so it cannot write as a person"
            )),
            "{why}"
        );
        let why = refused(as_her.set_priority("tools/x", 3, handle));
        assert!(why.contains("cannot write as a person"), "{why}");
    }
    f.store.leave(&other).unwrap();
    let why = refused(person(&f, &other).say("tools/x", &other, "left", None));
    assert!(
        why.contains("cannot write as a person"),
        "a left session's handle too: {why}"
    );
    assert_eq!(
        f.store.thread_rows("tools/x").unwrap().len(),
        lines,
        "nothing was written"
    );
}

#[test]
fn a_board_without_the_step_takes_no_persons_write_and_every_other() {
    let f = Fixture::new(Seed::default());
    claimed(&f);
    // Made before the board loses the step: a store that opens it now adds the step again.
    let ana = person(&f, ANA);
    let step = ADDED.iter().position(|step| *step == PERSON_LINES).unwrap();
    (f.store.raw().row("meta", "additions"))
        .set(serde_json::json!({"version": step}))
        .unwrap();
    let why = refused(ana.say("tools/x", ANA, "hello", None));
    assert!(why.contains("this write names `message.person`"), "{why}");
    f.store.say("tools/x", "owner", "still here", None).unwrap();
    assert_eq!(newest(&f, "tools/x").text, "still here");
}

/// A board made before the step, which no column of `message.person` was ever added to, as a build
/// of this code meets it when it may not move the board: a writer of the contract before the step
/// and every reader. Each line reads as no person's, the owner's as the owner's, and every read of
/// the thread takes it: the window the task page draws, the rows, the feed, the inbox and the
/// questions waiting on the owner.
#[test]
fn a_board_made_without_the_step_reads_every_line_and_takes_a_sessions_post() {
    let db = database();
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
    let step = ADDED.iter().position(|step| *step == PERSON_LINES).unwrap();
    let step = i64::try_from(step).unwrap();
    let before = Opening::write().at(Schema::of_this_contract(step));
    let writer = open(&before);
    let worker = writer
        .register(&NewSession::new("worker"))
        .unwrap()
        .row
        .handle;
    writer
        .create(&NewTask::new("tools/x", "Jump departure", "owner"))
        .unwrap();
    writer.claim("tools/x", &worker).unwrap();
    let asked = writer
        .say_asking_as("tools/x", "owner", "how far along?", None)
        .unwrap();
    let heard: Vec<String> = writer
        .inbox(&worker)
        .unwrap()
        .into_iter()
        .map(|m| m.text)
        .collect();
    assert!(heard.contains(&"how far along?".to_string()), "{heard:?}");
    let answer = writer
        .say(
            "tools/x",
            &worker,
            "Does the larger font fit?",
            Some("owner"),
        )
        .unwrap();
    let why = refused(
        open(&before)
            .for_person(ANA)
            .say("tools/x", ANA, "hello", None),
    );
    assert!(why.contains("this write names `message.person`"), "{why}");

    let reader = open(&Opening::read());
    assert_eq!(reader.additions().unwrap(), step, "nothing moved the board");
    for store in [&writer, &reader] {
        let window = store.thread_window("tools/x", None).unwrap();
        assert!(window.by_person.is_empty(), "{:?}", window.by_person);
        let ids: Vec<i64> = window.lines.iter().map(|m| m.id).collect();
        assert!(ids.ends_with(&[asked.id, answer.id]), "{ids:?}");
        let rows = store.thread_rows("tools/x").unwrap();
        assert!(rows.iter().all(|row| !row.person), "{rows:?}");
        let owners: Vec<i64> = rows
            .iter()
            .filter(|row| row.by_person())
            .map(|row| row.id)
            .collect();
        assert!(
            owners.contains(&asked.id) && !owners.contains(&answer.id),
            "{owners:?}"
        );
        let feed = store.messages_after_rows(asked.id - 1, answer.id).unwrap();
        assert_eq!(
            feed.iter()
                .map(|row| (row.id, row.person))
                .collect::<Vec<_>>(),
            [(asked.id, false), (answer.id, false)]
        );
        let waiting = store
            .questions_waiting("owner", &["tools/x".to_string()])
            .unwrap();
        assert_eq!(
            waiting.iter().map(|m| m.id).collect::<Vec<_>>(),
            [answer.id]
        );
    }
}

/// The person at the machine under the name they gave themselves (`Store::for_machine`): their lines
/// are a person's, marked theirs and reaching whom the owner's reach, their move reads with their
/// name, and the task they file is theirs; a store not made for them takes the name as a session's.
#[test]
fn the_machines_person_under_their_name_writes_as_a_person_and_files_as_the_owner() {
    let f = Fixture::new(Seed::default());
    let worker = claimed(&f);
    f.store.inbox(&worker).unwrap();
    let me = f.other().for_machine("stefano");
    assert!(me.is_person("stefano") && me.is_person("owner") && !me.is_person(&worker));
    assert!(!f.store.is_person("stefano"));
    let said = me.say("tools/x", "stefano", "is it done?", None).unwrap();
    assert_eq!(said.sender, "stefano");
    let line = newest(&f, "tools/x");
    assert!(line.person && line.by_person(), "{line:?}");
    let moved = me.move_to("tools/x", "cancelled", "stefano").unwrap();
    assert_eq!(moved.status, "cancelled");
    let rows = f.store.thread_rows("tools/x").unwrap();
    assert!(
        rows.iter().any(|row| row.person
            && row.sender == "stefano"
            && row.text == "stefano moved it in_progress → cancelled"),
        "{rows:?}"
    );
    let heard: Vec<String> = (f.store.inbox(&worker).unwrap().into_iter())
        .map(|m| m.text)
        .collect();
    assert!(heard.contains(&"is it done?".to_string()), "{heard:?}");
    let filed = me
        .create_owner_task(
            &NewTask::new("tools/mine", "Filed under a name", "stefano"),
            "receipt-mine",
        )
        .unwrap();
    assert_eq!(filed.created_by, "stefano");
    assert_eq!(me.owner_saved("receipt-mine").unwrap(), ["tools/mine"]);
    // `owner` stays the owner's from that store too, and a store for a person a board named
    // takes nobody as the machine's.
    assert!(f.other().for_machine("owner").is_person("owner"));
    let refusal = refused(f.other().say("tools/x", "stefano", "hello", None));
    assert!(refusal.starts_with("no session 'stefano'"), "{refusal}");
}
