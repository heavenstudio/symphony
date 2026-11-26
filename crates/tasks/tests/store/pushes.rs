//! What reaches an orchestrator's socket: what it cannot predict and must act on, said once. A PULL
//! says what changed since the last one it received; it never hears its own writes, nor the roles
//! its own children take or let go; a child going quiet is said once per transition. A session
//! with no channel reads the same lines by waiting (`Store::collect_callbacks`).

use super::callbacks::Listener;
use super::common::Fixture;
use super::quiet_callbacks::listen;
use symphony_board::cli::Env;
use symphony_board::health::{LIVE_SECONDS, STALE_SECONDS};
use symphony_board::{Error, NewSession, NewTask};

/// An orchestrator heard on a socket, past its first PULL.
fn orchestrator(f: &Fixture) -> (String, Listener) {
    let orch = f.session("orchestrator");
    let listener = listen(f, &orch);
    let first = listener.text();
    assert!(first.starts_with("PULL "), "{first}");
    (orch, listener)
}

/// A session `parent` registered.
fn child(f: &Fixture, parent: &str) -> String {
    f.session_with(NewSession {
        parent: Some(parent.to_owned()),
        ..NewSession::new("worker")
    })
}

#[test]
fn a_backlog_left_unstaffed_is_not_said_again_when_it_gains_a_row() {
    let f = Fixture::new(crate::common::Seed::default());
    let sender = f.session("worker");
    for n in 0..33 {
        f.task(&format!("tools/backlog-{n:02}"));
    }
    let (orch, o) = orchestrator(&f);
    f.task("tools/new");
    assert_eq!(
        o.drain(&f, &sender, &orch),
        [
            "NEW tools/new ready — Jump departure",
            "PULL ready_unstaffed=34 (was 33)",
            "ready_unstaffed p2 tools/new — Jump departure",
        ]
    );
    // An unchanged board sends nothing, at any age.
    for age in [601.0, 86_400.0] {
        f.advance(age);
        for handle in [&orch, &sender] {
            f.store.touch(handle).unwrap();
        }
        f.store.dispatch_callbacks().unwrap();
        assert!(o.drain(&f, &sender, &orch).is_empty());
    }
}

#[test]
fn what_its_own_children_take_on_and_let_go_is_not_pushed_but_their_moves_are() {
    let f = Fixture::new(crate::common::Seed::default());
    let (orch, o) = orchestrator(&f);
    let kid = child(&f, &orch);
    let stranger = f.session("worker");
    for slug in ["tools/kid", "tools/stranger"] {
        f.task(slug);
        f.store.take_role(slug, &orch, "orchestrator").unwrap();
    }
    o.drain(&f, &stranger, &orch);
    // Its child reserves and claims the work it was launched for: no ROLE, no STATUS.
    f.store.reserve("tools/kid", &kid, 600.0).unwrap();
    f.store.claim("tools/kid", &kid).unwrap();
    assert!(o.drain(&f, &stranger, &orch).is_empty());
    // A session it did not register claiming its work is news.
    f.store.claim("tools/stranger", &stranger).unwrap();
    assert_eq!(
        o.drain(&f, &stranger, &orch),
        ["ROLE tools/stranger ".to_owned() + &stranger + " executor, in_progress"]
    );
    // Its child lets the work go: no ROLE, STATUS or NEW names the release; the PULL says the row
    // is back, with what moved since the last PULL it received, which listed both tasks ready.
    f.store.release("tools/kid", &kid, None).unwrap();
    assert_eq!(
        o.drain(&f, &stranger, &orch),
        [
            "PULL executing=1 (was 0) ready_unstaffed=1 (was 2)",
            "ready_unstaffed p2 tools/kid — Jump departure",
            "resolved ready_unstaffed tools/stranger",
        ]
    );
    // A move the child makes on its own is still said: work in review needs a reviewer.
    f.store.claim("tools/kid", &kid).unwrap();
    o.drain(&f, &stranger, &orch);
    f.store.move_to("tools/kid", "review", &kid).unwrap();
    let told = o.drain(&f, &stranger, &orch);
    assert_eq!(told[0], "STATUS tools/kid in_progress → review", "{told:?}");
}

