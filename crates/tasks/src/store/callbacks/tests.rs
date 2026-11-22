use super::*;

#[test]
fn plain_orchestrator_tie_break_does_not_depend_on_callback_row_order() {
    let callback = |handle: &str| {
        let mut row: Callback = serde_json::from_value(json!({
            "handle":handle, "address":"codex:abcdef01", "host":"test",
            "session":crate::rows::SessionRow::default()
        }))
        .unwrap();
        row.live_orchestrator = true;
        row
    };
    for rows in [
        vec![callback("zinc-otter"), callback("amber-heron")],
        vec![callback("amber-heron"), callback("zinc-otter")],
    ] {
        assert_eq!(recipient(&rows), Some("amber-heron"));
    }
}

#[test]
fn capacity_splits_between_events_and_never_loses_or_reorders_them() {
    let mut attempts = Vec::new();
    let mut delivered = Vec::new();
    deliver_batches(
        &[1, 2, 3, 4, 5],
        1.0,
        &|| 0.0,
        &mut |batch| {
            attempts.push(batch.to_vec());
            if batch.len() > 2 {
                Err(DeliveryError::capacity("full"))
            } else {
                Ok(())
            }
        },
        &mut |batch, result| {
            assert!(result.is_ok());
            delivered.extend_from_slice(batch);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(
        attempts,
        [
            vec![1, 2, 3, 4, 5],
            vec![1, 2],
            vec![3, 4, 5],
            vec![3],
            vec![4, 5]
        ]
    );
    assert_eq!(delivered, [1, 2, 3, 4, 5]);
}

#[test]
fn an_oversized_event_is_retained_but_does_not_block_smaller_events() {
    let mut recorded = Vec::new();
    deliver_batches(
        &[1, 2],
        1.0,
        &|| 0.0,
        &mut |batch| {
            if batch.contains(&1) {
                Err(DeliveryError::capacity("large"))
            } else {
                Ok(())
            }
        },
        &mut |batch, result| {
            recorded.push((batch.to_vec(), result.is_ok()));
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(recorded, [(vec![1], false), (vec![2], true)]);
}

#[test]
fn a_transport_failure_retains_the_whole_batch_without_splitting_or_retrying_it() {
    let mut attempts = 0;
    let mut recorded = Vec::new();
    deliver_batches(
        &[1, 2, 3],
        1.0,
        &|| 0.0,
        &mut |_| {
            attempts += 1;
            Err(DeliveryError::retry("short write"))
        },
        &mut |batch, result| {
            recorded.push((batch.to_vec(), result.is_ok()));
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(attempts, 1);
    assert_eq!(recorded, [(vec![1, 2, 3], false)]);
}

/// The batches a delivery tried, in order.
type Attempts = Vec<Vec<i32>>;
/// The batches it recorded, each with whether it was sent.
type Recorded = Vec<(Vec<i32>, bool)>;

/// Each attempt takes ten seconds of the clock it is handed. The oversized first event is tried
/// twice (the whole queue, then alone): the second event's turn comes at 20 s.
fn delivered_by(deadline: f64) -> (Attempts, Recorded) {
    let clock = std::cell::Cell::new(0.0);
    let (mut attempts, mut recorded) = (Vec::new(), Vec::new());
    deliver_batches(
        &[1, 2],
        deadline,
        &|| clock.get(),
        &mut |batch| {
            attempts.push(batch.to_vec());
            clock.set(clock.get() + 10.0);
            match batch.contains(&1) {
                true => Err(DeliveryError::capacity("large")),
                false => Ok(()),
            }
        },
        &mut |batch, result| {
            recorded.push((batch.to_vec(), result.is_ok()));
            Ok(())
        },
    )
    .unwrap();
    (attempts, recorded)
}

#[test]
fn no_batch_begins_at_or_past_the_deadline_by_the_clock_it_is_handed() {
    // The first event's lone attempt began at 10 s, before the deadline: it is recorded, though it
    // ended past it. The second's turn, at 20 s, is at or past the deadline: never tried, never
    // recorded, still due for the next dispatcher with its whole budget.
    for deadline in [15.0, 20.0] {
        assert_eq!(
            delivered_by(deadline),
            (vec![vec![1, 2], vec![1]], vec![(vec![1], false)]),
            "deadline {deadline}"
        );
    }
    // Before it, the second goes.
    assert_eq!(
        delivered_by(20.5),
        (
            vec![vec![1, 2], vec![1], vec![2]],
            vec![(vec![1], false), (vec![2], true)]
        )
    );
    // A deadline already past sends nothing at all.
    assert_eq!(delivered_by(0.0), (vec![], vec![]));
}

/// A plain line keeps what it is given, starts unattempted and carries no PULL: on the outbox it
/// is written in the very shape the literal it replaced wrote, `pull_said` left out.
#[test]
fn a_plain_line_is_unattempted_and_carries_no_pull() {
    let message = Event::line(
        "message-7".into(),
        "amber-wren",
        "MSG #7".into(),
        None,
        Some(7),
    );
    assert_eq!(
        serde_json::to_value(&message).unwrap(),
        json!({"key":"message-7", "handle":"amber-wren", "text":"MSG #7", "slug":null,
            "message":7, "attempts":0, "sequence":0, "pull_entries":null, "pull_withheld":null})
    );
    let line = Event::line(
        "event-3-0".into(),
        "zinc-otter",
        String::new(),
        Some("p/a".into()),
        None,
    );
    assert_eq!((line.slug.as_deref(), line.message), (Some("p/a"), None));
    assert_eq!(line.text, "");
    assert!(line.pull_entries.is_none() && line.pull_withheld.is_none());
    assert!(line.pull_said.is_none());
}

/// A board read afresh drops the pass's inbox pool, whether one was made or not, and reads the
/// board as it stands, the task filed since the last read on it.
#[test]
fn a_fresh_board_drops_the_inbox_pool_and_reads_the_board_as_it_stands() {
    let fixture = crate::store_fixture::Fixture::empty();
    let store = &fixture.store;
    let pooled = || (store.inbox_pool.lock().unwrap()).is_some();
    let (board, _) = store.fresh_board().unwrap();
    assert!(board.tasks.is_empty());
    assert!(!pooled());
    store.could_hear("amber-wren", 0, 0).unwrap();
    assert!(pooled(), "a pass's read of an inbox makes the pool");
    fixture.task("p/filed");
    let (board, sequence) = store.fresh_board().unwrap();
    assert!(!pooled());
    assert_eq!(sequence, store.board_marker().unwrap());
    assert_eq!(
        (board.tasks.iter().map(|t| t.slug.as_str())).collect::<Vec<_>>(),
        ["p/filed"]
    );
}

/// A plan's visit made again under the recipient's lease answers with the plan's PULL time, which
/// the leased pass does not move, and leaves the recipient unreached while it cannot take the
/// lease: another dispatcher holds it, or the pass delivers to a channel the recipient has none of.
#[test]
fn a_leased_visit_keeps_the_plans_pull_time_and_is_unreached_without_the_lease() {
    let fixture = crate::store_fixture::Fixture::empty();
    let store = &fixture.store;
    let handle = fixture.session("worker");
    let callback: Callback = serde_json::from_value(json!({
        "handle":handle, "address":"", "host":crate::store_fixture::HOST,
        "session":crate::rows::SessionRow::default()
    }))
    .unwrap();
    let env = crate::cli::Env::of_process();
    let (mut board, mut sequence) = store.callback_board().unwrap();
    let mut leased = |pass| {
        let visit = Visit::new(pass, &env, store, None);
        (store.leased(&handle, &callback, &mut board, &mut sequence, &visit, 42.0)).unwrap()
    };
    assert_eq!(
        leased(Pass::Observe),
        (false, 42.0),
        "no channel to deliver to"
    );
    assert_eq!(
        leased(Pass::Deliver),
        (false, 42.0),
        "no channel to deliver to"
    );
    // A reader with no channel collects: the lease is its, and the leased pass, which finds no
    // state of it, reaches it and leaves the plan's time as it was.
    assert_eq!(leased(Pass::Collect), (true, 42.0));
    assert_eq!(leased(Pass::Collect), (true, 42.0), "the lease was let go");
    let until = fixture.now() + 30.0;
    let lease = store.raw().row("callback_lease", handle.as_str());
    lease.put(json!({"until":until, "revision":9})).unwrap();
    assert_eq!(
        leased(Pass::Collect),
        (false, 42.0),
        "another holds the lease"
    );
}

/// Only an observer an older dispatcher recorded a look for, and no newer one wrote for since, is
/// upgraded: one never observed, or written for by a newer dispatcher, is not.
#[test]
fn only_a_look_no_newer_dispatcher_wrote_for_is_upgraded() {
    let looked = || {
        let empty = json!({"roles":[], "statuses":{}, "ready":{}, "children":{}});
        Some(serde_json::from_value::<attach::Seen>(empty).unwrap())
    };
    for (seen, delivery_version, upgrades) in [
        (None, None, false),
        (looked(), None, true),
        (looked(), Some(1), false),
        (None, Some(1), false),
    ] {
        let state = State {
            seen,
            delivery_version,
            ..State::default()
        };
        assert_eq!(upgrades_legacy(&state), upgrades, "{delivery_version:?}");
    }
}

/// An orchestrator's look and PULL read the holdings the visit observed, and refuse a visit that
/// observed none.
#[test]
fn a_visit_with_no_holdings_observation_has_no_holdings_to_read() {
    let fixture = crate::store_fixture::Fixture::empty();
    let env = crate::cli::Env::of_process();
    let none = Visit::new(Pass::Deliver, &env, &fixture.store, None);
    let refused = holdings(&none).map(|_| ()).unwrap_err().to_string();
    assert!(
        refused.contains("orchestrator PULL has no holdings observation"),
        "{refused}"
    );
    let observed = pull::Holdings::from([("amber-wren".into(), BTreeSet::from(["p/a".into()]))]);
    let visit = Visit::new(Pass::Deliver, &env, &fixture.store, Some(&observed));
    assert!(std::ptr::eq(holdings(&visit).unwrap(), &observed));
}

/// A due PULL to an orchestrator: its first says the whole digest and is queued to wait for its
/// acknowledgement; an observer an older dispatcher kept has its whole digest counted as told on
/// upgrade, and nothing queued; a visit with no holdings observation plans none.
#[test]
fn a_due_pull_is_queued_first_and_upgraded_for_a_legacy_observer() {
    use crate::store_fixture::{Backend, Fixture, Seed};
    let fixture = Fixture::on(Backend::SurrealDb, Seed::default().task("p/ready", "ready"));
    let store = &fixture.store;
    let handle = fixture.session("orchestrator");
    let (board, _) = store.callback_board().unwrap();
    let observed = store.slot_holdings().unwrap();
    let env = crate::cli::Env::of_process();
    let visit = Visit::new(Pass::Deliver, &env, store, Some(&observed));
    let plan = |state: State, visit: &Visit<'_>| {
        store.plan_pull(&handle, &board, visit, &mut look_over(state))
    };
    let entry = |list: &str, name: &str| (list.to_string(), name.to_string());
    let (volume, ready) = (
        entry("low_volume", &handle),
        entry("ready_unstaffed", "p/ready"),
    );
    let whole = format!(
        "PULL executing=0 ready_unstaffed=1 in_progress_orphaned=0 review_unreviewed=0 landed_open=0 idle=0\n\
         low_volume {handle} open=0 threshold=4 cap=none — pick disjoint ready work or resolve owned blockers using actual resources and ownership\n\
         ready_unstaffed p2 p/ready — Task p/ready"
    );
    let first = plan(State::default(), &visit).unwrap();
    assert_eq!(
        first.entries,
        BTreeSet::from([volume.clone(), ready.clone()])
    );
    assert_eq!(first.text, whole);
    assert!(first.said.idle.is_empty());
    assert_eq!(first.account.plan, PullPlan::Queued);
    assert_eq!(
        first.account.recorded, None,
        "nothing told before the first"
    );
    let awaiting = first.account.awaiting.as_ref().unwrap();
    assert_eq!(
        (&awaiting.entries, awaiting.withheld.len()),
        (&first.entries, 0)
    );
    // An older dispatcher's look: what it recorded counts as told, but the condition it never
    // knew, the orchestrator's low volume, which the upgrade queues alone.
    let looked = json!({"roles":[], "statuses":{}, "ready":{}, "children":{}});
    let legacy = State {
        seen: Some(serde_json::from_value(looked).unwrap()),
        ..State::default()
    };
    let upgraded = plan(legacy, &visit).unwrap();
    assert_eq!(upgraded.text, whole);
    assert_eq!(upgraded.account.plan, PullPlan::Queued);
    assert_eq!(upgraded.account.recorded, Some(BTreeSet::from([ready])));
    assert_eq!(upgraded.account.untold, BTreeSet::from([volume]));
    let none = Visit::new(Pass::Deliver, &env, store, None);
    let refused = plan(State::default(), &none).map(|_| ()).unwrap_err();
    assert!(
        refused.to_string().contains("no holdings observation"),
        "{refused}"
    );
}

/// What a recipient was sent, as events: its inbox only where the pass reads it, after its cursor
/// or, on its first observation, whole but for what is neither after the cursor nor addressed to
/// it; the owner's chat only while it holds the seat, each event once its outbox holds none.
#[test]
fn a_recipients_messages_are_its_inbox_where_read_and_the_chat_while_it_holds_the_seat() {
    use crate::store_fixture::{Fixture, Seed};
    let fixture = Fixture::new(Seed::default().task("p/a", "in_progress"));
    let store = &fixture.store;
    let (me, other) = (fixture.session("worker"), fixture.session("worker"));
    let to_me = store.say("p/a", &other, "for  you", Some(&me)).unwrap();
    let to_all = store.say("p/a", &other, "for all", None).unwrap();
    store.owner_callback("ask-1", "owner says").unwrap();
    let (board, _) = store.callback_board().unwrap();
    let mut callback: Callback = serde_json::from_value(json!({
        "handle":me, "address":"", "host":crate::store_fixture::HOST,
        "session":crate::rows::SessionRow::default()
    }))
    .unwrap();
    let events = |callback: &Callback, inbox| {
        let events = store.message_events(&me, callback, &board, inbox).unwrap();
        (events.into_iter())
            .map(|e| (e.key, e.text, e.slug, e.message))
            .collect::<Vec<_>>()
    };
    let after = |floor, first| Some(Inbox { floor, first });
    let message = (
        format!("message-{}", to_me.id),
        format!("MSG #{} p/a from {other} — for you", to_me.id),
        Some("p/a".to_string()),
        Some(to_me.id),
    );
    let chat = (
        "chat-ask-1".to_string(),
        "owner says".to_string(),
        None,
        None,
    );
    assert_eq!(events(&callback, None), [], "no inbox read, no seat");
    assert_eq!(
        events(&callback, after(0, false)),
        std::slice::from_ref(&message)
    );
    assert_eq!(
        events(&callback, after(to_me.id, false)),
        [],
        "none after its cursor"
    );
    // Read whole on its first look, what it was sent before the cursor is still its.
    assert!(to_all.id > to_me.id);
    assert_eq!(
        events(&callback, after(to_all.id, true)),
        std::slice::from_ref(&message)
    );
    callback.chat = true;
    assert_eq!(events(&callback, None), std::slice::from_ref(&chat));
    assert_eq!(events(&callback, after(0, false)), [message, chat]);
}

/// An empty look, as `attach::seen` gives one of a board with nothing on it.
fn nothing_seen() -> attach::Seen {
    serde_json::from_value(json!({"roles":[], "statuses":{}, "ready":{}, "children":{}})).unwrap()
}

/// A worker's look at an empty board, with no PULL due, over the recorded `state`: one upgraded
/// from a legacy observer where `state` is one, as [`Store::look`] reads it ([`upgrades_legacy`]).
fn look_over(state: State) -> Look {
    let seen = nothing_seen();
    Look {
        narrowing: attach::Narrowing::between(&seen, &seen),
        legacy_upgrade: upgrades_legacy(&state),
        state,
        queued: Vec::new(),
        seen: seen.clone(),
        heard: seen.clone(),
        legacy: seen,
        before: None,
        initializing: false,
        first_observation: false,
        message_floor: 0,
        statuses: BTreeMap::new(),
        rescoped: Rescoped::default(),
        agents: Agents {
            view: None,
            idle: Vec::new(),
            busy: Vec::new(),
        },
        due: false,
        pull_at: f64::INFINITY,
    }
}

/// The state that recorded exactly `look_over`'s look, its inbox read to message 7.
fn recorded() -> State {
    let seen = nothing_seen();
    State {
        narrowing: Some(attach::Narrowing::between(&seen, &seen)),
        seen: Some(seen),
        delivery_version: Some(1),
        inbox_cursor: Some(7),
        ..State::default()
    }
}

/// A look is current only while its state records it, narrowed as it is, and its inbox was read
/// to the board's newest message.
#[test]
fn a_look_is_current_while_its_state_records_it_and_its_inbox_is_read_to_the_newest_message() {
    let board = |latest_message| crate::Board {
        tasks: Vec::new(),
        sessions: Vec::new(),
        latest_message,
        workflow_generation: None,
    };
    let look = look_over(recorded());
    assert!(look.unchanged());
    assert!(look.current(&board(7)));
    assert!(
        !look.current(&board(8)),
        "a message came since its inbox was read"
    );
    let never_read = look_over(State {
        inbox_cursor: None,
        ..recorded()
    });
    assert!(never_read.unchanged() && !never_read.current(&board(7)));
    let unnarrowed = look_over(State {
        narrowing: None,
        ..recorded()
    });
    assert!(!unnarrowed.unchanged() && !unnarrowed.current(&board(7)));
    let unseen = look_over(State {
        seen: None,
        ..recorded()
    });
    assert!(!unseen.unchanged() && !unseen.current(&board(7)));
    let mut moved = look_over(recorded());
    moved.legacy.latest_message = Some(9);
    assert!(!moved.unchanged(), "its look moved on since");
}

/// A look settles, changing nothing its record holds, only with no event to queue, once a newer
/// dispatcher has written for it, with its registration observed and its statuses, its children
/// of unknown liveness, any PULL planned and the lines queued for it as recorded.
#[test]
fn a_look_settles_only_when_recording_it_changes_nothing() {
    let none = BTreeSet::new();
    let settled = |look: &Look| look.settled(&[], &none, None);
    assert!(settled(&look_over(recorded())));
    let event = Event::line("event-1-0".into(), "amber-wren", "x".into(), None, None);
    assert!(!look_over(recorded()).settled(&[event], &none, None));
    let unknown = BTreeSet::from(["zinc-otter".to_string()]);
    assert!(!look_over(recorded()).settled(&[], &unknown, None));
    let mut initializing = look_over(recorded());
    initializing.initializing = true;
    assert!(!settled(&initializing));
    let registered = State {
        registration_revision: 2,
        ..recorded()
    };
    assert!(!settled(&look_over(registered)));
    let mut statuses = look_over(recorded());
    statuses.statuses = BTreeMap::from([("p/a".into(), "ready".into())]);
    assert!(!settled(&statuses));
    let mut rescoped = look_over(recorded());
    rescoped.rescoped.dropped.push("event-1-0".into());
    assert!(!settled(&rescoped));
    let pulled = |plan| Pulled {
        text: String::new(),
        said: pull::Said::default(),
        account: Account {
            plan,
            queued: None,
            recorded: None,
            unheard: BTreeSet::new(),
            untold: BTreeSet::new(),
            withheld: BTreeSet::new(),
            awaiting: None,
        },
        entries: BTreeSet::new(),
    };
    let look = look_over(recorded());
    assert!(look.settled(&[], &none, Some(&pulled(PullPlan::Unchanged))));
    assert!(!look.settled(&[], &none, Some(&pulled(PullPlan::Dropped))));
}

/// The lines of a look's change are keyed in order under the lease's revision, each about the
/// task the recipient holds a role on, or none.
#[test]
fn a_looks_lines_are_keyed_in_order_under_the_lease_and_name_the_recipients_task() {
    use crate::store_fixture::{Fixture, Seed};
    let fixture = Fixture::new(
        Seed::default()
            .session("amber-wren")
            .task("p/a", "in_progress")
            .role("p/a", "amber-wren", "executor"),
    );
    let (board, _) = fixture.store.callback_board().unwrap();
    let lines = vec!["CLAIMED p/a".to_string(), "MOVED p/a".to_string()];
    let events = line_events("amber-wren", &board, lines.clone(), 4);
    let shown: Vec<_> = (events.iter())
        .map(|e| {
            (
                e.key.as_str(),
                e.text.as_str(),
                e.slug.as_deref(),
                e.message,
            )
        })
        .collect();
    assert_eq!(
        shown,
        [
            ("event-4-0", "CLAIMED p/a", Some("p/a"), None),
            ("event-4-1", "MOVED p/a", Some("p/a"), None)
        ]
    );
    let elsewhere = line_events("zinc-otter", &board, lines, 4);
    assert!(elsewhere.iter().all(|event| event.slug.is_none()));
    assert!(line_events("amber-wren", &board, Vec::new(), 4).is_empty());
}

/// What a look hears changed: nothing on its first look, nor for an observer it upgrades from a
/// legacy one, which is baselined, nor while what it hears is what it was told, however the whole
/// look moved. Once what it hears moved, each line of the whole look's change, a finished task's
/// move among them, its whitespace run together.
#[test]
fn a_looks_changed_lines_are_none_until_what_it_hears_moves_and_then_the_whole_looks() {
    let fixture = crate::store_fixture::Fixture::empty();
    let board = crate::Board {
        tasks: Vec::new(),
        sessions: Vec::new(),
        latest_message: 0,
        workflow_generation: None,
    };
    let me = "amber-wren";
    // A look at the tasks of `statuses`, each `me`'s to execute, and the ready ones of `ready`.
    let seen = |statuses: &[(&str, &str)], ready: &[(&str, &str)]| {
        let mut seen = nothing_seen();
        for &(slug, status) in statuses {
            seen.statuses.insert(slug.into(), status.into());
            seen.roles
                .insert((slug.into(), me.into(), "executor".into()));
        }
        seen.ready = (ready.iter())
            .map(|&(slug, title)| (slug.into(), title.into()))
            .collect();
        seen
    };
    let title = [("p/n", "Two  spaced   words")];
    let told = seen(&[("p/a", "ready"), ("p/b", "in_progress")], &[]);
    let whole = seen(&[("p/a", "in_progress"), ("p/b", "done")], &title);
    // What it hears of the whole look: the open tasks alone, as `Store::look` compacts it.
    let heard = seen(&[("p/a", "in_progress")], &title);
    let lines = |before: Option<&attach::Seen>, heard: &attach::Seen, upgrade: bool| {
        let mut look = look_over(recorded());
        look.before = before.cloned();
        (look.seen, look.heard) = (whole.clone(), heard.clone());
        look.legacy_upgrade = upgrade;
        fixture.store.changed_lines(me, &board, &look).unwrap()
    };
    let none: [&str; 0] = [];
    assert_eq!(lines(None, &heard, false), none, "its first look");
    assert_eq!(lines(Some(&told), &heard, true), none, "upgraded");
    assert_eq!(lines(Some(&told), &told, false), none, "it hears no move");
    assert_eq!(
        lines(Some(&told), &heard, false),
        [
            "STATUS p/a ready → in_progress",
            "STATUS p/b in_progress → done",
            "NEW p/n ready — Two spaced words",
        ]
    );
}

/// A planned PULL is an event only when its account queues a new one, keyed by the lease's
/// revision and carrying what it records and says.
#[test]
fn a_planned_pull_is_an_event_only_when_its_account_queues_one() {
    let board = crate::Board {
        tasks: Vec::new(),
        sessions: Vec::new(),
        latest_message: 0,
        workflow_generation: None,
    };
    let entry = ("ready_unstaffed".to_string(), "p/a".to_string());
    let withheld = ("ready_unstaffed".to_string(), "p/b".to_string());
    let awaiting = super::super::callback_state::Awaiting {
        entries: BTreeSet::from([entry.clone(), withheld.clone()]),
        withheld: BTreeSet::from([withheld]),
    };
    let pulled = |plan, awaiting| Pulled {
        text: "PULL ready_unstaffed=1".into(),
        said: pull::Said {
            entries: BTreeSet::from([entry.clone()]),
            ..pull::Said::default()
        },
        account: Account {
            plan,
            queued: None,
            recorded: None,
            unheard: BTreeSet::new(),
            untold: BTreeSet::new(),
            withheld: BTreeSet::new(),
            awaiting,
        },
        entries: BTreeSet::new(),
    };
    let queued = pulled(PullPlan::Queued, Some(awaiting.clone()));
    let event = queued.event("amber-wren", &board, 6).unwrap();
    assert_eq!(
        (event.key.as_str(), event.text.as_str()),
        ("pull-6", "PULL ready_unstaffed=1")
    );
    assert_eq!((event.slug, event.message, event.attempts), (None, None, 0));
    assert_eq!(event.pull_entries, Some(awaiting.entries.clone()));
    assert_eq!(event.pull_withheld, Some(awaiting.withheld.clone()));
    assert_eq!(event.pull_said, Some(queued.said.clone()));
    assert!(
        pulled(PullPlan::Queued, None)
            .event("amber-wren", &board, 6)
            .is_none()
    );
    let kept = pulled(PullPlan::Kept("pull-2".into()), Some(awaiting));
    assert!(kept.event("amber-wren", &board, 6).is_none());
    assert!(
        pulled(PullPlan::Unchanged, None)
            .event("amber-wren", &board, 6)
            .is_none()
    );
}

/// A recipient's stored state is there to observe once it has one; a write's observation leaves
/// one recorded at or past the board it holds, and a state recorded past that board, or before
/// observations were sequenced, has the board read again.
#[test]
fn a_stored_state_is_observed_against_a_board_at_least_as_new_as_it() {
    let fixture = crate::store_fixture::Fixture::empty();
    let store = &fixture.store;
    let handle = fixture.session("worker");
    let callback: Callback = serde_json::from_value(json!({
        "handle":handle, "address":"", "host":crate::store_fixture::HOST,
        "session":crate::rows::SessionRow::default()
    }))
    .unwrap();
    let env = crate::cli::Env::of_process();
    let (mut board, marker) = store.callback_board().unwrap();
    let mut read = |pass, sequence: u64| {
        let visit = Visit::new(pass, &env, store, None);
        let mut sequence = sequence;
        let state = store.stored_state(&handle, &callback, &mut board, &mut sequence, &visit);
        (
            state.unwrap().map(|(state, _, _)| state.snapshot_sequence),
            sequence,
        )
    };
    assert_eq!(read(Pass::Deliver, marker), (None, marker), "no state yet");
    let state = store.raw().row("callback_state", handle.as_str());
    state.put(json!({"snapshot_sequence":marker})).unwrap();
    assert_eq!(
        read(Pass::Observe, marker),
        (None, marker),
        "observed at this board"
    );
    assert_eq!(read(Pass::Deliver, marker), (Some(marker), marker));
    let behind = marker - 1;
    assert_eq!(
        read(Pass::Observe, behind),
        (None, behind),
        "observed past it"
    );
    assert_eq!(
        read(Pass::Deliver, behind),
        (Some(marker), marker),
        "read again"
    );
    let unsequenced = json!({"snapshot_sequence":0, "seen":nothing_seen()});
    state.put(unsequenced).unwrap();
    let now = store.board_marker().unwrap();
    assert_eq!(
        read(Pass::Observe, 0),
        (Some(0), now),
        "unsequenced, read again"
    );
}

/// A visit with nothing to observe reaches its recipient at once, takes no lease and has no PULL
/// due: one with no state yet, and one whose state stands for a session no longer here.
#[test]
fn a_visit_with_nothing_to_observe_reaches_its_recipient_and_takes_no_lease() {
    let fixture = crate::store_fixture::Fixture::empty();
    let store = &fixture.store;
    let here = fixture.session("worker");
    let env = crate::cli::Env::of_process();
    let (mut board, marker) = store.callback_board().unwrap();
    let mut visit = |handle: &str| {
        let callback: Callback = serde_json::from_value(json!({
            "handle":handle, "address":"", "host":crate::store_fixture::HOST,
            "session":crate::rows::SessionRow::default()
        }))
        .unwrap();
        let visit = Visit::new(Pass::Deliver, &env, store, None);
        let mut sequence = marker;
        (store.dispatch_one(handle, &callback, &mut board, &mut sequence, &visit)).unwrap()
    };
    assert_eq!(visit(&here), (true, f64::INFINITY), "no state yet");
    let gone = "zinc-otter";
    let state = store.raw().row("callback_state", gone);
    state.put(json!({"snapshot_sequence":marker})).unwrap();
    assert_eq!(visit(gone), (true, f64::INFINITY), "no session here");
    for handle in [here.as_str(), gone] {
        let lease = store.raw().row("callback_lease", handle);
        assert!(!lease.exists().unwrap(), "{handle} took no lease");
    }
}

/// The look kept for older dispatchers says each child of unknown liveness as the board does: one
/// no longer on it has left. Every other child keeps its health, and only the unknown are named.
#[test]
fn a_stored_look_says_the_unknown_children_as_the_board_does() {
    let fixture = crate::store_fixture::Fixture::empty();
    let store = &fixture.store;
    let here = fixture.session("worker");
    let (board, _) = store.callback_board().unwrap();
    let mut legacy = nothing_seen();
    legacy.children = BTreeMap::from([
        (here.clone(), crate::Health::Unknown),
        ("zinc-otter".into(), crate::Health::Unknown),
        ("amber-wren".into(), crate::Health::Stale),
    ]);
    let (stored, unknown) = store.stored_look(&board, &legacy);
    assert_eq!(
        unknown,
        BTreeSet::from([here.clone(), "zinc-otter".to_string()])
    );
    assert_eq!(stored.children["zinc-otter"], crate::Health::Left);
    assert_eq!(stored.children["amber-wren"], crate::Health::Stale);
    assert_eq!(stored.children[&here], fixture.health(&here));
}
