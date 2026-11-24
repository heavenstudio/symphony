//! The actor seam (`symphony_plugin::Actor`, docs/product/analysis/users-and-identity.md, "The way
//! from one owner"): every save takes who asked. The local owner writes `owner` as it always has; a
//! test host's other actor is seen on a verdict, a card's message, a board move, a post and the
//! submission a board action's plugin receives, while a read reaches the plugin with no one; and
//! no save path is left that writes the constant where an actor is in hand.
//!
//! The board takes a person the saves vouch for by their short name
//! ([`symphony_board::Store::for_person`]), marking each line of theirs a person's, and refuses
//! a name one of its sessions holds as its handle: the other actor here is `ana`, whom no
//! session's handle names.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::json;
use symphony_board::workflow::local_actor;
use symphony_plugin::execution::{Principal, PrincipalKind};
use symphony_plugin::vocabulary::TaskOffer;
use symphony_plugin::{
    Actor, BoardAction, BoardField, FieldKind, Plugin, Sender, Submission, Submitted, TaskRef,
};

use super::*;
use crate::plugins::Plugins;
use crate::taskview::page_tests::store_fixture::Fixture;
use crate::test_tree::Tree;

const PAGE: &str =
    "# Topic\n\n<!-- register:start -->\n- [ ] **one** — proposal.\n<!-- register:end -->\n";

/// A save's answer, or the test's failure saying why it was refused.
fn ok(saved: Result<Value, Failure>) -> Value {
    saved.unwrap_or_else(|failure| panic!("refused ({}): {}", failure.status, failure.error))
}

/// A test host's person: `name` on their lines, verified by an issuer of the host's own.
fn ana(name: &str) -> Actor {
    Actor::new(
        Sender::new(name, "Ana Lima", vec!["ana-l".into(), "al".into()]),
        Principal::new(PrincipalKind::Human, "test-host".into(), "u-ana".into()),
    )
}

/// What [`Signing`] was asked, by the checkout it was asked in: `offer SLUG` for a read, `submit
/// SLUG from NAME (DISPLAY, GIT)` for a submission.
static ASKED: Mutex<Vec<(PathBuf, String)>> = Mutex::new(Vec::new());

fn asked_in(root: &Path) -> Vec<String> {
    let asked = ASKED.lock().unwrap();
    (asked.iter())
        .filter(|(at, _)| at == root)
        .map(|(_, call)| call.clone())
        .collect()
}

/// A board action that writes for whoever sent its submission, and releases `sim/held`; and a
/// board field, `effort`.
struct Signing;

static SIGNING: Signing = Signing;

impl Plugin for Signing {
    fn name(&self) -> &str {
        "signing"
    }
    fn board_actions(&self) -> Vec<&dyn BoardAction> {
        vec![&SIGNING]
    }
    fn board_fields(&self) -> Vec<&dyn BoardField> {
        vec![&SIGNING]
    }
}

impl BoardField for Signing {
    fn name(&self) -> &str {
        "effort"
    }
    fn kind(&self) -> FieldKind {
        FieldKind::Number
    }
}

impl BoardAction for Signing {
    fn name(&self) -> &str {
        "sign"
    }
    fn offer(&self, root: &Path, task: &TaskRef<'_>) -> Result<TaskOffer, String> {
        ASKED
            .lock()
            .unwrap()
            .push((root.into(), format!("offer {}", task.slug)));
        Ok(TaskOffer {
            action: "sign".into(),
            label: "Sign".into(),
            branch: task.branch.into(),
            payload: Some("tip".into()),
        })
    }
    fn submit(&self, _: &Path, _: &TaskRef<'_>, _: &Submission) -> Result<Submitted, String> {
        Err("the host hands every submission its sender".into())
    }
    fn submit_from(
        &self,
        root: &Path,
        task: &TaskRef<'_>,
        submission: &Submission,
        sender: &Sender,
    ) -> Result<Submitted, String> {
        let call = format!(
            "submit {} from {} ({}, {})",
            task.slug,
            sender.name,
            sender.display,
            sender.git.join(" ")
        );
        ASKED.lock().unwrap().push((root.into(), call));
        Ok(Submitted {
            said: format!("signed {} for {}", submission.payload, sender.display),
            released: vec!["sim/held".into()],
        })
    }
}