#[test]
fn a_pull_names_every_unanswered_post_it_tells_however_many() {
    let f = Fixture::new(crate::common::Seed::default());
    let (orch, o) = orchestrator(&f);
    let barrier = f.session("worker");
    // Eight executors, each sent a post, all gone quiet past the half hour a post waits.
    let posts: Vec<i64> = (0..8)
        .map(|n| {
            let slug = format!("tools/gap-{n}");
            f.task(&slug);
            f.store.claim(&slug, &f.session("worker")).unwrap();
            let post = f.store.say(&slug, "owner", "continue", Some("@executor"));
            post.unwrap().id
        })
        .collect();
    o.drain(&f, &barrier, &orch);
    f.advance(31.0 * 60.0);
    for handle in [&orch, &barrier] {
        f.store.touch(handle).unwrap();
    }
    f.store.dispatch_callbacks().unwrap();
    let told = o.drain(&f, &barrier, &orch);
    // Each line names a post and the command that answers it: none is cut, since each is told once.
    let unanswered: Vec<&String> = (told.iter())
        .filter(|line| line.starts_with("unanswered "))
        .collect();
    assert_eq!(unanswered.len(), posts.len(), "{told:?}");
    for id in posts {
        let named = format!(" post #{id} — ");
        assert!(unanswered.iter().any(|l| l.contains(&named)), "{told:?}");
    }
    assert!(!told.iter().any(|l| l.contains("unanswered …")), "{told:?}");
}

#[test]
fn its_own_move_is_never_echoed_to_it() {
    let f = Fixture::new(crate::common::Seed::default());
    let (orch, o) = orchestrator(&f);
    let other = f.session("orchestrator");
    for slug in ["tools/mine", "tools/theirs"] {
        f.task(slug);
        f.store.take_role(slug, &orch, "orchestrator").unwrap();
    }
    o.drain(&f, &other, &orch);
    f.store.move_to("tools/mine", "blocked", &orch).unwrap();
    assert!(o.drain(&f, &other, &orch).is_empty());
    f.store
        .take_role("tools/theirs", &other, "reviewer")
        .unwrap();
    o.drain(&f, &other, &orch);
    f.store.move_to("tools/theirs", "blocked", &other).unwrap();
    assert_eq!(
        o.drain(&f, &other, &orch),
        ["STATUS tools/theirs ready → blocked"]
    );
}

#[test]
fn a_move_of_the_orchestrator_role_is_said_to_the_other_side_only() {
    let f = Fixture::new(crate::common::Seed::default());
    let (orch, o) = orchestrator(&f);
    let (other, p) = orchestrator(&f);
    let worker = f.session("worker");
    f.task("tools/moved");
    f.store
        .take_role("tools/moved", &orch, "orchestrator")
        .unwrap();
    o.drain(&f, &worker, &orch);
    p.drain(&f, &worker, &other);
    // It hands the task on: the one it reached hears it, and it does not.
    f.store.hand_over("tools/moved", &orch, &other).unwrap();
    let handed = format!("ROLE tools/moved {other} orchestrator, ready (from {orch})");
    assert_eq!(o.drain(&f, &worker, &orch), Vec::<String>::new());
    assert_eq!(p.drain(&f, &worker, &other), [handed]);
    // The holder goes quiet and it takes the task over: its own take-over is not said to it either.
    f.advance(LIVE_SECONDS + STALE_SECONDS + 5.0);
    for handle in [&orch, &worker] {
        f.store.touch(handle).unwrap();
    }
    f.store.dispatch_callbacks().unwrap();
    o.drain(&f, &worker, &orch);
    f.store
        .take_role("tools/moved", &orch, "orchestrator")
        .unwrap();
    let thread = f.store.thread("tools/moved").unwrap();
    let took = format!("{orch} took orchestrator over from {other} (");
    assert!(
        thread.iter().any(|m| m.text.starts_with(&took)),
        "{thread:?}"
    );
    assert_eq!(o.drain(&f, &worker, &orch), Vec::<String>::new());
}

#[test]
fn a_quiet_child_is_said_stale_once_a_transition_not_once_a_pass() {
    let f = Fixture::new(crate::common::Seed::default());
    let (orch, o) = orchestrator(&f);
    let kid = child(&f, &orch);
    let sender = f.session("worker");
    f.task_with(NewTask::new("tools/held", "Held", "owner"));
    f.store.claim("tools/held", &kid).unwrap();
    o.drain(&f, &sender, &orch);
    let stale = format!("STALE {kid} — no sign of it for 90 s");
    let quiet = |seconds: f64| {
        let mut told = Vec::new();
        for _ in 0..3 {
            f.advance(seconds / 3.0);
            for handle in [&orch, &sender] {
                f.store.touch(handle).unwrap();
            }
            f.store.dispatch_callbacks().unwrap();
            told.extend(o.drain(&f, &sender, &orch));
        }
        told
    };
    // Three passes see it quiet: one STALE.
    assert_eq!(quiet(LIVE_SECONDS + 30.0), [stale.as_str()]);
    // It shows a sign and goes quiet again: that is a new transition, said once more.
    f.store.touch(&kid).unwrap();
    f.store.dispatch_callbacks().unwrap();
    assert!(o.drain(&f, &sender, &orch).is_empty());
    assert_eq!(quiet(LIVE_SECONDS + 30.0), [stale.as_str()]);
}

