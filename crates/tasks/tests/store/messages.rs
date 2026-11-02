//! Threads: saying, inboxes and acknowledging them, what reaches the owner, each sender's last word,
//! and landings on the record.

use crate::common::{Fixture, HOST, NOW, refused};
use std::collections::BTreeSet;
use tasks::health::LIVE_SECONDS;
use tasks::rpc::Client;
use tasks::{Health, Message, NewTask, Store};

fn texts(messages: Vec<Message>) -> Vec<String> {
    messages.into_iter().map(|m| m.text).collect()
}

fn under(slug: &str, parent: &str) -> NewTask {
    NewTask {
        parent: Some(parent.into()),
        ..NewTask::new(slug, "Jump departure", "owner")
    }
}

/// An orchestrator on `a`; `a/b` under it, `a/b/c` under that.
fn chain() -> (Fixture, String) {
    let f = Fixture::empty();
    let orch = f.session("orchestrator");
    f.task("a");
    f.store.take_role("a", &orch, "orchestrator").unwrap();
    f.task_with(under("a/b", "a"));
    f.task_with(under("a/b/c", "a/b"));
    (f, orch)
}

/// `slug` in review, its executor and reviewer each joined, and a landing posted on its thread.
fn landed(f: &Fixture, slug: &str) -> (String, String) {
    f.task(slug);
    let executor = f.session("worker");
    f.store.claim(slug, &executor).unwrap();
    f.store.move_to(slug, "review", &executor).unwrap();
    let reviewer = f.session("reviewer");
    f.store.take_role(slug, &reviewer, "reviewer").unwrap();
    f.store
        .say(
            slug,
            &reviewer,
            &format!("landed {slug} on main at 3e0c66192471"),
            None,
        )
        .unwrap();
    (executor, reviewer)
}

fn landings(f: &Fixture) -> BTreeSet<String> {
    f.store.landings_posted().unwrap()
}

#[test]
fn a_message_is_said_on_the_record() {
    let f = Fixture::empty();
    let (a, b) = (f.session("worker"), f.session("worker"));
    f.task("sim/jump");
    f.advance(5.0);
    let said = f
        .store
        .say("sim/jump", &a, "  the dock ring is at 900 \n", Some(&b))
        .unwrap();
    assert_eq!(
        (
            said.slug.as_str(),
            said.sender.as_str(),
            said.recipient.as_deref()
        ),
        ("sim/jump", a.as_str(), Some(b.as_str()))
    );
    assert_eq!(
        (said.kind.as_str(), said.text.as_str(), said.ts),
        ("msg", "the dock ring is at 900", f.now())
    );
    assert_eq!(f.store.thread("sim/jump").unwrap().last(), Some(&said));
}

/// A line that read the clock, then lost the race to one that read it later, is never dated before
/// the line ahead of it on the record: the SQLite store read the clock inside its write lock.
#[test]
fn a_line_is_never_dated_before_the_line_ahead_of_it() {
    let f = Fixture::empty();
    let (a, b) = (f.session("worker"), f.session("worker"));
    f.task("sim/jump");
    let client = f.other_client();
    let first = b.clone();
    let slow = f.slow(move || {
        let later = Store::with(
            Client::new(client.addr(), "sf", client.database()),
            Box::new(|| NOW + 30.0),
            Box::new(|_| false),
            HOST,
        )
        .unwrap();
        later.say("sim/jump", &first, "first", None).unwrap();
    });
    slow.say("sim/jump", &a, "second", None).unwrap();
    let thread = f.store.thread("sim/jump").unwrap();
    let dated: Vec<(String, f64)> = thread
        .into_iter()
        .filter(|m| m.kind == "msg")
        .map(|m| (m.text, m.ts))
        .collect();
    assert_eq!(
        dated,
        [
            ("first".to_string(), NOW + 30.0),
            ("second".to_string(), NOW + 30.0)
        ]
    );
}