/// The thread of `slug`: each line's sender and words.
fn thread(f: &Fixture, slug: &str) -> Vec<(String, String)> {
    (f.store.thread(slug).unwrap().into_iter())
        .map(|m| (m.sender, m.text))
        .collect()
}

/// The senders of `slug`'s lines the board marks a person's a board named.
fn marked(f: &Fixture, slug: &str) -> Vec<String> {
    (f.store.thread_rows(slug).unwrap().into_iter())
        .filter(|row| row.person)
        .map(|row| row.sender)
        .collect()
}

/// The board as the saves reach it for `actor`, who asked: vouched for by their short name, as
/// [`Saves::store_by`] reaches it.
fn board_for(f: &Fixture, actor: &Actor) -> symphony_board::Store {
    f.other().for_person(actor.name())
}

/// Each of a task's writes asked by `actor` on a board holding `sim/a` and `sim/held`, both ready:
/// what the project's board action was asked.
fn on_the_board(f: &Fixture, actor: &Actor, root: &Path) -> Vec<String> {
    let plugins = || Plugins::new(vec![&SIGNING]);
    let store = board_for(f, actor);
    let write =
        |route: &str, payload: Value| ok(on_task(&store, plugins, root, actor, route, &payload));
    write(
        "/api/task-message",
        json!({"slug": "sim/a", "text": "Ready when you are"}),
    );
    write(
        "/api/task-priority",
        json!({"slug": "sim/a", "priority": 0}),
    );
    // A read: the offer, asked of the plugin with no one.
    crate::taskview::owner_offer(&f.store, &plugins(), "sim/a", root).unwrap();
    let said = write(
        symphony_api::TASK_ACTION_ROUTE,
        json!({"slug": "sim/a", "action": "sign", "payload": "tip", "note": ""}),
    );
    let signed = format!("signed tip for {}", actor.sender.display);
    assert_eq!(said["said"], signed.as_str());
    write("/api/task-cancel", json!({"slug": "sim/a"}));
    asked_in(root)
}

fn seeded(f: &Fixture, by: &str) {
    for (slug, status) in [("sim/a", "ready"), ("sim/held", "ready")] {
        f.task_with(symphony_board::NewTask {
            status: status.into(),
            ..symphony_board::NewTask::new(slug, "Jump departure", by)
        });
    }
}

#[test]
fn a_person_a_host_admitted_is_the_sender_and_mover_the_board_records_and_the_plugins_sender() {
    let (f, tree) = (Fixture::empty(), Tree::new());
    seeded(&f, "owner");
    // Work an executor holds blocked, which the action releases back to review.
    let worker = f.session("worker");
    f.store.claim("sim/held", &worker).unwrap();
    f.store.hold_blocked("sim/held", &worker).unwrap();
    let asked = on_the_board(&f, &ana("ana"), &tree.root);
    assert_eq!(
        asked,
        ["offer sim/a", "submit sim/a from ana (Ana Lima, ana-l al)"],
        "the read carries nobody, the submission its sender"
    );
    let lines = thread(&f, "sim/a");
    let by_ana: Vec<&str> = (lines.iter())
        .filter(|(sender, _)| sender == "ana")
        .map(|(_, text)| text.as_str())
        .collect();
    assert!(by_ana.contains(&"Ready when you are"), "{lines:?}");
    assert!(by_ana.contains(&"signed tip for Ana Lima"), "{lines:?}");
    assert!(
        (lines.iter().skip(1)).all(|(sender, _)| sender != "owner"),
        "no line but the seed's filing is the owner's: {lines:?}"
    );
    // Every line of hers is marked a person's, and no other line is.
    let marks = marked(&f, "sim/a");
    assert!(
        !marks.is_empty() && marks.iter().all(|sender| sender == "ana"),
        "{marks:?}"
    );
    assert_eq!(marks.len(), by_ana.len(), "{lines:?}");
    let task = f.store.get("sim/a").unwrap();
    assert_eq!((task.status.as_str(), task.priority), ("cancelled", 0));
    assert!(
        (lines.iter()).any(|(_, text)| text.contains("ana") && text.contains("cancelled")),
        "the cancel is the actor's: {lines:?}"
    );
    // Released work is moved back to review as the actor: its thread names her as the mover.
    assert_eq!(f.store.get("sim/held").unwrap().status, "review");
    let moved = (
        "ana".to_string(),
        "ana moved it blocked → review".to_string(),
    );
    assert!(
        thread(&f, "sim/held").contains(&moved),
        "{:?}",
        thread(&f, "sim/held")
    );
    assert_eq!(marked(&f, "sim/held"), ["ana"]);
}