/// The lines a collecting look printed: each text is one delivery's.
fn read(f: &Fixture, handle: &str) -> Vec<String> {
    let mut said: Vec<String> = Vec::new();
    let mut print = |text: &str| {
        said.extend(text.lines().map(str::to_owned));
        Ok(())
    };
    let (printed, _) = (f.store)
        .collect_callbacks(handle, &Env::of_process(), &mut print)
        .unwrap();
    assert_eq!(printed > 0, !said.is_empty(), "{said:?}");
    said
}

/// The requests and, of them, the writes `f`'s board was sent while `act` ran.
pub(super) fn sent(f: &Fixture, act: impl FnOnce()) -> (u64, u64) {
    let log = f.store.database().queries();
    let counts = || {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
        let (queries, writes) = log.report(now.unwrap().as_secs_f64());
        (queries.count, writes.count)
    };
    let before = counts();
    act();
    let after = counts();
    (after.0 - before.0, after.1 - before.1)
}

#[test]
fn a_waiting_session_looking_at_a_board_that_stood_still_writes_nothing() {
    let f = Fixture::new(crate::common::Seed::default());
    let reader = f.session("orchestrator");
    for slug in ["tools/one", "tools/two"] {
        f.task(slug);
    }
    // The first look makes its state row and puts it on the waiting list, and reads the digest.
    let (_, first) = sent(&f, || assert!(!read(&f, &reader).is_empty()));
    assert!(first > 0, "{first}");
    // After it, a look while the board stands still only reads, the one after its beat too; and
    // `wait`'s other requests are one read of the board's marker a poll and one write a beat.
    let looks = 4;
    let began = std::time::Instant::now();
    let (requests, writes) = sent(&f, || {
        for _ in 0..looks {
            assert!(read(&f, &reader).is_empty());
        }
    });
    let took = began.elapsed() / looks as u32;
    eprintln!("a look at a board that stood still: {requests} requests, {took:?} each");
    assert_eq!(writes, 0, "{requests} requests");
    assert!(requests <= looks * LOOK_REQUESTS, "{requests} requests");
    let poll = sent(&f, || {
        f.store.board_mark().unwrap();
    });
    assert_eq!(poll, (1, 0));
    assert_eq!(sent(&f, || f.store.touch(&reader).unwrap()), (1, 1));
    let (requests, writes) = sent(&f, || assert!(read(&f, &reader).is_empty()));
    assert_eq!(writes, 0, "{requests} requests");
    assert!(requests <= LOOK_REQUESTS, "{requests} requests");
}

/// The most requests one look of `tasks wait` sends while the board stands still, all reads:
/// nine, measured on 2026-11-11.
pub(super) const LOOK_REQUESTS: u64 = 9;

#[test]
fn a_session_with_no_channel_reads_what_a_channel_would_carry_once() {
    let f = Fixture::new(crate::common::Seed::default());
    let (heard, o) = orchestrator(&f);
    let reader = f.session("orchestrator");
    let sender = f.session("worker");
    // Its first read is the whole digest, as a channel's first PULL is.
    let first = read(&f, &reader);
    assert!(first[0].starts_with("PULL "), "{first:?}");
    assert!(read(&f, &reader).is_empty());
    // A new task: the reader reads, line for line, what the channel carried.
    f.task("tools/new");
    let pushed = o.drain(&f, &sender, &heard);
    assert!(pushed[0].starts_with("NEW tools/new "), "{pushed:?}");
    assert_eq!(read(&f, &reader), pushed);
    // It orchestrates two tasks: its own child staffing one is not said to it, a stranger claiming
    // the other is, once.
    let kid = child(&f, &reader);
    for slug in ["tools/kid", "tools/stranger"] {
        f.task(slug);
        f.store.take_role(slug, &reader, "orchestrator").unwrap();
    }
    read(&f, &reader);
    f.store.reserve("tools/kid", &kid, 600.0).unwrap();
    f.store.claim("tools/kid", &kid).unwrap();
    assert!(read(&f, &reader).is_empty());
    f.store.claim("tools/stranger", &sender).unwrap();
    assert_eq!(
        read(&f, &reader),
        [format!(
            "ROLE tools/stranger {sender} executor, in_progress"
        )]
    );
    assert!(read(&f, &reader).is_empty());
}