/// fleet-vole: a line from a clock a day ahead dated every later line a day ahead too, and a session that
/// joined after them found them all in its inbox. A line further ahead than a write can take to commit is a
/// wrong clock, and the lines after it keep their own dates, also after the board is copied.
#[test]
fn a_line_from_a_clock_far_ahead_does_not_date_the_lines_after_it() {
    let f = Fixture::empty();
    let a = f.session("worker");
    f.task("sim/jump");
    let ahead = Store::with(
        f.other_client(),
        Box::new(|| NOW + 86_400.0),
        Box::new(|_| false),
        HOST,
    )
    .unwrap();
    ahead.say("sim/jump", &a, "ahead", None).unwrap();
    let said = f.store.say("sim/jump", &a, "now", None).unwrap();
    assert_eq!(said.ts, NOW);
    f.store.copy(&f.store.dump().unwrap()).unwrap();
    let said = f.store.say("sim/jump", &a, "after the copy", None).unwrap();
    assert_eq!(said.ts, NOW);
    let dated: Vec<(String, f64)> = f
        .store
        .thread("sim/jump")
        .unwrap()
        .into_iter()
        .filter(|m| m.kind == "msg")
        .map(|m| (m.text, m.ts))
        .collect();
    assert_eq!(
        dated,
        [
            ("ahead".to_string(), NOW + 86_400.0),
            ("now".to_string(), NOW),
            ("after the copy".to_string(), NOW)
        ]
    );
}

#[test]
fn a_message_addressed_to_a_handle_reaches_only_it() {
    let f = Fixture::empty();
    let (a, b) = (f.session("worker"), f.session("worker"));
    f.task("sim/jump");
    f.store
        .say("sim/jump", &a, "the dock ring is at 900", Some(&b))
        .unwrap();
    assert_eq!(
        texts(f.store.inbox(&b).unwrap()),
        ["the dock ring is at 900"]
    );
    assert!(f.store.inbox(&a).unwrap().is_empty());
}

#[test]
fn a_role_address_reaches_whoever_holds_the_role() {
    let f = Fixture::empty();
    let (orch, exe) = (f.session("orchestrator"), f.session("worker"));
    f.task("sim/jump");
    f.store
        .take_role("sim/jump", &orch, "orchestrator")
        .unwrap();
    f.store.claim("sim/jump", &exe).unwrap();
    f.store
        .say(
            "sim/jump",
            &exe,
            "blocked on the grid",
            Some("@orchestrator"),
        )
        .unwrap();
    f.store
        .say("sim/jump", &orch, "to whoever reviews", Some("@reviewer"))
        .unwrap();
    assert_eq!(
        texts(f.store.inbox(&orch).unwrap()),
        ["blocked on the grid"]
    );
    assert!(f.store.inbox(&exe).unwrap().is_empty());
}

#[test]
fn only_the_owner_broadcast_reaches_participants() {
    let f = Fixture::empty();
    let (a, b, outsider) = (
        f.session("worker"),
        f.session("worker"),
        f.session("worker"),
    );
    f.task("sim/jump");
    f.store.claim("sim/jump", &a).unwrap();
    f.store.take_role("sim/jump", &b, "orchestrator").unwrap();
    f.store
        .say("sim/jump", "owner", "please keep the golden", None)
        .unwrap();
    f.store.say("sim/jump", &a, "will do", None).unwrap();
    assert_eq!(
        texts(f.store.inbox(&a).unwrap()),
        ["please keep the golden"]
    );
    assert_eq!(
        texts(f.store.inbox(&b).unwrap()),
        ["please keep the golden"]
    );
    assert!(f.store.inbox(&outsider).unwrap().is_empty());
}

#[test]
fn ack_empties_the_inbox_and_new_messages_arrive_again() {
    let f = Fixture::empty();
    let a = f.session("worker");
    f.task("sim/jump");
    f.store.claim("sim/jump", &a).unwrap();
    f.store.say("sim/jump", "owner", "one", None).unwrap();
    let ids: Vec<i64> = f.store.inbox(&a).unwrap().iter().map(|m| m.id).collect();
    f.store.ack(&a, &ids).unwrap();
    f.store.ack(&a, &ids).unwrap(); // again: nothing new
    assert!(f.store.inbox(&a).unwrap().is_empty());
    f.store.say("sim/jump", "owner", "two", None).unwrap();
    assert_eq!(texts(f.store.inbox(&a).unwrap()), ["two"]);
    assert_eq!(f.store.dump().unwrap().deliveries.len(), 1);
}

