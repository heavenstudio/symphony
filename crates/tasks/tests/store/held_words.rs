//! Words held for people (`symphony_board::held`, `Store::say_or_hold_as`, `Store::pass`): the
//! message of a person who may speak and may not direct agents is a row no read of a thread reads,
//! and the board's line for it says only that it is held, to the owner. No agent is handed the
//! words by its inbox or a callback, and the line waits on the person at the machine until a
//! person's line follows it. Passing it on puts the words on the thread under their author's name,
//! the passer first, once: they reach the task's agents as a person's message does and open its
//! ask. A board without the step holds nothing and refuses a hold.

use super::callbacks::Listener;
use super::common::{Fixture, HOST, NOW, Seed, database, refused};
use super::dispatch_later::{ids, listen};
use symphony_board::held::{Held, MARKER, Said};
use symphony_board::people::ownership::Place;
use symphony_board::people::{Holder, Power};
use symphony_board::schema::{ADDED, HELD};
use symphony_board::stamps::read::reads_as_approve;
use symphony_board::{Opening, Schema, Store};

/// The words every case holds, which no agent's read may carry.
const WORDS: &str = "please add a picture of the wide page";

/// The session that adds people.
const BY: &str = "amber-heron";

/// A board where `tools/x` is claimed by a worker and orchestrated, with `ana`, who may speak and
/// not direct agents, and `stefano`, who may do both: the worker's and the orchestrator's handles.
struct Board {
    f: Fixture,
    worker: String,
    orchestrator: String,
}

fn board() -> Board {
    let f = Fixture::new(Seed::default().session(BY));
    let worker = f.session("worker");
    let orchestrator = f.session("orchestrator");
    f.task("tools/x");
    f.task("tools/y");
    f.store.claim("tools/x", &worker).unwrap();
    f.store
        .take_role("tools/x", &orchestrator, "orchestrator")
        .unwrap();
    f.store.add_person("ana", &[], &[Power::Speak], BY).unwrap();
    f.store
        .add_person("stefano", &[], &[Power::Speak, Power::DirectAgents], BY)
        .unwrap();
    Board {
        f,
        worker,
        orchestrator,
    }
}

impl Board {
    /// A store on the board writing for `name`, as the Symphony makes for a person it admitted.
    fn person(&self, name: &str) -> Store {
        self.f.other().for_person(name)
    }

    /// `ana`'s message on `tools/x`, held.
    fn held(&self, to: Option<&str>) -> Held {
        match self
            .person("ana")
            .say_or_hold_as("tools/x", "ana", WORDS, to)
        {
            Ok(Said::Held(held)) => held,
            other => panic!("not held: {other:?}"),
        }
    }

    /// Every text an agent can read of `slug`'s thread.
    fn thread(&self, slug: &str) -> Vec<String> {
        let thread = self.f.store.thread(slug).unwrap();
        thread.into_iter().map(|m| m.text).collect()
    }

    /// What `handle`'s inbox hands it.
    fn inbox(&self, handle: &str) -> Vec<String> {
        let inbox = self.f.store.inbox(handle).unwrap();
        inbox.into_iter().map(|m| m.text).collect()
    }

    /// A listener for `handle`'s callback.
    fn listening(&self, handle: &str) -> Listener {
        let database = self.f.store.database().database().to_string();
        listen(&self.f.store, handle, &format!("{database}-{handle}"))
    }

    /// The questions to the owner still waiting on `tools/x`.
    fn waiting(&self) -> Vec<String> {
        let open = ["tools/x".to_string()];
        let waiting = self.f.store.questions_waiting("owner", &open).unwrap();
        waiting.into_iter().map(|m| m.text).collect()
    }
}

