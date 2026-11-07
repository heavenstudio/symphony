//! A close or a restore whose hold of the session lapsed and was taken over writes nothing: the hold
//! is the fencing token of each of its writes (`Store::take_session`). A restore that loses it after
//! starting a terminal kills that terminal, as it does when a launch replaced its record meanwhile.
//! And a handle's registrations are numbered once, from the clock, so a retired one's number never
//! comes back, not even one numbered before the count.

use crate::close::restore_line;
use crate::close_holds::restoring;
use crate::close_races::{after, ahead_of, record};
use crate::common::refused;
use crate::{Hooked, World};
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};
use symphony_board::launch::hc::Hc;
use symphony_board::launch::{Started, restore};
use symphony_board::{Error, NewLaunch};

/// `handle`'s session on sim/jump restored, through `hc`.
fn restoring_through(world: &World, hc: &dyn Hc, handle: &str) -> Result<Vec<Started>, Error> {
    restore(
        world.store(),
        hc,
        &world.machine(),
        "sim/jump",
        Some(handle),
        None,
    )
}

/// What a write of `handle`'s close or restore is told once another took its session over.
fn taken(handle: &str) -> String {
    format!("another close or a restore of {handle} took its session over")
}

/// What sim/jump's thread says of a restore of `handle` into `terminal`.
fn restored(handle: &str, terminal: &str) -> String {
    format!("restored {handle} in pane {terminal}")
}

#[test]
fn a_close_of_a_gone_terminal_that_lost_its_hold_leaves_the_restore_that_took_it_over_alone() {
    // Its record still open, then one a close stopped before retiring the callback recorded
    // closed; and the hc call after which the close knows the terminal is gone.
    for (closed_first, call, nth) in [(false, "get", 1), (true, "ls", 2)] {
        let world = World::new();
        let session = world.waiting_reviewer();
        let handle = &session.handle;
        world.edit_terminals(|all| all.clear());
        if closed_first {
            let store = world.store();
            let hold = store
                .take_session(handle, "a close by amber-heron")
                .unwrap();
            let why = "closed by amber-heron";
            store
                .close_task_session_as(&session, why, &restore_line(handle), hold)
                .unwrap();
            store.release_session(handle, hold).unwrap();
        }
        let new = RefCell::new(String::new());
        // Its hold lapses there: a restore takes the session over, and its agent registers its
        // thread again from no terminal, as the close read it.
        let racing = after(&world, call, nth, || {
            world.f.advance(300.0);
            let [again] = <[_; 1]>::try_from(restoring(&world, handle).unwrap()).unwrap();
            *new.borrow_mut() = again.row.pane;
            world
                .store()
                .callback_in(handle, Some("codex:0abc-12"), false, false, None)
                .unwrap();
        });
        let closed = world.close_through(&racing, handle);
        let new = new.take();
        assert_eq!(refused(closed), taken(handle), "{call}");
        let now = record(&world);
        assert_eq!(
            (now.closed, now.pane.as_str(), world.has_terminal(&new)),
            (None, new.as_str(), true),
            "{call}"
        );
        assert_eq!(
            world.f.said("sim/jump").last(),
            Some(&restored(handle, &new))
        );
        let callbacks = world.store().callbacks().unwrap();
        let registered = callbacks.get(handle).map(|callback| &callback.terminal);
        assert_eq!(registered, Some(&None), "{call}");
    }
}

#[test]
fn a_found_close_that_lost_its_hold_after_its_kill_leaves_the_restored_record_open() {
    let world = World::new();
    let session = world.waiting_reviewer();
    let handle = &session.handle;
    world.close(handle).unwrap();
    // The handle runs again in a terminal no open record names.
    let found = world.add_terminal(true, Some(handle), json!({}));
    let new = RefCell::new(String::new());
    // That terminal is killed and removed; the close's hold lapses there, a restore takes the
    // session over, and its agent registers from its new terminal.
    let racing = after(&world, "rm", 1, || {
        world.f.advance(300.0);
        let [again] = <[_; 1]>::try_from(restoring(&world, handle).unwrap()).unwrap();
        world
            .store()
            .callback_in(
                handle,
                Some("codex:0abc-12"),
                false,
                false,
                Some(&again.row.pane),
            )
            .unwrap();
        *new.borrow_mut() = again.row.pane;
    });
    let before = world.f.said("sim/jump").len();
    let closed = world.close_through(&racing, handle);
    let new = new.take();
    assert_eq!(
        refused(closed),
        format!(
            "terminal {found} is killed, but {}, so this close records nothing more: the next \
             close or restore of {handle} finds it gone",
            taken(handle)
        )
    );
    assert_eq!(
        (world.has_terminal(&found), world.has_terminal(&new)),
        (false, true)
    );
    let now = record(&world);
    assert_eq!((now.closed, now.pane.as_str()), (None, new.as_str()));
    assert_eq!(world.said_since(before), [restored(handle, &new)]);
    let callbacks = world.store().callbacks().unwrap();
    let registered = callbacks
        .get(handle)
        .map(|callback| callback.terminal.clone());
    assert_eq!(registered, Some(Some(new)));
}