#[test]
fn the_local_owner_writes_every_board_line_as_owner_as_before() {
    let (f, tree) = (Fixture::empty(), Tree::new());
    seeded(&f, "owner");
    // Work its executor holds blocked, which the owner's move sends back to review.
    let worker = f.session("worker");
    f.store.claim("sim/held", &worker).unwrap();
    f.store.hold_blocked("sim/held", &worker).unwrap();
    let asked = on_the_board(&f, &local_actor(), &tree.root);
    assert_eq!(asked, ["offer sim/a", "submit sim/a from owner (owner, )"]);
    let lines = thread(&f, "sim/a");
    for said in ["Ready when you are", "signed tip for owner"] {
        assert!(
            lines.contains(&("owner".to_string(), said.to_string())),
            "{said}: {lines:?}"
        );
    }
    assert_eq!(f.store.get("sim/a").unwrap().status, "cancelled");
    assert_eq!(f.store.get("sim/held").unwrap().status, "review");
}

#[test]
fn a_board_actions_note_its_own_approve_and_a_field_are_the_actors_and_an_unknown_action_none() {
    let (f, tree) = (Fixture::empty(), Tree::new());
    seeded(&f, "owner");
    let actor = ana("ana");
    let store = board_for(&f, &actor);
    let write = |route: &str, payload: Value| {
        on_task(
            &store,
            || Plugins::new(vec![&SIGNING]),
            &tree.root,
            &actor,
            route,
            &payload,
        )
    };
    let act = |action: &str, payload: &str, note: &str| {
        let asked = json!({"slug": "sim/a", "action": action, "payload": payload, "note": note});
        write(symphony_api::TASK_ACTION_ROUTE, asked)
    };
    // With a note, the line waits on an agent: it opens the task's ask, as the actor.
    ok(act("sign", "tip", "check the dock"));
    // The work it releases is ready, which no move takes to review: the board refuses the move,
    // and the save still answers.
    assert_eq!(f.store.get("sim/held").unwrap().status, "ready");
    // The Symphony's own approve, where no plugin's action has its name.
    let approve = crate::gates::stamps::OWNER_APPROVE;
    ok(act(approve, "c0ffee1", ""));
    let refused = act(approve, "not a commit", "").unwrap_err();
    assert_eq!(refused.status, 400, "{}", refused.error);
    let refused = act("nobody's", "", "").unwrap_err();
    assert_eq!(
        (refused.status, refused.error.as_str()),
        (404, "no such board action")
    );
    // A page's field write is the actor's; an agent's own command names its session.
    ok(write(
        "/api/task-field",
        json!({"slug": "sim/a", "name": "effort", "value": 3}),
    ));
    let other = f.session("worker");
    let named = json!({"slug": "sim/a", "name": "effort", "value": 5, "actor": other});
    ok(write("/api/task-field", named));
    let unknown = json!({"slug": "sim/a", "name": "nothing", "value": 1});
    let refused = write("/api/task-field", unknown).unwrap_err();
    assert_eq!(
        refused.error,
        "no plugin of this project declares a board field \"nothing\""
    );
    let lines = thread(&f, "sim/a");
    let said = |by: &str, text: &str| (lines.iter()).any(|(s, t)| s == by && t.contains(text));
    assert!(said("ana", "signed tip for Ana Lima"), "{lines:?}");
    assert!(said("ana", "approve c0ffee1"), "{lines:?}");
    let set = |by: &str, value: &str| said(by, &format!("{by} set signing.effort to {value}"));
    assert!(set("ana", "3.0"), "{lines:?}");
    assert!(set(&other, "5.0"), "{lines:?}");
    // The session's own field write is its own, never marked a person's.
    assert!(marked(&f, "sim/a").iter().all(|sender| sender == "ana"));
    assert!(
        (lines.iter().skip(1)).all(|(sender, _)| sender != "owner"),
        "no line but the seed's filing is the owner's: {lines:?}"
    );
    assert_eq!(
        lines[0],
        ("owner".to_string(), "created by owner".to_string())
    );
}