#[test]
fn the_records_own_lines_and_what_came_before_a_session_never_reach_it() {
    let f = Fixture::empty();
    let a = f.session("worker");
    f.task("sim/jump");
    f.store
        .say("sim/jump", "owner", "before anyone", None)
        .unwrap();
    f.advance(1.0);
    let b = f.session("worker");
    f.store.claim("sim/jump", &a).unwrap();
    f.store.take_role("sim/jump", &b, "orchestrator").unwrap();
    assert!(
        f.store.inbox(&b).unwrap().is_empty(),
        "a claim is the record's, not a message"
    );
    assert_eq!(texts(f.store.inbox(&a).unwrap()), ["before anyone"]);
}

#[test]
fn messages_to_the_owner_are_listed_for_the_app_newest_first() {
    let f = Fixture::empty();
    let a = f.session("worker");
    f.task("sim/jump");
    for text in ["first", "second", "third"] {
        f.store.say("sim/jump", &a, text, Some("owner")).unwrap();
    }
    f.store.say("sim/jump", &a, "to everyone", None).unwrap();
    assert_eq!(
        texts(f.store.to_owner(40).unwrap()),
        ["third", "second", "first"]
    );
    assert_eq!(texts(f.store.to_owner(2).unwrap()), ["third", "second"]);
}

#[test]
fn empty_messages_and_unknown_senders_or_tasks_are_refused() {
    let f = Fixture::empty();
    f.task("sim/jump");
    assert_eq!(
        refused(f.store.say("sim/jump", "owner", "  ", None)),
        "an empty message says nothing"
    );
    assert_eq!(
        refused(f.store.say("sim/jump", "ghost-owl", "hi", None)),
        "no session 'ghost-owl'; attach first (just tasks-attach)"
    );
    assert_eq!(
        refused(f.store.say("sim/none", "owner", "hi", None)),
        "no task 'sim/none'"
    );
    assert_eq!(f.store.thread("sim/jump").unwrap().len(), 1);
}

#[test]
fn an_unknown_addressee_is_refused() {
    let f = Fixture::empty();
    let a = f.session("worker");
    f.task("sim/jump");
    assert_eq!(
        refused(f.store.say("sim/jump", &a, "hi", Some("ghost-owl"))),
        "no session 'ghost-owl' to send to"
    );
    assert_eq!(
        refused(f.store.say("sim/jump", &a, "hi", Some("@janitor"))),
        "no role @janitor; use one of @orchestrator, @executor, @reviewer, @reserved"
    );
}

#[test]
fn saying_keeps_the_sender_live() {
    let f = Fixture::empty();
    let a = f.session("worker");
    f.task("sim/jump");
    f.advance(LIVE_SECONDS + 1.0);
    f.store.say("sim/jump", &a, "still here", None).unwrap();
    assert_eq!(f.health(&a), Health::Live);
}

#[test]
fn the_inbox_of_an_unknown_session_is_refused() {
    let f = Fixture::empty();
    assert_eq!(
        refused(f.store.inbox("ghost-owl")),
        "no session 'ghost-owl'"
    );
    assert_eq!(
        refused(f.store.ack("ghost-owl", &[1])),
        "no session 'ghost-owl'; attach first (just tasks-attach)"
    );
}

#[test]
fn a_changes_verdict_reaches_the_executor() {
    let f = Fixture::empty();
    let (exe, rev) = (f.session("worker"), f.session("reviewer"));
    f.task("sim/jump");
    f.store.claim("sim/jump", &exe).unwrap();
    f.store.move_to("sim/jump", "review", &exe).unwrap();
    f.store.take_role("sim/jump", &rev, "reviewer").unwrap();
    f.store
        .verdict("sim/jump", &rev, "changes", "the hash moved")
        .unwrap();
    let inbox: Vec<(String, String)> = f
        .store
        .inbox(&exe)
        .unwrap()
        .into_iter()
        .map(|m| (m.kind, m.text))
        .collect();
    assert_eq!(
        inbox,
        [("verdict".to_string(), "changes: the hash moved".to_string())]
    );
}

#[test]
fn events_on_a_grandchild_reach_the_root_orchestrator() {
    let (f, orch) = chain();
    let agent = f.session("worker");
    f.store.claim("a/b/c", &agent).unwrap();
    f.store.say("a/b/c", &agent, "halfway", None).unwrap();
    f.store
        .say("a/b/c", &agent, "to the reviewer", Some("@reviewer"))
        .unwrap();
    f.store
        .say(
            "a/b/c",
            &agent,
            "to the orchestrator",
            Some("@orchestrator"),
        )
        .unwrap();
    let private = f.session("worker");
    f.store
        .say("a/b/c", &agent, "private", Some(&private))
        .unwrap();
    assert_eq!(
        texts(f.store.inbox(&orch).unwrap()),
        ["to the orchestrator"]
    );
}