#[test]
fn a_childs_release_does_not_hide_the_move_it_made_before_it() {
    let f = Fixture::new(crate::common::Seed::default());
    let reader = f.session("orchestrator");
    let kid = child(&f, &reader);
    f.task("tools/kid");
    f.store
        .take_role("tools/kid", &reader, "orchestrator")
        .unwrap();
    read(&f, &reader);
    f.store.claim("tools/kid", &kid).unwrap();
    assert!(read(&f, &reader).is_empty(), "its child's claim");
    // Between two looks its child blocks the work, then lets it go: the release made no move of
    // its own, so the block is said.
    f.store.move_to("tools/kid", "blocked", &kid).unwrap();
    f.store.release("tools/kid", &kid, None).unwrap();
    assert_eq!(
        read(&f, &reader),
        ["STATUS tools/kid in_progress → blocked"]
    );
    // A claim and a release that each make the move themselves still say nothing.
    f.store.move_to("tools/kid", "ready", &reader).unwrap();
    read(&f, &reader);
    f.store.claim("tools/kid", &kid).unwrap();
    f.store.release("tools/kid", &kid, None).unwrap();
    assert!(read(&f, &reader).is_empty());
}

const VERDICT: &str = "OWNER accepted card 7 in docs/DECISIONS.md — land it";

#[test]
fn a_waiting_orchestrator_holds_the_vacant_chat_seat() {
    let f = Fixture::new(crate::common::Seed::default());
    let reader = f.session("orchestrator");
    read(&f, &reader);
    assert_eq!(f.store.chat_recipient().unwrap(), Some(reader.clone()));
    assert_eq!(
        f.store.chat_seat().unwrap(),
        Some((reader.clone(), symphony_board::ChatSeat::Waiting))
    );
    f.store.owner_callback("asks-1", VERDICT).unwrap();
    assert_eq!(read(&f, &reader), [VERDICT]);
    assert!(read(&f, &reader).is_empty(), "said once");
    // Once received, no orchestrator reads it again on demand.
    let other = f.session("orchestrator");
    assert!(f.store.unheard_owner_events(&other).unwrap().is_empty());
}

#[test]
fn a_channel_holds_the_chat_seat_before_a_waiting_orchestrator() {
    let f = Fixture::new(crate::common::Seed::default());
    let reader = f.session("orchestrator");
    read(&f, &reader);
    let (heard, o) = orchestrator(&f);
    assert_eq!(f.store.chat_recipient().unwrap(), Some(heard.clone()));
    f.store.owner_callback("asks-1", VERDICT).unwrap();
    assert_eq!(o.drain(&f, &reader, &heard), [VERDICT]);
    assert!(read(&f, &reader).is_empty());
}

#[test]
fn registering_a_channel_takes_a_waiter_off_the_waiting_list() {
    let f = Fixture::new(crate::common::Seed::default());
    let reader = f.session("orchestrator");
    read(&f, &reader);
    let _channel = listen(&f, &reader);
    f.store.callback(&reader, None, false, false).unwrap();
    // Neither a channel nor waiting: nobody holds the seat until it waits again.
    assert_eq!(f.store.chat_recipient().unwrap(), None);
    read(&f, &reader);
    assert_eq!(f.store.chat_recipient().unwrap(), Some(reader));
}

#[test]
fn what_the_owner_says_to_nobody_is_read_by_every_orchestrator_until_one_takes_it() {
    let f = Fixture::new(crate::common::Seed::default());
    let (first, second) = (f.session("orchestrator"), f.session("orchestrator"));
    let worker = f.session("worker");
    f.store.owner_callback("asks-1", VERDICT).unwrap();
    assert_eq!(f.store.chat_recipient().unwrap(), None);
    let unheard = vec![("chat-asks-1".to_string(), VERDICT.to_string())];
    for handle in [&first, &second] {
        assert_eq!(f.store.unheard_owner_events(handle).unwrap(), unheard);
    }
    assert!(f.store.unheard_owner_events(&worker).unwrap().is_empty());
    // An orchestrator's inbox takes it: it is received, and read on demand by nobody after.
    let inbox = f.store.callback_inbox(&first).unwrap();
    assert_eq!(inbox, unheard);
    let keys: Vec<String> = inbox.into_iter().map(|(key, _)| key).collect();
    f.store.ack_callbacks(&first, &keys).unwrap();
    assert!(f.store.unheard_owner_events(&second).unwrap().is_empty());
    assert!(f.store.callback_inbox(&second).unwrap().is_empty());
}

