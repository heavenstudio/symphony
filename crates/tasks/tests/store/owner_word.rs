//! The owner's word on a task reaches who acts on it at once, with no orchestrator relaying it:
//! its posts and moves reach everyone on the task and the task's orchestrator, and on a task no live
//! orchestrator holds (nobody, or a holder gone stale) every orchestrator, marked so, through
//! `tasks wait`, a channel and the inbox alike.
//! Its cancel tells the executor the task moves no further. A task an orchestrator files is its
//! own.

use super::common::{Fixture, Seed};
use super::quiet_callbacks::listen;
use symphony_board::cli::Env;
use symphony_board::health::{LIVE_SECONDS, STALE_SECONDS};
use symphony_board::{NewSession, NewTask};

const UNOWNED: &str = "[unowned: no orchestrator holds it] ";
const NO_FURTHER: &str = "; it moves no further: stop work on it";

/// What one look of `tasks wait` printed for `handle`, line by line.
fn read(f: &Fixture, handle: &str) -> Vec<String> {
    let mut said: Vec<String> = Vec::new();
    let mut print = |text: &str| {
        said.extend(text.lines().map(str::to_owned));
        Ok(())
    };
    (f.store)
        .collect_callbacks(handle, &Env::of_process(), &mut print)
        .unwrap();
    said
}

/// Of `lines`, the ones naming a thread's line or a move: not the PULL digest's.
fn words(lines: Vec<String>) -> Vec<String> {
    let said = ["MSG ", "MOVE ", "VERDICT ", "STATUS "];
    (lines.into_iter())
        .filter(|line| said.iter().any(|word| line.starts_with(word)))
        .collect()
}

/// The texts in `handle`'s inbox, as `tasks inbox` reads them.
fn inbox(f: &Fixture, handle: &str) -> Vec<String> {
    let messages = f.store.inbox(handle).unwrap();
    messages.into_iter().map(|m| m.text).collect()
}

/// The id of the newest line on `slug`'s thread that reads `text`.
fn line(f: &Fixture, slug: &str, text: &str) -> i64 {
    let thread = f.store.thread(slug).unwrap();
    let found = thread.iter().rev().find(|m| m.text == text);
    found.unwrap_or_else(|| panic!("{text} in {thread:?}")).id
}

#[test]
fn the_owners_post_on_a_task_nobody_orchestrates_reaches_every_orchestrator_marked() {
    let f = Fixture::new(crate::common::Seed::default());
    let (a, b, c) = (
        f.session("orchestrator"),
        f.session("orchestrator"),
        f.session("orchestrator"),
    );
    let (exe, other) = (f.session("worker"), f.session("worker"));
    f.task("tools/loose");
    f.store.claim("tools/loose", &exe).unwrap();
    for handle in [&a, &b, &exe, &other] {
        read(&f, handle);
    }
    assert!(inbox(&f, &c).is_empty());
    let post = f
        .store
        .say("tools/loose", "owner", "stop it", None)
        .unwrap();
    let said = format!("MSG #{} tools/loose from owner — {UNOWNED}stop it", post.id);
    // Each orchestrator waiting hears it, once, marked.
    for handle in [&a, &b] {
        assert_eq!(words(read(&f, handle)), [said.as_str()], "{handle}");
        assert!(words(read(&f, handle)).is_empty(), "{handle}");
    }
    // The executor hears it unmarked: the mark says why it reached an orchestrator.
    let plain = format!("MSG #{} tools/loose from owner — stop it", post.id);
    assert_eq!(words(read(&f, &exe)), [plain]);
    assert!(inbox(&f, &exe).is_empty(), "read by waiting");
    // An orchestrator that reads its inbox hears it there, and a worker on no role, nothing.
    assert_eq!(inbox(&f, &c), [format!("{UNOWNED}stop it")]);
    assert!(words(read(&f, &other)).is_empty());
    assert!(inbox(&f, &other).is_empty());
    // The thread keeps the line as the owner wrote it.
    assert_eq!(f.said("tools/loose").last().unwrap(), "stop it");
}