#[test]
fn a_changes_verdict_on_a_grandchild_reaches_the_root_orchestrator() {
    let (f, orch) = chain();
    let (agent, reviewer) = (f.session("worker"), f.session("reviewer"));
    f.store.claim("a/b/c", &agent).unwrap();
    f.store.move_to("a/b/c", "review", &agent).unwrap();
    f.store.take_role("a/b/c", &reviewer, "reviewer").unwrap();
    f.store
        .verdict("a/b/c", &reviewer, "changes", "no")
        .unwrap();
    let kinds: Vec<String> = f
        .store
        .inbox(&orch)
        .unwrap()
        .into_iter()
        .map(|m| m.kind)
        .collect();
    assert_eq!(kinds, ["verdict"]);
}

#[test]
fn a_done_note_reaches_the_root_orchestrator() {
    let (f, orch) = chain();
    let agent = f.session("worker");
    f.task_with(NewTask {
        instruction: true,
        ..under("a/b/c/d", "a/b/c")
    });
    f.store.claim("a/b/c/d", &agent).unwrap();
    f.store.done("a/b/c/d", &agent, "all clear").unwrap();
    assert_eq!(texts(f.store.inbox(&orch).unwrap()), ["done: all clear"]);
}

#[test]
fn an_orchestrator_of_a_sibling_hears_nothing() {
    let (f, _) = chain();
    let stranger = f.session("orchestrator");
    f.task("elsewhere");
    f.store
        .take_role("elsewhere", &stranger, "orchestrator")
        .unwrap();
    let agent = f.session("worker");
    f.store.claim("a/b/c", &agent).unwrap();
    f.store.say("a/b/c", &agent, "hello", None).unwrap();
    assert!(f.store.inbox(&stranger).unwrap().is_empty());
}

#[test]
fn each_sender_has_its_latest_board_message() {
    let f = Fixture::empty();
    let (a, b) = (f.session("worker"), f.session("worker"));
    f.task("sim/jump");
    f.store.say("sim/jump", &a, "first", None).unwrap();
    f.advance(5.0);
    f.store.say("sim/jump", &a, "second", None).unwrap();
    f.advance(5.0);
    f.store.say("sim/jump", &b, "from b", None).unwrap();
    let last = f.store.last_messages().unwrap();
    assert_eq!(
        (last[&a].text.as_str(), last[&a].slug.as_str(), last[&a].ts),
        ("second", "sim/jump", NOW + 5.0)
    );
    assert_eq!(last[&b].text, "from b");
    assert_eq!(last.len(), 2);
}

#[test]
fn the_record_of_what_the_board_did_is_not_a_message_an_agent_wrote() {
    let f = Fixture::empty();
    let a = f.session("worker");
    f.task("sim/jump");
    f.store.claim("sim/jump", &a).unwrap();
    assert!(!f.store.last_messages().unwrap().contains_key(&a));
}

#[test]
fn a_verdict_counts_as_a_last_word() {
    let f = Fixture::empty();
    let (exe, rev) = (f.session("worker"), f.session("reviewer"));
    f.task("sim/jump");
    f.store.claim("sim/jump", &exe).unwrap();
    f.store.move_to("sim/jump", "review", &exe).unwrap();
    f.store.take_role("sim/jump", &rev, "reviewer").unwrap();
    f.store
        .verdict("sim/jump", &rev, "changes", "the ring")
        .unwrap();
    assert_eq!(
        f.store.last_messages().unwrap()[&rev].text,
        "changes: the ring"
    );
}

#[test]
fn nobody_has_spoken() {
    let f = Fixture::empty();
    f.task("sim/jump");
    assert!(f.store.last_messages().unwrap().is_empty());
}

#[test]
fn a_landing_line_counts() {
    let f = Fixture::empty();
    landed(&f, "sim/a");
    assert_eq!(landings(&f), BTreeSet::from(["sim/a".to_string()]));
}