/// Whether `lines` are a whole digest: its header, first, names every count, changed or not.
pub(super) fn whole(lines: &[String]) -> bool {
    lines.first().is_some_and(|header| {
        header.starts_with("PULL ") && header.contains(" executing=") && header.contains(" idle=")
    })
}

#[test]
fn a_waiter_that_registers_a_channel_hears_the_whole_digest_there_first() {
    let f = Fixture::new(crate::common::Seed::default());
    let reader = f.session("orchestrator");
    assert!(whole(&read(&f, &reader)));
    f.task("tools/new");
    read(&f, &reader);
    let o = listen(&f, &reader);
    let first: Vec<String> = o.text().lines().map(str::to_owned).collect();
    assert!(whole(&first), "{first:?}");
    assert!(first.contains(&"ready_unstaffed p2 tools/new — Jump departure".to_string()));
}

#[test]
fn a_channel_another_process_listens_on_starts_from_the_whole_digest() {
    for elsewhere in [false, true] {
        let f = Fixture::new(crate::common::Seed::default());
        let (orch, o) = orchestrator(&f);
        let barrier = f.session("worker");
        f.task("tools/told");
        o.drain(&f, &barrier, &orch);
        if elsewhere {
            // The registration as a listener in another process made it.
            let owner = serde_json::json!({"owner": {"pid": 1, "started": "before"}});
            let other = (f.store.raw().row("callback_state", orch.as_str())).set(owner);
            other.unwrap();
        }
        let moved = Listener::new(&format!("{}-moved", f.store.database().database()));
        let address = format!("uds:{}", moved.path.display());
        f.store
            .callback(&orch, Some(&address), false, false)
            .unwrap();
        let first = moved.drain(&f, &barrier, &orch);
        if elsewhere {
            assert!(whole(&first), "{first:?}");
            let told = "ready_unstaffed p2 tools/told — Jump departure";
            assert!(first.iter().any(|line| line == told), "{first:?}");
        } else {
            // The same listener on a new socket keeps what it was told.
            assert!(first.is_empty(), "{first:?}");
        }
    }
}

#[test]
fn a_channel_turned_off_starts_the_wait_from_the_whole_digest_but_a_told_post() {
    let f = Fixture::new(crate::common::Seed::default());
    let (orch, o) = orchestrator(&f);
    let barrier = f.session("worker");
    f.task("tools/gap");
    // Its orchestrator holds it, so the look below has nothing to give.
    f.store
        .take_role("tools/gap", &orch, "orchestrator")
        .unwrap();
    f.store.claim("tools/gap", &f.session("worker")).unwrap();
    let post = (f.store)
        .say("tools/gap", "owner", "continue", Some("@executor"))
        .unwrap();
    o.drain(&f, &barrier, &orch);
    f.advance(31.0 * 60.0);
    for handle in [&orch, &barrier] {
        f.store.touch(handle).unwrap();
    }
    f.store.dispatch_callbacks().unwrap();
    let named = format!("unanswered tools/gap post #{} — ", post.id);
    let told = o.drain(&f, &barrier, &orch);
    assert!(told.iter().any(|l| l.starts_with(&named)), "{told:?}");
    f.store.callback(&orch, None, false, false).unwrap();
    let again = read(&f, &orch);
    assert!(whole(&again), "{again:?}");
    assert!(again[0].contains(" unanswered=1 "), "{again:?}");
    assert!(
        !again.iter().any(|l| l.starts_with("unanswered ")),
        "{again:?}"
    );
}

#[test]
fn a_session_with_a_channel_or_gone_cannot_read_by_waiting() {
    let f = Fixture::new(crate::common::Seed::default());
    let (heard, _o) = orchestrator(&f);
    let env = Env::of_process();
    let refused = |handle: &str| match f.store.collect_callbacks(handle, &env, &mut |_| Ok(())) {
        Err(Error::Refused(why)) => why,
        other => panic!("{handle}: {other:?}"),
    };
    let why = refused(&heard);
    assert!(
        why.ends_with(&format!(
            "; read it there, or run just tasks --database {} callback --off --as {heard} first",
            f.other_database().database()
        )),
        "{why}"
    );
    let gone = f.session("orchestrator");
    f.store.leave(&gone).unwrap();
    assert_eq!(
        refused(&gone),
        format!("no session {gone} here to wait for")
    );
    let never = "quiet-never";
    assert_eq!(
        refused(never),
        format!("no session {never} here to wait for")
    );
}