#[test]
fn a_name_no_person_may_take_is_refused_and_nothing_is_written() {
    let (f, tree) = (Fixture::empty(), Tree::new());
    seeded(&f, "owner");
    let before = thread(&f, "sim/a");
    let say = |store: &symphony_board::Store, actor: &Actor| {
        on_task(
            store,
            || Plugins::new(Vec::new()),
            &tree.root,
            actor,
            "/api/task-message",
            &json!({"slug": "sim/a", "text": "hello"}),
        )
        .unwrap_err()
    };
    // A person the saves did not vouch for is no session of the board's.
    let refused = say(&f.store, &ana("ana"));
    assert!(
        refused.error.contains(&f.no_session("ana")),
        "{}",
        refused.error
    );
    // A word that is no short name is never vouched for: it stays a session's, which none is.
    let capital = ana("Ana");
    let refused = say(&board_for(&f, &capital), &capital);
    assert!(
        refused.error.contains(&f.no_session("Ana")),
        "{}",
        refused.error
    );
    // A name one of the board's sessions holds as its handle never writes as a person, nor as
    // that session.
    let handle = f.session("worker");
    assert!(
        symphony_docs::syntax::is_short_name(&handle),
        "{handle} could be a person's name"
    );
    let named = ana(&handle);
    let refused = say(&board_for(&f, &named), &named);
    assert!(refused.error.contains(&handle), "{}", refused.error);
    assert_eq!(thread(&f, "sim/a"), before);
}

/// A checkout holding one page with one card, its saves.
fn page() -> (Tree, Saves) {
    let tree = Tree::new();
    tree.write("docs/TOPIC.md", PAGE);
    let saves = Saves::start(&tree.root);
    (tree, saves)
}

#[test]
fn a_verdict_and_a_message_carry_the_actors_name_and_the_local_owners_reads_owner() {
    for (actor, by) in [(ana("ana"), "ana"), (local_actor(), "owner")] {
        let (tree, saves) = page();
        let decide = json!({"path": "docs/TOPIC.md", "slug": "one", "state": "question", "note": "Which one?", "tap": by});
        ok(saves.post_as(&actor, "", "/api/decision", &decide));
        let message = json!({"path": "docs/TOPIC.md", "slug": "one", "text": "And why?"});
        ok(saves.post_as(&actor, "", "/api/message", &message));
        let text = std::fs::read_to_string(tree.root.join("docs/TOPIC.md")).unwrap();
        let lines: Vec<&str> = text.lines().filter(|l| l.starts_with("  [")).collect();
        assert_eq!(lines.len(), 2, "{text}");
        assert!(
            lines[0].ends_with(&format!("] {by} · question — Which one?")),
            "{text}"
        );
        assert!(lines[1].ends_with(&format!("] {by} — And why?")), "{text}");
        saves.close();
    }
}

#[test]
fn a_verdict_sent_again_is_a_repeat_only_of_the_same_persons() {
    let (tree, saves) = page();
    let decide = json!({"path": "docs/TOPIC.md", "slug": "one", "state": "rejected", "note": "No"});
    ok(saves.post_as(&ana("ana"), "", "/api/decision", &decide));
    let again = ok(saves.post_as(&ana("ana"), "", "/api/decision", &decide));
    assert_eq!(again["repeat"], true, "the same person's tap again");
    let other = ok(saves.post_as(&local_actor(), "", "/api/decision", &decide));
    assert_eq!(
        other.get("repeat"),
        None,
        "another person's same words are theirs"
    );
    let text = std::fs::read_to_string(tree.root.join("docs/TOPIC.md")).unwrap();
    assert!(text.contains("] ana · rejected — No"), "{text}");
    assert!(text.contains("] owner · rejected — No"), "{text}");
    saves.close();
}