/// `Store::landed` writes the landing as the task's verdict: that line is the landing, not a status
/// change after it (`n.id > m.id`).
#[test]
fn a_landing_the_board_records_as_its_verdict_counts() {
    let f = Fixture::empty();
    f.task("sim/a");
    let executor = f.session("worker");
    f.store.claim("sim/a", &executor).unwrap();
    f.store.move_to("sim/a", "review", &executor).unwrap();
    f.store
        .landed(
            "sim/a",
            &executor,
            "landed sim/a on main at 3e0c66192471",
            false,
        )
        .unwrap();
    assert_eq!(f.store.get("sim/a").unwrap().status, "done");
    assert_eq!(landings(&f), BTreeSet::from(["sim/a".to_string()]));
    f.store
        .say("sim/a", &executor, "the worktree is gone", None)
        .unwrap();
    assert_eq!(landings(&f), BTreeSet::from(["sim/a".to_string()]));
}

#[test]
fn a_landing_line_names_its_own_task() {
    let f = Fixture::empty();
    landed(&f, "sim/a");
    f.task("sim/b");
    f.store
        .say(
            "sim/b",
            "owner",
            "landed sim/a on main at 3e0c66192471",
            None,
        )
        .unwrap();
    f.store
        .say(
            "sim/b",
            "owner",
            "Landed sim/b on Main at 3e0c66192471",
            None,
        )
        .unwrap();
    assert_eq!(landings(&f), BTreeSet::from(["sim/a".to_string()]));
}

#[test]
fn a_changes_verdict_that_sends_the_task_back_voids_it() {
    let f = Fixture::empty();
    let (_, reviewer) = landed(&f, "sim/a");
    f.store
        .verdict("sim/a", &reviewer, "changes", "rework")
        .unwrap();
    assert_eq!(f.store.get("sim/a").unwrap().status, "in_progress");
    assert!(landings(&f).is_empty());
}

#[test]
fn a_release_alone_keeps_it_and_a_claim_after_voids_it() {
    let f = Fixture::empty();
    f.task("sim/a");
    let first = f.session("worker");
    f.store.claim("sim/a", &first).unwrap();
    let orch = f.session("orchestrator");
    f.store
        .say("sim/a", &orch, "landed sim/a on main at 3e0c66192471", None)
        .unwrap();
    f.store.release("sim/a", &first, None).unwrap();
    assert_eq!(landings(&f), BTreeSet::from(["sim/a".to_string()]));
    let second = f.session("worker");
    f.store.claim("sim/a", &second).unwrap();
    assert!(landings(&f).is_empty());
}

#[test]
fn a_move_voids_it() {
    let f = Fixture::empty();
    let (executor, _) = landed(&f, "sim/a");
    f.store.move_to("sim/a", "in_progress", &executor).unwrap();
    assert!(landings(&f).is_empty());
}

#[test]
fn a_landing_posted_after_the_rework_counts_again() {
    let f = Fixture::empty();
    let (executor, reviewer) = landed(&f, "sim/a");
    f.store
        .verdict("sim/a", &reviewer, "changes", "rework")
        .unwrap();
    f.store.move_to("sim/a", "review", &executor).unwrap();
    f.store
        .say(
            "sim/a",
            &reviewer,
            "landed sim/a on main at aaaaaaaaaaaa",
            None,
        )
        .unwrap();
    assert_eq!(landings(&f), BTreeSet::from(["sim/a".to_string()]));
}

#[test]
fn a_move_claim_or_verdict_on_another_task_does_not_void_it() {
    let f = Fixture::empty();
    landed(&f, "sim/a");
    let (other_executor, other_reviewer) = landed(&f, "sim/b");
    f.store
        .verdict("sim/b", &other_reviewer, "changes", "rework")
        .unwrap();
    f.store.move_to("sim/b", "review", &other_executor).unwrap();
    f.task("sim/c");
    let worker = f.session("worker");
    f.store.claim("sim/c", &worker).unwrap();
    assert_eq!(landings(&f), BTreeSet::from(["sim/a".to_string()]));
}

#[test]
fn a_remark_after_the_landing_does_not_void_it() {
    let f = Fixture::empty();
    let (_, reviewer) = landed(&f, "sim/a");
    f.store
        .say("sim/a", &reviewer, "thanks, tidy up the worktree", None)
        .unwrap();
    assert_eq!(landings(&f), BTreeSet::from(["sim/a".to_string()]));
}