#[test]
fn a_restore_that_lost_its_hold_starts_no_terminal_or_kills_the_one_it_started_and_records_nothing()
{
    // Taken over by a close before it starts its terminal, then by one that stopped right after
    // taking it, once the restore's terminal has taken its first message.
    for call in ["ls", "send"] {
        let world = World::new();
        let session = world.waiting_reviewer();
        let handle = &session.handle;
        world.close(handle).unwrap();
        let closed = record(&world);
        let before = (world.f.said("sim/jump").len(), world.addresses());
        let (seen, started) = (Cell::new(false), RefCell::new(String::new()));
        let mut racing = Hooked::new(&world);
        racing.after = Box::new(|args| {
            if args[0] != call || seen.replace(true) {
                return;
            }
            // `send TERMINAL TEXT`: the terminal the restore started.
            *started.borrow_mut() = args.get(1).copied().unwrap_or_default().into();
            world.f.advance(300.0);
            if call == "ls" {
                world.close(handle).unwrap();
            } else {
                let store = world.store();
                store
                    .take_session(handle, "a close by amber-heron")
                    .unwrap();
            }
        });
        let restored = restoring_through(&world, &racing, handle);
        let started = started.take();
        let why = if call == "ls" {
            format!("{}, so the restore starts no terminal", taken(handle))
        } else {
            format!(
                "{} while it was restored, so the restore records nothing and terminal \
                 {started}, which it started, is killed",
                taken(handle)
            )
        };
        assert_eq!(refused(restored), why, "{call}");
        assert_eq!(record(&world), closed, "{call}");
        assert_eq!(
            (world.f.said("sim/jump").len(), world.addresses()),
            before,
            "{call}"
        );
    }
}

#[test]
fn a_restore_whose_record_a_launch_replaced_as_its_terminal_started_kills_it_and_records_nothing() {
    let world = World::new();
    let session = world.waiting_reviewer();
    let handle = &session.handle;
    world.close(handle).unwrap();
    let (started, launched) = (RefCell::new(String::new()), RefCell::new(None));
    let mut racing = Hooked::new(&world);
    // Its terminal has taken its first message: the handle is launched again meanwhile, in
    // another terminal, which changes the record and not the hold.
    racing.after = Box::new(|args| {
        if args[0] != "send" || launched.borrow().is_some() {
            return;
        }
        *started.borrow_mut() = args[1].into();
        world.f.advance(1.0);
        let again = NewLaunch {
            pane: "t77".into(),
            ..NewLaunch::new("sim/jump", handle, "reviewer")
        };
        *launched.borrow_mut() = Some(world.store().record_launch(&again).unwrap());
    });
    let restored = restoring_through(&world, &racing, handle);
    let started = started.take();
    assert_eq!(
        refused(restored),
        format!(
            "{handle}'s record changed while it was restored, so the restore records nothing and \
             terminal {started}, which it started, is killed"
        )
    );
    assert_eq!(
        (Some(record(&world)), world.has_terminal(&started)),
        (launched.take(), false)
    );
}

/// `handle` registers its thread from no terminal.
fn register(world: &World, handle: &str) {
    world
        .store()
        .callback_in(handle, Some("codex:0abc-12"), false, false, None)
        .unwrap();
}

/// What `sql` answers first on the world's board, given `$handle` and the time, `$now`.
fn ask(world: &World, handle: &str, sql: &str) -> Value {
    let vars = json!({"handle": handle, "now": world.f.now()});
    let mut answers = world.store().client().query(sql, vars).unwrap();
    answers.remove(0)
}

/// The number of `handle`'s registration, as its row holds it.
fn number(world: &World, handle: &str) -> u64 {
    let sql =
        "SELECT VALUE registration_revision FROM ONLY type::record('callback_state', $handle)";
    ask(world, handle, sql).as_u64().unwrap()
}

#[test]
fn a_handles_registrations_are_numbered_once_from_the_clock_so_a_retired_ones_never_returns() {
    let world = World::new();
    let session = world.waiting_reviewer();
    let handle = &session.handle;
    let seed = ask(&world, handle, "RETURN <int> math::floor($now)");
    register(&world, handle);
    let first = number(&world, handle);
    // The close retires it, its row with it; the restored agent registers alike again.
    world.close(handle).unwrap();
    restoring(&world, handle).unwrap();
    register(&world, handle);
    let again = number(&world, handle);
    // A count lost, its row kept: it numbers on from that row's.
    ask(
        &world,
        handle,
        "DELETE type::record('counter', ['registration', $handle])",
    );
    register(&world, handle);
    let seed = seed.as_u64().unwrap();
    assert_eq!(
        [first, again, number(&world, handle)],
        [seed + 1, seed + 2, seed + 3]
    );
}

#[test]
fn a_registration_after_one_from_before_the_count_was_retired_never_takes_its_number() {
    let world = World::new();
    let session = world.waiting_reviewer();
    let handle = &session.handle;
    // A board from before the count: the handle's one registration, from no terminal, is its
    // first, and nothing counts them.
    register(&world, handle);
    ask(
        &world,
        handle,
        "UPDATE type::record('callback_state', $handle) SET registration_revision = 1 RETURN NONE;
         DELETE type::record('counter', ['registration', $handle]);",
    );
    // As the close kills, a close from before the hold retires that registration, its row with
    // it, and the agent registers alike again: the count starts, and must not give it 1.
    let racing = ahead_of(&world, "kill", 1, || {
        ask(
            &world,
            handle,
            "DELETE type::record('callback_state', $handle)",
        );
        register(&world, handle);
    });
    world.close_through(&racing, handle).unwrap();
    let left = world.store().callbacks().unwrap().remove(handle.as_str());
    assert_eq!(left.map(|callback| callback.terminal), Some(None));
}