#[test]
fn a_held_message_reaches_no_agent_and_the_boards_line_says_only_that_it_is_held() {
    let b = board();
    let to_worker = b.listening(&b.worker);
    let to_orchestrator = b.listening(&b.orchestrator);
    let _ = to_worker.drain(&b.f, BY, &b.worker);
    let _ = to_orchestrator.drain(&b.f, BY, &b.orchestrator);
    b.inbox(&b.worker);
    b.inbox(&b.orchestrator);

    let held = b.held(Some("@executor"));
    assert_eq!(
        (held.slug.as_str(), held.author.as_str(), held.text.as_str()),
        ("tools/x", "ana", WORDS)
    );
    assert_eq!(
        (held.to.as_deref(), held.passed.as_ref()),
        (Some("@executor"), None)
    );

    // The thread carries the board's line, numbered as the row is keyed, and nothing of it.
    let rows = b.f.store.thread_rows("tools/x").unwrap();
    let line = rows.last().unwrap();
    assert_eq!(line.id, held.id);
    assert_eq!(
        (
            line.sender.as_str(),
            line.recipient.as_deref(),
            line.kind.as_str()
        ),
        ("board", Some("owner"), "msg")
    );
    assert_eq!(line.text, format!("{MARKER}#{}", held.id));
    assert!(!line.person, "the board's line is no person's");
    let thread = b.thread("tools/x");
    assert!(
        thread
            .iter()
            .all(|text| !text.contains(WORDS) && !text.contains("ana")),
        "{thread:?}"
    );

    // No inbox and no callback hands an agent the words or the line.
    for handle in [&b.worker, &b.orchestrator] {
        assert_eq!(b.inbox(handle), Vec::<String>::new(), "{handle}");
    }
    for (listener, handle) in [(&to_worker, &b.worker), (&to_orchestrator, &b.orchestrator)] {
        let heard = listener.drain(&b.f, BY, handle);
        assert!(
            heard
                .iter()
                .all(|line| !line.contains(WORDS) && !line.contains(MARKER)),
            "{handle}: {heard:?}"
        );
        assert!(!ids(&heard).contains_key(&held.id), "{handle}: {heard:?}");
    }

    // An agent is told how many are held; people read them whole; the owner is asked.
    assert_eq!(b.f.store.held_count("tools/x").unwrap(), 1);
    assert_eq!(b.f.store.held_count("tools/y").unwrap(), 0);
    assert_eq!(
        b.f.store.held_on("tools/x").unwrap(),
        std::slice::from_ref(&held)
    );
    assert_eq!(b.waiting(), [format!("{MARKER}#{}", held.id)]);
    let told: Vec<i64> = (b.f.store.to_owner(10).unwrap().into_iter())
        .map(|m| m.id)
        .collect();
    assert!(told.contains(&held.id), "{told:?}");
    assert!(
        b.f.store.asks(None).unwrap().is_empty(),
        "a held message opens no ask"
    );
}

#[test]
fn passed_on_the_words_reach_the_tasks_agents_once_under_both_names_and_ask() {
    let b = board();
    let to_worker = b.listening(&b.worker);
    let _ = to_worker.drain(&b.f, BY, &b.worker);
    b.inbox(&b.worker);
    let held = b.held(None);

    let passed = b.f.store.pass("tools/x", held.id, &b.orchestrator).unwrap();
    let mark = passed.passed.clone().expect("marked passed");
    assert_eq!(mark.by, b.orchestrator);
    let text = format!("[passed on by {}] {WORDS}", b.orchestrator);

    // The line is its author's, a person's, the passer first.
    let rows = b.f.store.thread_rows("tools/x").unwrap();
    let line = rows.last().unwrap();
    assert_eq!(line.id, mark.line);
    assert_eq!(
        (
            line.sender.as_str(),
            line.person,
            line.text.as_str(),
            line.recipient.as_deref()
        ),
        ("ana", true, text.as_str(), None)
    );
    // It reaches the executor as a person's word does, once, by its callback.
    let heard = to_worker.drain(&b.f, BY, &b.worker);
    assert_eq!(ids(&heard).get(&mark.line), Some(&1), "{heard:?}");
    // It opens the task's ask, and answers the line that waited on the owner.
    let asks = b.f.store.asks(None).unwrap();
    let [ask] = asks.as_slice() else {
        panic!("one ask: {asks:?}")
    };
    assert!(ask.open && ask.words.contains(WORDS), "{ask:?}");
    assert_eq!(b.waiting(), Vec::<String>::new());
    assert_eq!(b.f.store.held_count("tools/x").unwrap(), 0);
    assert_eq!(b.f.store.held_on("tools/x").unwrap(), [passed]);

    // A second pass is refused, naming who passed it, and writes nothing.
    let before = b.f.store.thread_rows("tools/x").unwrap().len();
    let why = refused(b.f.store.pass("tools/x", held.id, &b.worker));
    assert!(
        why.contains(&format!("passed on already, by {}", b.orchestrator)),
        "{why}"
    );
    assert!(why.ends_with("nothing was written"), "{why}");
    assert_eq!(b.f.store.thread_rows("tools/x").unwrap().len(), before);
    let again = to_worker.drain(&b.f, BY, &b.worker);
    assert!(!ids(&again).contains_key(&mark.line), "{again:?}");
}

