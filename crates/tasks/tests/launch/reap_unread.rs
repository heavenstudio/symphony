//! The idle sweep and a board that does not answer what holds a terminal's agent (its unanswered
//! lookup): the terminal stays, never closed on no information, whether the sweep judges it or
//! takes its last look before the close, and the sweep goes on to the next. A lookup past its wait
//! is one such board. The presence of Codex threads reads no field of a launch record it does not
//! judge, so a record unreadable elsewhere still says its thread was launched.

use crate::World;
use crate::last_look::changes_at_get;
use std::cell::Cell;
use std::slice::from_ref;
use std::time::Duration;
use symphony_board::launch::reap::{Kept, Sweep};

/// What every reason a terminal stays on no information starts with.
const NO_INFORMATION: &str = "never closed on no information: ";

/// A launch record no reader decodes, on `slug`: its start a NaN, which the database holds and
/// reads out as text, where a reader takes a number. A lookup that reads that task's launches
/// fails.
fn unreadable(slug: &str) -> String {
    format!(
        "INSERT INTO task_session (id, slug, handle, role, claude_session, worktree, branch, \
         pane, model, started, closed, closed_why, restore) VALUES ('[\"{slug}\", \"nobody\"]', \
         '{slug}', 'nobody', 'executor', '', '', '', '', '', 'NaN', 1.0, '', '')"
    )
}

/// A closed launch record of `handle` on `slug` naming the Codex `thread`, which no reader of a
/// whole record decodes: its start a NaN, as [`unreadable`]'s.
fn unreadable_codex(slug: &str, handle: &str, thread: &str) -> String {
    format!(
        "INSERT INTO task_session (id, slug, handle, role, claude_session, worktree, branch, \
         pane, model, started, closed, closed_why, restore) VALUES ('[\"{slug}\", \"{handle}\"]', \
         '{slug}', '{handle}', 'executor', 'codex:{thread}', '', '', 't-gone', '', 'NaN', 1.0, '', '')"
    )
}

/// The board's launches readable again, `slug`'s unreadable one gone.
fn readable(slug: &str) -> String {
    format!("DELETE FROM task_session WHERE id = '[\"{slug}\", \"nobody\"]'")
}

fn write(w: &World, sql: &str) {
    let symphony_board::Database::PostgreSql(client) = w.f.other_database();
    client.simple(sql).unwrap();
}

/// What `swept` said of `terminal`, which it kept: there must be one.
fn kept<'s>(swept: &'s Sweep, terminal: &str) -> &'s Kept {
    let mut of = swept.kept.iter().filter(|k| k.pane == terminal);
    let kept = of
        .next()
        .unwrap_or_else(|| panic!("{terminal} not kept: {swept:?}"));
    assert!(of.next().is_none(), "{swept:?}");
    kept
}

/// Kept for no information, as the idle agent of `handle` its sweep could not judge.
fn unjudged(swept: &Sweep, terminal: &str, handle: &str) {
    let said = kept(swept, terminal);
    assert_eq!(
        (said.handle.as_str(), said.idle),
        (handle, true),
        "{swept:?}"
    );
    assert!(said.why.starts_with(NO_INFORMATION), "{swept:?}");
}

#[test]
fn a_terminal_whose_holds_the_board_cannot_say_stays_and_the_sweep_judges_the_rest() {
    let w = World::new();
    let (stuck, stuck_pane) = w.worker("sim/jump", Some("done"));
    w.task("sim/other", None);
    let (other, other_pane) = w.worker("sim/other", Some("done"));
    write(&w, &unreadable("sim/jump"));
    let swept = w.sweep_through(&w.hc());
    unjudged(&swept, &stuck_pane, &stuck);
    assert_eq!(swept.idle, from_ref(&other), "{swept:?}");
    assert!(swept.closed.is_empty(), "{swept:?}");

    // Readable again, it waits its own idle wait from there, and the other one closes.
    write(&w, &readable("sim/jump"));
    let swept = w.sweep_through(&w.hc());
    assert_eq!(swept.idle, from_ref(&stuck), "{swept:?}");
    let closed: Vec<&str> = swept.closed.iter().map(|c| c.pane.as_str()).collect();
    assert_eq!(closed, [other_pane.as_str()], "{swept:?}");
    assert!(w.has_terminal(&stuck_pane));
}

/// The presence of Codex threads reads a launch record for its thread, its terminal and whether it
/// is open alone: one the board cannot read elsewhere still says its thread was launched, so a
/// thread in no terminal now is retired, and the sweep judges the terminals.
#[test]
fn a_codex_record_unreadable_where_presence_never_reads_still_retires_its_gone_thread() {
    let w = World::new();
    let (idle, _) = w.worker("sim/jump", Some("done"));
    let gone = w.f.session("worker");
    (w.store())
        .callback_in(&gone, Some("codex:0abc"), false, false, None)
        .unwrap();
    write(&w, &unreadable_codex("sim/old", &gone, "0abc"));
    let swept = w.sweep_through(&w.hc());
    assert_eq!(swept.idle, from_ref(&idle), "{swept:?}");
    assert!(!w.store().callbacks().unwrap().contains_key(&gone));
}

#[test]
fn a_terminal_whose_holds_the_board_cannot_say_at_either_last_look_stays_with_its_record() {
    // The second get is the close's first look; the third, its own right before the kill, after
    // the closure is recorded.
    for nth in [2, 3] {
        let w = World::new();
        let (handle, terminal) = w.worker("sim/jump", Some("done"));
        assert_eq!(w.sweep_through(&w.hc()).idle, from_ref(&handle));
        let gets = Cell::new(0);
        let fail = |_: &str| write(&w, &unreadable("sim/jump"));
        let swept = w.sweep_through(&changes_at_get(&w, nth, &fail, &gets));
        assert_eq!(gets.get(), nth, "{swept:?}");
        unjudged(&swept, &terminal, &handle);
        assert!(swept.closed.is_empty(), "{nth}: {swept:?}");
        assert!(w.has_terminal(&terminal), "{nth}: {terminal} was closed");
        assert_eq!(w.calls_of(&["kill"]), Vec::<Vec<String>>::new());
        write(&w, &readable("sim/jump"));
        let [open] = <[_; 1]>::try_from(w.store().task_sessions("sim/jump").unwrap()).unwrap();
        assert_eq!((open.closed, open.pane.as_str()), (None, terminal.as_str()));
    }
}

/// A lookup past its wait is a board that does not say: a microsecond, which no answer meets
/// however quiet the machine, keeps the terminal, and the store the sweep runs on otherwise, with
/// no deadline of its own, judges it.
#[test]
fn a_lookup_past_its_wait_keeps_the_terminal_it_was_to_judge() {
    let w = World::new();
    let (handle, terminal) = w.worker("sim/jump", Some("done"));
    let hurried = w.f.other().looking_up_within(Duration::from_micros(1));
    let swept = w.sweep_on(&hurried, &w.hc());
    unjudged(&swept, &terminal, &handle);
    assert!(swept.idle.is_empty(), "{swept:?}");
    assert_eq!(w.sweep_through(&w.hc()).idle, from_ref(&handle));
}