#[test]
fn the_owners_post_on_an_orchestrated_task_reaches_its_orchestrator_alone() {
    let f = Fixture::new(crate::common::Seed::default());
    let (mine, theirs) = (f.session("orchestrator"), f.session("orchestrator"));
    f.task("tools/held");
    f.task_with(NewTask {
        parent: Some("tools/held".into()),
        instruction: true,
        ..NewTask::new("tools/held/step", "A step", "owner")
    });
    f.store
        .take_role("tools/held", &mine, "orchestrator")
        .unwrap();
    for handle in [&mine, &theirs] {
        read(&f, handle);
    }
    let post = f.store.say("tools/held", "owner", "on it", None).unwrap();
    // On a subtask it holds no role on: the orchestrator of the nearest task above holds it.
    let below = (f.store)
        .say("tools/held/step", "owner", "this too", None)
        .unwrap();
    assert_eq!(
        words(read(&f, &mine)),
        [
            format!("MSG #{} tools/held from owner — on it", post.id),
            format!("MSG #{} tools/held/step from owner — this too", below.id),
        ]
    );
    assert!(words(read(&f, &theirs)).is_empty());
    assert!(inbox(&f, &theirs).is_empty());
}

/// An orchestrator holding a task that is not live, gone stale or lost, keeps the owner's word to
/// itself no longer: every other orchestrator waiting hears the post and the cancel at once and
/// once, marked with the holder that is not live; the stale holder still hears both, unmarked, and
/// so does the executor.
#[test]
fn the_owners_word_on_a_task_whose_orchestrator_is_not_live_reaches_every_orchestrator() {
    for lost in [false, true] {
        let f = Fixture::new(crate::common::Seed::default());
        let (gone, here) = (f.session("orchestrator"), f.session("orchestrator"));
        let exe = f.session("worker");
        f.task("tools/left");
        f.store
            .take_role("tools/left", &gone, "orchestrator")
            .unwrap();
        f.store.claim("tools/left", &exe).unwrap();
        for handle in [&gone, &here, &exe] {
            read(&f, handle);
        }
        // Quiet past the live line (stale) or past the lost line, with `here` and `exe` beating.
        f.advance(LIVE_SECONDS + if lost { STALE_SECONDS } else { 0.0 } + 5.0);
        for handle in [&here, &exe] {
            f.store.touch(handle).unwrap();
        }
        let post = f.store.say("tools/left", "owner", "anyone?", None).unwrap();
        f.store.move_to("tools/left", "cancelled", "owner").unwrap();
        let moved = "owner moved it in_progress → cancelled";
        let move_id = line(&f, "tools/left", moved);
        let told = |mark: &str| {
            [
                format!("MSG #{} tools/left from owner — {mark}anyone?", post.id),
                format!("MOVE #{move_id} tools/left from owner — {mark}{moved}{NO_FURTHER}"),
            ]
        };
        // A lost holder holds the task no more; a stale one does, and is named.
        let mark = match lost {
            true => UNOWNED.to_string(),
            false => format!("[unowned: no live orchestrator holds it: {gone} (stale)] "),
        };
        assert_eq!(words(read(&f, &here)), told(&mark), "lost: {lost}");
        assert!(words(read(&f, &here)).is_empty(), "lost: {lost}");
        assert_eq!(words(read(&f, &exe)), told(""), "lost: {lost}");
        if !lost {
            assert_eq!(words(read(&f, &gone)), told(""), "its own task's word");
        }
    }
}