#[test]
fn the_local_owners_entry_points_are_the_local_actors() {
    let (tree, saves) = page();
    let decide =
        json!({"path": "docs/TOPIC.md", "slug": "one", "state": "approved", "note": "Yes"});
    let order = crate::postpone::View::default();
    ok(saves.post_with_order("", "/api/decision", &decide, &order));
    let text = std::fs::read_to_string(tree.root.join("docs/TOPIC.md")).unwrap();
    assert!(text.contains("] owner · approved — Yes"), "{text}");
    // A workflow edge asked with no slug is refused before the board is read, as before.
    let refused = saves.apply_workflow(&json!({"request": {}})).unwrap_err();
    assert_eq!(
        (refused.failure.status, refused.failure.error.as_str()),
        (400, "slug is required")
    );
    saves.close();
}

#[test]
fn an_ask_and_its_follow_up_are_by_the_actor() {
    let (tree, saves) = page();
    let asked = json!({"path": "docs/TOPIC.md", "question": "What is this?", "anchor": "topic"});
    let id = ok(saves.post_as(&ana("ana"), "", "/api/ask", &asked))["id"]
        .as_i64()
        .unwrap();
    let follow = json!({"id": id, "text": "And this?"});
    ok(saves.post_as(&ana("ana"), "", "/api/ask-reply", &follow));
    let store = asks::AskStore::open(&asks::default_path(&tree.root, None)).unwrap();
    let thread = store.thread(id).unwrap();
    let by: Vec<(&str, &str)> = (thread.messages.iter())
        .map(|m| (m.by.as_str(), m.text.as_str()))
        .collect();
    assert_eq!(by, [("ana", "What is this?"), ("ana", "And this?")]);
    saves.close();
}

/// The saves' own code: where the local owner may still be named, and what may no longer stand
/// where an actor is in hand.
const CODE: [(&str, &str); 5] = [
    ("save.rs", include_str!("../save.rs")),
    ("save/quick.rs", include_str!("quick.rs")),
    ("save/settings.rs", include_str!("settings.rs")),
    ("save/attest.rs", include_str!("attest.rs")),
    (
        "save/quick_task/journal.rs",
        include_str!("quick_task/journal.rs"),
    ),
];

/// What stood for the actor before the seam: the constant, the local owner's principal, the
/// register's owner word and the writes that fix it.
const CONSTANT: [&str; 9] = [
    "\"owner\"",
    "local_owner()",
    "Who::Owner",
    ".say_asking(",
    "edit::set_decision(",
    "edit::add_message(",
    "edit::is_repeat_decision(",
    "store.ask(",
    "store.reply(",
];

/// `code` up to its inline test module, a `#[cfg(test)]` item that opens a block: its test
/// modules kept in files of their own (`#[cfg(test)] mod name;`) stand on a line each.
fn before_tests(code: &str) -> &str {
    let mut at = 0;
    while let Some(found) = code[at..].find("#[cfg(test)]") {
        let start = at + found;
        let item = code[start..]
            .lines()
            .skip(1)
            .find(|line| !line.trim_start().starts_with("#["));
        if item.is_some_and(|item| item.trim_end().ends_with('{')) {
            return &code[..start];
        }
        at = start + 1;
    }
    code
}

#[test]
fn no_save_path_writes_the_constant_where_an_actor_is_in_hand() {
    for (file, code) in CODE {
        let code = before_tests(code);
        assert!(code.lines().count() > 20, "{file} read whole");
        for (n, line) in code.lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            for constant in CONSTANT {
                assert!(
                    !line.contains(constant),
                    "{file}:{}: {constant} where the save's actor is in hand: {line}",
                    n + 1
                );
            }
        }
    }
    // The local owner is supplied in one place each: the entry points that are the local owner's,
    // and the journal's reading of a record from before records named who asked.
    let supplied: Vec<&str> = (CODE.iter())
        .flat_map(|(file, code)| {
            (before_tests(code).lines())
                .filter(|line| line.contains("local_actor()"))
                .map(move |_| *file)
        })
        .collect();
    assert_eq!(
        supplied,
        [
            "save.rs",
            "save.rs",
            "save.rs",
            "save/quick_task/journal.rs"
        ]
    );
}
