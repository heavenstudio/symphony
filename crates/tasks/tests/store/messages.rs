//! Threads: saying, inboxes and acknowledging them, what reaches the owner, each sender's last word,
//! and landings on the record.

mod inbox;
mod last_words;
mod questions;

use crate::common::{Backend, Fixture, HOST, NOW, Seed, chain, in_review, refused, under};
use std::collections::BTreeSet;
use symphony_board::health::LIVE_SECONDS;
use symphony_board::{Health, LANDS_FROM, Message, NewLaunch, NewTask, Store};

fn texts(messages: Vec<Message>) -> Vec<String> {
    messages.into_iter().map(|m| m.text).collect()
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

on_both! {
fn a_message_is_said_on_the_record(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
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
}

on_both! {
/// A line that read the clock, then lost the race to one that read it later, is never dated before
/// the line ahead of it on the record: the SQLite store read the clock inside its write lock.
fn a_line_is_never_dated_before_the_line_ahead_of_it(backend: Backend) {
    let f = Fixture::on(backend, crate::common::Seed::default());
    let (a, b) = (f.session("worker"), f.session("worker"));
    f.task("sim/jump");
    let client = f.other_database();
    let first = b.clone();
    let slow = f.slow(move || {
        let later = Store::opening_on(
            client.another(),
            Box::new(|| NOW + 30.0),
            Box::new(|_| false),
            HOST,
            &symphony_board::Opening::write(),
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

on_both! {
fn a_message_addressed_to_a_handle_reaches_only_it(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
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
}

on_both! {
fn a_role_address_reaches_whoever_holds_the_role(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
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
}

on_both! {
fn only_the_owner_broadcast_reaches_participants(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
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
}

on_both! {
fn the_records_own_lines_and_what_came_before_a_session_never_reach_it(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
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
}

on_both! {
fn messages_to_the_owner_are_listed_for_the_app_newest_first(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
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
}

on_both! {
fn empty_messages_and_unknown_senders_or_tasks_are_refused(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
    f.task("sim/jump");
    assert_eq!(
        refused(f.store.say("sim/jump", "owner", "  ", None)),
        "an empty message says nothing"
    );
    assert_eq!(
        refused(f.store.say("sim/jump", "ghost-owl", "hi", None)),
        f.no_session("ghost-owl")
    );
    assert_eq!(
        refused(f.store.say("sim/none", "owner", "hi", None)),
        "no task 'sim/none'"
    );
    assert_eq!(f.store.thread("sim/jump").unwrap().len(), 1);

}
}

on_both! {
fn an_unknown_addressee_is_refused(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
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
    // A role's name without its `@` is a handle nobody has: the refusal says how it is written.
    for (to, role) in [("orchestrator", "orchestrator"), ("Reviewer", "reviewer")] {
        assert_eq!(
            refused(f.store.say("sim/jump", &a, "hi", Some(to))),
            format!("no session '{to}' to send to; the task's {role} is --to @{role}")
        );
    }
    assert_eq!(f.store.thread("sim/jump").unwrap().len(), 1);

}
}

on_both! {
fn saying_keeps_the_sender_live(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
    let a = f.session("worker");
    f.task("sim/jump");
    f.advance(LIVE_SECONDS + 1.0);
    f.store.say("sim/jump", &a, "still here", None).unwrap();
    assert_eq!(f.health(&a), Health::Live);

}
}

on_both! {
fn the_inbox_of_an_unknown_session_is_refused(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
    assert_eq!(
        refused(f.store.inbox("ghost-owl")),
        "no session 'ghost-owl'"
    );
    assert_eq!(
        refused(f.store.ack("ghost-owl", &[1])),
        f.no_session("ghost-owl")
    );

}
}

on_both! {
fn a_changes_verdict_reaches_the_executor(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
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
}

on_both! {
fn events_on_a_grandchild_reach_the_root_orchestrator(backend: Backend) {
    let (f, orch) = chain(backend);
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
}

on_both! {
fn a_changes_verdict_on_a_grandchild_reaches_the_root_orchestrator(backend: Backend) {
    let (f, orch) = chain(backend);
    let (agent, reviewer) = (f.session("worker"), f.session("reviewer"));
    f.store.claim("a/b/c", &agent).unwrap();
    f.historical_review("a/b/c");
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
}

on_both! {
fn a_done_note_reaches_the_root_orchestrator(backend: Backend) {
    let (f, orch) = chain(backend);
    let agent = f.session("worker");
    f.task_with(NewTask {
        instruction: true,
        ..under("a/b/c/d", "a/b/c")
    });
    f.store.claim("a/b/c/d", &agent).unwrap();
    f.store.done("a/b/c/d", &agent, "all clear").unwrap();
    assert_eq!(texts(f.store.inbox(&orch).unwrap()), ["done: all clear"]);

}
}

on_both! {
fn an_orchestrator_of_a_sibling_hears_nothing(backend: Backend) {
    let (f, _) = chain(backend);
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
}

on_both! {
fn each_sender_has_its_latest_board_message(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
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
}

on_both! {
fn the_record_of_what_the_board_did_is_not_a_message_an_agent_wrote(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
    let a = f.session("worker");
    f.task("sim/jump");
    f.store.claim("sim/jump", &a).unwrap();
    assert!(!f.store.last_messages().unwrap().contains_key(&a));

}
}

on_both! {
fn a_verdict_counts_as_a_last_word(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
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
}

on_both! {
fn nobody_has_spoken(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
    f.task("sim/jump");
    assert!(f.store.last_messages().unwrap().is_empty());

}
}

on_both! {
fn a_landing_line_counts(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
    landed(&f, "sim/a");
    assert_eq!(landings(&f), BTreeSet::from(["sim/a".to_string()]));

}
}

on_both! {
/// `Store::landed` writes the landing as the task's verdict: that line is the landing, not a status
/// change after it (`n.id > m.id`).
fn a_landing_the_board_records_as_its_verdict_counts(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
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
            &LANDS_FROM,
        )
        .unwrap();
    assert_eq!(f.store.get("sim/a").unwrap().status, "done");
    assert_eq!(landings(&f), BTreeSet::from(["sim/a".to_string()]));
    f.store
        .say("sim/a", &executor, "the worktree is gone", None)
        .unwrap();
    assert_eq!(landings(&f), BTreeSet::from(["sim/a".to_string()]));

}
}

on_both! {
fn a_landing_line_names_its_own_task(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
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
}

on_both! {
fn a_changes_verdict_that_sends_the_task_back_voids_it(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
    let (_, reviewer) = landed(&f, "sim/a");
    f.store
        .verdict("sim/a", &reviewer, "changes", "rework")
        .unwrap();
    assert_eq!(f.store.get("sim/a").unwrap().status, "in_progress");
    assert!(landings(&f).is_empty());

}
}

on_both! {
fn a_release_alone_keeps_it_and_a_claim_after_voids_it(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
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
}

on_both! {
fn a_move_voids_it(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
    let (executor, _) = landed(&f, "sim/a");
    f.store.move_to("sim/a", "in_progress", &executor).unwrap();
    assert!(landings(&f).is_empty());

}
}

on_both! {
fn a_landing_posted_after_the_rework_counts_again(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
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
}

on_both! {
fn a_move_claim_or_verdict_on_another_task_does_not_void_it(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
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
}

on_both! {
fn a_remark_after_the_landing_does_not_void_it(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
    let (_, reviewer) = landed(&f, "sim/a");
    f.store
        .say("sim/a", &reviewer, "thanks, tidy up the worktree", None)
        .unwrap();
    assert_eq!(landings(&f), BTreeSet::from(["sim/a".to_string()]));

}
}

// -- a post its executor cannot read ------------------------------------------------------

/// What the board told `sim/jump`'s orchestrator.
fn told(f: &Fixture) -> Vec<String> {
    let inbox = f.store.inbox("ochre-owl").unwrap();
    inbox
        .into_iter()
        .filter(|m| m.sender == "board")
        .map(|m| m.text)
        .collect()
}

/// `amber-heron` ran in a pane the board recorded closed.
fn pane_closed(f: &Fixture) {
    f.store
        .record_launch(&NewLaunch {
            claude_session: "sess-amber".into(),
            pane: "t7".into(),
            ..NewLaunch::new("sim/jump", "amber-heron", "executor")
        })
        .unwrap();
    f.store
        .close_task_session(
            "sim/jump",
            "amber-heron",
            "idle for two sweeps, its work finished",
            "just tasks restore sim/jump amber-heron",
        )
        .unwrap();
}

on_both! {
fn a_post_its_live_executor_reads_tells_nobody_else(backend: Backend) {
    let f = in_review(backend);
    f.store
        .say("sim/jump", "brisk-finch", "a nit", Some("@executor"))
        .unwrap();
    assert_eq!(told(&f), Vec::<String>::new());
    assert_eq!(texts(f.store.inbox("amber-heron").unwrap()), ["a nit"]);

}
}

on_both! {
fn a_post_to_an_executor_whose_pane_was_closed_tells_the_orchestrator_how_to_restore_it(backend: Backend) {
    let f = in_review(backend);
    pane_closed(&f);
    assert_eq!(
        f.health("amber-heron"),
        Health::Live,
        "the record decides, not the last beat"
    );
    let sent = f
        .store
        .say(
            "sim/jump",
            "brisk-finch",
            "the hash moved",
            Some("@executor"),
        )
        .unwrap();
    assert_eq!(
        told(&f),
        [format!(
            "#{} from brisk-finch to @executor reaches no live executor: amber-heron's pane was closed (idle for two sweeps, its work finished); just tasks restore sim/jump amber-heron",
            sent.id
        )]
    );
    let notice = f.store.thread("sim/jump").unwrap().pop().unwrap();
    assert_eq!(
        (
            notice.sender.as_str(),
            notice.kind.as_str(),
            notice.recipient.as_deref()
        ),
        ("board", "msg", Some("@orchestrator"))
    );
    assert_eq!(
        texts(f.store.inbox("amber-heron").unwrap()),
        ["the hash moved"],
        "the post waits in its inbox for the restore"
    );
    assert_eq!(
        texts(f.store.inbox("brisk-finch").unwrap()),
        Vec::<String>::new()
    );
}
}

on_both! {
/// A landing that closes work taken back to in progress says so to its executor in the verdict
/// itself, and to the orchestrator when no executor reads it; one that closes work in review tells
/// nobody. Both count as the task's landing.
fn a_landing_of_work_taken_back_reaches_its_executor_or_the_orchestrator(backend: Backend) {
    let f = in_review(backend);
    pane_closed(&f);
    f.store
        .landed(
            "sim/jump",
            "ochre-owl",
            "landed sim/jump on main at 3e0c66192471",
            false,
            &LANDS_FROM,
        )
        .unwrap();
    assert_eq!(told(&f), Vec::<String>::new());
    let f = in_review(backend);
    f.store
        .move_to("sim/jump", "in_progress", "amber-heron")
        .unwrap();
    pane_closed(&f);
    let landed = f
        .store
        .landed(
            "sim/jump",
            "ochre-owl",
            "landed sim/jump on main at 3e0c66192471",
            false,
            &LANDS_FROM,
        )
        .unwrap();
    assert_eq!(
        (landed.task.status.as_str(), landed.from.as_str()),
        ("done", "in_progress")
    );
    let verdict = f
        .store
        .thread("sim/jump")
        .unwrap()
        .into_iter()
        .rfind(|m| m.kind == "verdict")
        .unwrap();
    assert_eq!(verdict.recipient.as_deref(), Some("@executor"));
    assert_eq!(
        texts(f.store.inbox("amber-heron").unwrap()),
        [
            "landed sim/jump on main at 3e0c66192471; sim/jump was taken back to in_progress, so the landing closes it: further changes go in a new task"
        ]
    );
    assert_eq!(
        told(&f),
        [
            "the landing of sim/jump from ochre-owl to @executor reaches no live executor: amber-heron's pane was closed (idle for two sweeps, its work finished); just tasks restore sim/jump amber-heron"
        ]
    );
    assert_eq!(landings(&f), BTreeSet::from(["sim/jump".to_string()]));
}
}

on_both! {
fn a_post_to_the_executor_by_handle_is_checked_and_one_to_another_handle_is_not(backend: Backend) {
    let f = in_review(backend);
    pane_closed(&f);
    f.store
        .say("sim/jump", "ochre-owl", "your turn", Some("brisk-finch"))
        .unwrap();
    assert_eq!(told(&f), Vec::<String>::new(), "brisk-finch reviews it");
    let sent = f
        .store
        .say("sim/jump", "ochre-owl", "rebase it", Some("amber-heron"))
        .unwrap();
    assert_eq!(
        told(&f),
        [format!(
            "#{} from ochre-owl to amber-heron reaches no live executor: amber-heron's pane was closed (idle for two sweeps, its work finished); just tasks restore sim/jump amber-heron",
            sent.id
        )]
    );
}
}

on_both! {
fn a_quiet_executor_with_no_callback_is_named_by_its_health(backend: Backend) {
    let f = in_review(backend);
    f.advance(LIVE_SECONDS + 1.0);
    f.store
        .say(
            "sim/jump",
            "brisk-finch",
            "for the reviewers",
            Some("@reviewer"),
        )
        .unwrap();
    assert_eq!(told(&f), Vec::<String>::new(), "only a post to @executor");
    let sent = f
        .store
        .say(
            "sim/jump",
            "brisk-finch",
            "the hash moved",
            Some("@executor"),
        )
        .unwrap();
    assert_eq!(
        told(&f),
        [format!(
            "#{} from brisk-finch to @executor reaches no live executor: amber-heron is stale with no callback",
            sent.id
        )]
    );

}
}

on_both! {
fn a_quiet_executor_with_a_native_callback_is_reached(backend: Backend) {
    let f = in_review(backend);
    f.advance(LIVE_SECONDS + 1.0);
    // Registered on another host, so this board's dispatch never tries to deliver it.
    (f.store.raw().row("callback_state", "amber-heron"))
        .put(serde_json::json!({
            "handle": "amber-heron",
            "address": "codex:abcdef01",
            "host": "another-host",
        }))
        .unwrap();
    assert!(f.store.callbacks().unwrap().contains_key("amber-heron"));
    f.store
        .say(
            "sim/jump",
            "brisk-finch",
            "the hash moved",
            Some("@executor"),
        )
        .unwrap();
    assert_eq!(told(&f), Vec::<String>::new());
}
}

on_both! {
fn the_orchestrator_hears_its_own_post_reached_nobody_when_no_session_executes_the_task(backend: Backend) {
    let f = Fixture::on(backend,
        Seed::default()
            .session("ochre-owl")
            .task("sim/jump", "in_progress")
            .role("sim/jump", "ochre-owl", "orchestrator"),
    );
    let sent = f
        .store
        .say("sim/jump", "ochre-owl", "rebase it", Some("@executor"))
        .unwrap();
    assert_eq!(
        told(&f),
        [format!(
            "#{} from ochre-owl to @executor reaches no live executor: no session holds the role",
            sent.id
        )]
    );

}
}

on_both! {
fn an_orchestrator_above_the_task_is_told_too(backend: Backend) {
    let (f, orch) = chain(backend);
    let executor = f.session("worker");
    f.store.claim("a/b/c", &executor).unwrap();
    f.historical_review("a/b/c");
    f.advance(LIVE_SECONDS + 1.0);
    let reviewer = f.session("reviewer");
    f.store.take_role("a/b/c", &reviewer, "reviewer").unwrap();
    f.store
        .say("a/b/c", &reviewer, "the hash moved", Some("@executor"))
        .unwrap();
    let heard: Vec<(String, String)> = f
        .store
        .inbox(&orch)
        .unwrap()
        .into_iter()
        .map(|m| (m.slug, m.sender))
        .collect();
    assert_eq!(heard, [("a/b/c".to_string(), "board".to_string())]);

}
}