#[test]
fn an_orchestrator_gone_quiet_leaves_its_task_nobody_s_and_the_owners_word_everyone_s() {
    let f = Fixture::new(crate::common::Seed::default());
    let (gone, here) = (f.session("orchestrator"), f.session("orchestrator"));
    f.task("tools/left");
    f.store
        .take_role("tools/left", &gone, "orchestrator")
        .unwrap();
    // Before: the task is `gone`'s, and the owner's word on it is not `here`'s.
    f.store.say("tools/left", "owner", "busy?", None).unwrap();
    assert!(inbox(&f, &here).is_empty());
    // Past the lost line with only `here` beating: `gone` keeps the role but no longer holds it.
    crate::orchestrators::go_quiet_but(&f, &[&here]);
    f.store.say("tools/left", "owner", "anyone?", None).unwrap();
    // The inbox decides as it reads: what the owner said that `gone` never relayed is `here`'s now.
    assert_eq!(
        inbox(&f, &here),
        [format!("{UNOWNED}busy?"), format!("{UNOWNED}anyone?")]
    );
}

#[test]
fn the_owners_cancel_reaches_the_executor_at_once_saying_the_task_moves_no_further() {
    let f = Fixture::new(crate::common::Seed::default());
    let (orch, exe) = (f.session("orchestrator"), f.session("worker"));
    f.task("tools/loose");
    f.store.claim("tools/loose", &exe).unwrap();
    for handle in [&orch, &exe] {
        read(&f, handle);
    }
    (f.store)
        .move_with("tools/loose", "cancelled", "owner", Some("not wanted"))
        .unwrap();
    let moved = "owner moved it in_progress → cancelled";
    let (move_id, why) = (
        line(&f, "tools/loose", moved),
        line(&f, "tools/loose", "cancelled: not wanted"),
    );
    let told = |mark: &str| {
        [
            format!("MOVE #{move_id} tools/loose from owner — {mark}{moved}{NO_FURTHER}"),
            format!("MSG #{why} tools/loose from owner — {mark}cancelled: not wanted"),
        ]
    };
    // Said once each, as the owner's move and its reason, never again as a STATUS; marked for
    // the orchestrator it reached because nobody orchestrates the task.
    assert_eq!(words(read(&f, &exe)), told(""));
    assert_eq!(words(read(&f, &orch)), told(UNOWNED));
    assert!(words(read(&f, &exe)).is_empty());
}

#[test]
fn the_owners_cancel_of_an_orchestrated_task_is_said_to_those_on_it_and_no_other() {
    let f = Fixture::new(crate::common::Seed::default());
    let (orch, theirs) = (f.session("orchestrator"), f.session("orchestrator"));
    let exe = f.session_with(NewSession {
        parent: Some(orch.clone()),
        ..NewSession::new("worker")
    });
    let rev = f.session("reviewer");
    f.task("tools/held");
    f.store
        .take_role("tools/held", &orch, "orchestrator")
        .unwrap();
    f.store.claim("tools/held", &exe).unwrap();
    f.store.move_to("tools/held", "review", &exe).unwrap();
    f.store.take_role("tools/held", &rev, "reviewer").unwrap();
    for handle in [&orch, &theirs, &exe, &rev] {
        read(&f, handle);
    }
    f.store.move_to("tools/held", "cancelled", "owner").unwrap();
    let moved = "owner moved it review → cancelled";
    let id = line(&f, "tools/held", moved);
    let told = format!("MOVE #{id} tools/held from owner — {moved}{NO_FURTHER}");
    for handle in [&orch, &exe, &rev] {
        assert_eq!(words(read(&f, handle)), [told.as_str()], "{handle}");
    }
    // Another orchestrator hears nothing of it, by waiting or in its inbox.
    assert!(words(read(&f, &theirs)).is_empty());
    assert!(inbox(&f, &theirs).is_empty());
}

