//! `wait`: what a callback would push the session, printed on its own output once there is any
//! ([`Store::collect_callbacks`]), so a session with no channel runs it as a background command
//! and runs it again when it exits. It looks when the board moved since its last look, and at
//! least every [`LOOK_SECONDS`], since some of what it would be told changes with time alone (a
//! child going quiet, a PULL falling due). Waiting is the session's sign of life, as a monitor's
//! is: it beats for it every [`BEAT_SECONDS`], so an orchestrator waiting on its board is not
//! called stale for it. An orchestrator that was not live at its first beat is given every open
//! task nobody orchestrates, by rule 4 among the live ones, and told one GIVEN line
//! (`Store::give_unowned`): what waited for an orchestrator while none was live is its own now.

use super::{Context, Stop};
use crate::Store;

/// How long it waits between reads of the board's marker.
const POLL_SECONDS: f64 = 2.0;
/// The longest it goes without a look while the board stands still.
const LOOK_SECONDS: f64 = 15.0;
/// How often it beats for the session, at a look: the monitor's beat.
const BEAT_SECONDS: f64 = 20.0;

pub(super) fn run(store: &Store, c: &mut Context) -> Result<(), Stop> {
    let me = c.actor()?;
    let p = c.parsed;
    let timeout = p.get("timeout").map(|_| p.float("timeout", 0.0));
    if timeout.is_some_and(|seconds| !seconds.is_finite() || seconds < 0.0) {
        return Err(Stop::Refused(
            "--timeout is a number of seconds, 0 or more".into(),
        ));
    }
    let started = store.now();
    // The marker the last look left the board at, and when it looked; when it last beat.
    let mut last: Option<(u64, f64)> = None;
    let mut beat: Option<f64> = None;
    loop {
        let due = match last {
            None => true,
            Some((marker, at)) => store.board_mark()? != marker || store.now() - at >= LOOK_SECONDS,
        };
        if due {
            // An orchestrator coming back from not live, at its first beat: what it is given.
            let mut given = None;
            // Before the look, which then reads the board this beat moved.
            if beat.is_none_or(|at| store.now() - at >= BEAT_SECONDS) {
                let was = beat.is_none().then(|| store.session(&me).ok()).flatten();
                store.touch(&me)?;
                beat = Some(store.now());
                if let Some(was) = was.filter(|s| s.health != crate::Health::Live) {
                    given = super::arriving(store, &me, &was.row.kind);
                }
            }
            // Each text printed whole and flushed before the look records it delivered.
            let out = &mut *c.out;
            let mut print = |text: &str| writeln!(out, "{text}").and_then(|()| out.flush());
            if let Some(line) = &given {
                print(line).map_err(|e| Stop::Board(format!("cannot print: {e}")))?;
            }
            let (printed, marker) = store.collect_callbacks(&me, c.env, &mut print)?;
            if printed > 0 || given.is_some() {
                return Ok(());
            }
            last = Some((marker, store.now()));
        }
        let waited = store.now() - started;
        if let Some(timeout) = timeout
            && waited >= timeout
        {
            c.say(&format!("WAIT nothing changed in {timeout} s"));
            return Ok(());
        }
        let step = timeout.map_or(POLL_SECONDS, |timeout| POLL_SECONDS.min(timeout - waited));
        c.clock.wait(step, &mut *c.input, &mut *c.err);
    }
}