#[test]
fn a_person_who_may_direct_agents_passes_on_and_a_reply_after_the_line_answers_it() {
    let b = board();
    b.inbox(&b.worker);
    let held = b.held(Some(&b.worker.clone()));
    // An inbox no callback reads hands the executor nothing of it, though it was written to it.
    assert_eq!(b.inbox(&b.worker), Vec::<String>::new());
    let stefano = b.person("stefano");
    let passed = stefano.pass("tools/x", held.id, "stefano").unwrap();
    let mark = passed.passed.expect("marked passed");
    assert_eq!(mark.by, "stefano");
    let rows = b.f.store.thread_rows("tools/x").unwrap();
    let line = rows.last().unwrap();
    assert_eq!(
        (line.sender.as_str(), line.person, line.recipient.as_deref()),
        ("ana", true, Some(b.worker.as_str()))
    );
    assert_eq!(line.text, format!("[passed on by stefano] {WORDS}"));
    assert_eq!(b.inbox(&b.worker), std::slice::from_ref(&line.text));

    // A line still held waits on the owner until a person's line follows it.
    let other = b.held(None);
    assert_eq!(b.waiting(), [format!("{MARKER}#{}", other.id)]);
    stefano
        .say_or_hold_as("tools/x", "stefano", "I will look at it", None)
        .unwrap();
    assert_eq!(b.waiting(), Vec::<String>::new());
    assert_eq!(b.f.store.held_count("tools/x").unwrap(), 1, "still held");
}

#[test]
fn who_may_direct_agents_writes_as_the_owner_does_and_only_a_persons_words_are_held() {
    let b = board();
    for (store, name) in [(b.person("stefano"), "stefano"), (b.f.other(), "owner")] {
        assert!(store.may_direct_agents(name).unwrap(), "{name}");
        match store
            .say_or_hold_as("tools/x", name, "go on", None)
            .unwrap()
        {
            Said::Line(line) => assert_eq!(line.sender, name),
            held => panic!("{name}'s words held: {held:?}"),
        }
    }
    let ana = b.person("ana");
    assert!(!ana.may_direct_agents("ana").unwrap());
    // A session's own words are no person's, and a store not made for a person holds none.
    assert!(b.f.store.may_direct_agents(&b.worker).unwrap());
    for (store, author) in [(b.f.other(), "owner"), (b.f.other(), "ana"), (ana, "owner")] {
        let why = refused(store.hold_words("tools/x", author, WORDS, None));
        assert!(why.starts_with("only a person's words are held"), "{why}");
    }
    assert_eq!(b.f.store.held_count("tools/x").unwrap(), 0);
}

#[test]
fn one_who_may_direct_agents_and_not_decide_the_task_writes_no_line_that_reads_as_an_approve() {
    let b = board();
    let stefano = b.person("stefano");
    let before = b.f.store.thread_rows("tools/x").unwrap().len();
    for text in ["approve 1a2b3c4", "  Approve: 1a2b3c4"] {
        let why = refused(stefano.say_or_hold_as("tools/x", "stefano", text, None));
        assert_eq!(
            why,
            "stefano may not decide on this board, and a line of theirs that starts with approve \
             reads as one: nothing was written"
        );
    }
    assert_eq!(b.f.store.thread_rows("tools/x").unwrap().len(), before);
    // A word that only begins so is no approve.
    let said = stefano.say_or_hold_as("tools/x", "stefano", "approved? not yet", None);
    assert!(matches!(said, Ok(Said::Line(_))), "{said:?}");
    // With decide given, an approve is theirs to say once the task is theirs to decide: not while
    // the board maps no group to it, nor while it is a group's they are no member of.
    (b.f.store)
        .grant(Holder::Person("stefano"), &[Power::Decide], BY)
        .unwrap();
    let approve = || stefano.say_or_hold_as("tools/x", "stefano", "approve 1a2b3c4", None);
    assert_eq!(
        refused(approve()),
        "it is the person at the machine's, as this board maps no group it keeps to the area tools \
         or to the board's default, so stefano may not decide it until an admin maps one (just \
         tasks-people map PLACE GROUP), and a line of theirs that starts with approve reads as one: \
         nothing was written"
    );
    b.f.store.add_group("design", &[], BY).unwrap();
    (b.f.store)
        .map_place(&Place::Area("tools".into()), "design", BY)
        .unwrap();
    assert_eq!(
        refused(approve()),
        "stefano is no member of the group design, whose it is, so may not decide it, and a line \
         of theirs that starts with approve reads as one: nothing was written"
    );
    assert_eq!(b.f.store.thread_rows("tools/x").unwrap().len(), before + 1);
    b.f.store.join_group("stefano", "design", BY).unwrap();
    let said = approve();
    assert!(matches!(said, Ok(Said::Line(_))), "{said:?}");
    // A task no board has is refused as no task, whatever the line says.
    let missing = refused(stefano.say_or_hold_as("tools/gone", "stefano", "approve 1a2b3c4", None));
    assert!(missing.contains("tools/gone"), "{missing}");
    // Held words that start so, passed on, read as the passer's first: never an approve.
    let ana = b.person("ana");
    let Ok(Said::Held(held)) = ana.say_or_hold_as("tools/x", "ana", "approve 1a2b3c4", None) else {
        panic!("not held")
    };
    stefano.pass("tools/x", held.id, "stefano").unwrap();
    let line = b.f.store.thread_rows("tools/x").unwrap().pop().unwrap();
    assert_eq!(line.text, "[passed on by stefano] approve 1a2b3c4");
    assert!(!reads_as_approve(&line.text));
}