#[test]
fn an_inbox_reads_the_owners_move_as_one_and_an_agents_move_not_at_all() {
    let f = Fixture::new(Seed::default());
    let (orch, exe) = (f.session("orchestrator"), f.session("worker"));
    f.task("tools/loose");
    f.task("tools/agents");
    f.store.claim("tools/loose", &exe).unwrap();
    f.store.claim("tools/agents", &exe).unwrap();
    f.store.move_to("tools/agents", "blocked", &exe).unwrap();
    f.store
        .move_to("tools/loose", "cancelled", "owner")
        .unwrap();
    let moved = "owner moved it in_progress → cancelled";
    assert_eq!(inbox(&f, &orch), [format!("{UNOWNED}{moved}{NO_FURTHER}")]);
    assert_eq!(inbox(&f, &exe), [format!("{moved}{NO_FURTHER}")]);
    // Read once: the inbox acknowledges what it printed.
    let ids: Vec<i64> = (f.store.inbox(&exe).unwrap().iter())
        .map(|m| m.id)
        .collect();
    f.store.ack(&exe, &ids).unwrap();
    assert!(inbox(&f, &exe).is_empty());
}

#[test]
fn a_channel_carries_the_owners_word_on_a_task_nobody_orchestrates() {
    let f = Fixture::new(crate::common::Seed::default());
    let orch = f.session("orchestrator");
    let sender = f.session("worker");
    let o = listen(&f, &orch);
    let first = o.text();
    assert!(first.starts_with("PULL "), "{first}");
    f.task("tools/loose");
    o.drain(&f, &sender, &orch);
    let post = f.store.say("tools/loose", "owner", "hello", None).unwrap();
    assert_eq!(
        words(o.drain(&f, &sender, &orch)),
        [format!(
            "MSG #{} tools/loose from owner — {UNOWNED}hello",
            post.id
        )]
    );
}

#[test]
fn a_task_an_orchestrator_files_is_its_own_unless_filed_under_one_held() {
    let f = Fixture::new(Seed::default());
    let (orch, other, worker) = (
        f.session("orchestrator"),
        f.session("orchestrator"),
        f.session("worker"),
    );
    let holders = |slug: &str| f.store.get(slug).unwrap().holders("orchestrator").len();
    f.task_with(NewTask::new("tools/mine", "Mine", &orch));
    assert!(f.store.orchestrate_filed("tools/mine", &orch).unwrap());
    assert_eq!(
        f.store.get("tools/mine").unwrap().holders("orchestrator"),
        [orch.as_str()]
    );
    assert_eq!(
        f.said("tools/mine").last().unwrap(),
        &format!("{orch} joined as orchestrator")
    );
    // Filed under a task another orchestrator holds: that one's already.
    f.task_with(NewTask {
        parent: Some("tools/mine".into()),
        ..NewTask::new("tools/mine/part", "Part", &other)
    });
    assert!(
        !f.store
            .orchestrate_filed("tools/mine/part", &other)
            .unwrap()
    );
    assert_eq!(holders("tools/mine/part"), 0);
    // A worker's task, and the owner's, are left as they were.
    f.task_with(NewTask::new("tools/workers", "Theirs", &worker));
    assert!(!f.store.orchestrate_filed("tools/workers", &worker).unwrap());
    f.task("tools/owners");
    assert!(!f.store.orchestrate_filed("tools/owners", "owner").unwrap());
    assert_eq!(holders("tools/workers") + holders("tools/owners"), 0);
    // A filer the board has no session of takes none, and the task stays filed.
    assert!(!f.store.orchestrate_filed("tools/owners", "nobody").unwrap());
    assert_eq!(holders("tools/owners"), 0);
}

#[test]
fn a_claim_by_an_orchestrators_child_leaves_its_role_as_it_is() {
    let f = Fixture::new(crate::common::Seed::default());
    let orch = f.session("orchestrator");
    let kid = f.session_with(NewSession {
        parent: Some(orch.clone()),
        ..NewSession::new("worker")
    });
    f.task_with(NewTask::new("tools/mine", "Mine", &orch));
    assert!(f.store.orchestrate_filed("tools/mine", &orch).unwrap());
    f.store.claim("tools/mine", &kid).unwrap();
    let task = f.store.get("tools/mine").unwrap();
    assert_eq!(task.holders("orchestrator"), [orch.as_str()]);
    assert_eq!(task.holders("executor"), [kid.as_str()]);
}