#[test]
fn a_hold_or_a_pass_that_cannot_be_made_writes_nothing() {
    let b = board();
    let ana = b.person("ana");
    let before = b.f.store.thread_rows("tools/x").unwrap().len();
    let refusals = [
        refused(ana.hold_words("tools/none", "ana", WORDS, None)),
        refused(ana.hold_words("tools/x", "ana", "  ", None)),
        refused(ana.hold_words("tools/x", "ana", WORDS, Some("@nobody"))),
        refused(ana.hold_words("tools/x", "ana", WORDS, Some("gone-heron-1"))),
    ];
    assert!(refusals[0].contains("tools/none"), "{}", refusals[0]);
    assert!(refusals[1].contains("an empty message says nothing"));
    assert!(
        refusals[2].starts_with("no role @nobody"),
        "{}",
        refusals[2]
    );
    assert!(
        refusals[3].contains("no session 'gone-heron-1'"),
        "{}",
        refusals[3]
    );
    assert_eq!(b.f.store.thread_rows("tools/x").unwrap().len(), before);
    assert_eq!(b.f.store.held_on("tools/x").unwrap(), []);

    let held = b.held(None);
    let rows = b.f.store.thread_rows("tools/x").unwrap().len();
    for (slug, id, by) in [
        ("tools/y", held.id, b.orchestrator.as_str()),
        ("tools/x", held.id + 1000, b.orchestrator.as_str()),
        ("tools/x", held.id, "nobody-heron-9"),
    ] {
        let why = refused(b.f.store.pass(slug, id, by));
        assert!(
            why.ends_with("nothing was written") || why.contains("nobody-heron-9"),
            "{why}"
        );
    }
    assert_eq!(b.f.store.thread_rows("tools/x").unwrap().len(), rows);
    assert_eq!(b.f.store.held_count("tools/x").unwrap(), 1);
}

#[test]
fn a_board_without_the_step_takes_no_hold_and_keeps_every_other_line() {
    let b = board();
    // Made before the board loses the step: a store that opens it now adds the step again.
    let ana = b.person("ana");
    let step = ADDED.iter().position(|step| *step == HELD).unwrap();
    (b.f.store.raw().row("meta", "additions"))
        .set(serde_json::json!({"version": step}))
        .unwrap();
    let lines = b.f.store.thread_rows("tools/x").unwrap().len();
    let why = refused(ana.say_or_hold_as("tools/x", "ana", WORDS, None));
    assert!(why.contains("this write names `held_message`"), "{why}");
    assert!(why.contains("nothing was written"), "{why}");
    assert_eq!(b.f.store.thread_rows("tools/x").unwrap().len(), lines);
    // Someone who may direct agents writes as before.
    let stefano = b.person("stefano");
    stefano
        .say_or_hold_as("tools/x", "stefano", "still here", None)
        .unwrap();
    assert_eq!(b.f.store.held_count("tools/x").unwrap(), 0);
}

/// A board made before the step, which no table of it was ever added to, as this build meets it
/// when it may not move the board: a writer of the contract before the step and every reader read
/// no held message, and an agent is told of none.
#[test]
fn a_board_made_without_the_step_reads_no_held_message() {
    let db = database();
    let step = ADDED.iter().position(|step| *step == HELD).unwrap();
    let step = i64::try_from(step).unwrap();
    let before = Opening::write().at(Schema::of_this_contract(step));
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
    let worker = writer
        .register(&symphony_board::NewSession::new("worker"))
        .unwrap()
        .row
        .handle;
    writer
        .create(&symphony_board::NewTask::new("sim/look", "Look", &worker))
        .unwrap();
    let reader = open(&Opening::read());
    assert_eq!(reader.additions().unwrap(), step, "nothing moved the board");
    for store in [&writer, &reader] {
        assert_eq!(store.held_on("sim/look").unwrap(), []);
        assert_eq!(store.held_count("sim/look").unwrap(), 0);
    }
}
