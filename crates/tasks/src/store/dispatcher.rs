//! The detached dispatcher a command line's write starts ([`Store::dispatching_by`]): the delivery
//! passes its host's writes ask for, by one dispatcher a host at a time. A write has already queued
//! what each recipient hears when it starts one; the dispatcher delivers it, and observes again
//! for what the write could not (a recipient whose lease another dispatcher held).
//!
//! The lease is the row `callback_lease:['dispatch', HOST]`: `until` and `revision` say who holds
//! it, `asked` counts the passes dispatchers asked for, `served` how many of them a pass has begun
//! after, and `completed` how many of them a pass begun after them has ended. A dispatcher that
//! asked stays responsible for its ask until `completed` reaches it: a holder lost between
//! beginning the pass and ending it leaves its lease to lapse, and the one that asked takes the
//! host over. A host's dispatchers deliver to its recipients alone, so another host's never takes
//! an ask it could not serve.

use super::callbacks::{CALLBACK_LEASE_SECONDS, Pass};
use super::postgres::dispatcher as pg;
use super::statement::Statement;
use super::{Error, Store};
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

/// How long a dispatcher holds its host's lease without a word: past it, a dispatcher waiting on
/// it takes the host over. Each pass renews it.
const LEASE_SECONDS: u64 = 60;

/// How often a dispatcher waiting on another looks at the lease again.
const LOOK: Duration = Duration::from_millis(200);

/// How long a dispatcher waits on another, by this machine's clock, before it leaves the ask to
/// the next write's dispatcher or the server's clock: twice a lease, which a dead holder's lapses
/// within, unless the board's clock stands still (a test's).
const PATIENCE: Duration = Duration::from_secs(2 * LEASE_SECONDS);

/// How long a holder passes again for a recipient another dispatcher held: past a recipient's
/// lease, which lapses within it if its holder died.
const RECIPIENT_PATIENCE: Duration = Duration::from_secs(CALLBACK_LEASE_SECONDS + 5);

/// How long a holder waits before passing again for a recipient another dispatcher held.
const BACK_OFF: Duration = Duration::from_millis(500);

/// What a dispatcher found at the lease: taken (its revision, and the asks its first pass begins
/// after), or held by another, which it asked for one more pass (the number of its ask).
#[derive(Deserialize)]
struct Took {
    taken: Option<u64>,
    began: Option<u64>,
    ask: Option<u64>,
}

/// The lease as a waiting dispatcher reads it.
#[derive(Deserialize)]
struct Lease {
    completed: u64,
    until: f64,
}

/// What a holder does after a pass, and the asks its next pass begins after.
#[derive(Deserialize)]
struct Next {
    next: String,
    began: Option<u64>,
}

/// How a wait on another dispatcher ended.
enum Waited {
    /// A pass begun after the ask ended: the holder's, or a later holder's.
    Served,
    /// The holder's lease lapsed before that: it died or stalled, and the host is free to take.
    Lapsed,
    /// Neither came within [`PATIENCE`].
    Gave,
}

impl Store {
    /// A detached dispatcher's work: its host's delivery passes, by one dispatcher at a time. It
    /// takes the host's lease, unless another dispatcher holds it: then it asks that one for one
    /// more pass, since a pass begun before its write committed may not have seen it, and waits
    /// until a pass begun after the ask has ended. If the holder's lease lapses first (it died, or
    /// stalled past [`LEASE_SECONDS`]), whether before that pass began or during it, it takes the
    /// host over and makes the passes itself; a new holder alone is not the pass it asked for. The
    /// holder passes until a pass ends with no ask standing and every recipient reached, then lets
    /// the lease go; one whose lease lapsed and was taken ends after its pass, leaving the lease and
    /// the asks to the new holder. The passes it made, 0 when another made them. Two passes at once
    /// stay safe, as they always were: each recipient's own lease and the outbox's keys keep every
    /// event to one delivery.
    pub fn dispatch_detached(&self) -> Result<usize, Error> {
        let began = Instant::now();
        loop {
            let took: Took = self.retrying(|| {
                self.transact(
                    Statement::from(pg::take()),
                    json!({"host": self.host, "now": self.now(), "lease_seconds": LEASE_SECONDS}),
                )
            })?;
            if let Some(revision) = took.taken {
                return self.hold(revision, took.began.unwrap_or_default());
            }
            match self.wait_on(took.ask.unwrap_or_default(), began)? {
                Waited::Served | Waited::Gave => return Ok(0),
                Waited::Lapsed => {}
            }
        }
    }

    /// Wait until a pass begun after ask number `ask` has ended, whichever holder made it, or the
    /// lease lapses first.
    fn wait_on(&self, ask: u64, began: Instant) -> Result<Waited, Error> {
        loop {
            #[expect(
                clippy::disallowed_methods,
                reason = "no-timed-waits baseline: looks again in the store for another holder's pass to end, where a live query is the event"
            )]
            std::thread::sleep(LOOK);
            let lease: Lease = self
                .retrying(|| self.read(Statement::from(pg::lease()), json!({"host": self.host})))?;
            if lease.completed >= ask {
                return Ok(Waited::Served);
            }
            if lease.until <= self.now() {
                return Ok(Waited::Lapsed);
            }
            if began.elapsed() >= PATIENCE {
                return Ok(Waited::Gave);
            }
        }
    }

    /// The passes of the dispatcher holding revision `revision` of its host's lease, the first of
    /// them begun after ask number `began`.
    fn hold(&self, revision: u64, mut began: u64) -> Result<usize, Error> {
        let mut passes = 0;
        let mut short_since: Option<Instant> = None;
        loop {
            passes += 1;
            let reached = match self.dispatch_pass(Pass::Deliver) {
                Ok(reached) => reached,
                Err(error) => {
                    // The next dispatcher, or the server's clock, delivers what this one could not.
                    // A lease this cannot let go lapses.
                    let _ = self.retrying(|| {
                        self.transact::<Value>(
                            Statement::from(pg::let_go()),
                            json!({"host": self.host, "revision": revision}),
                        )
                    });
                    return Err(error);
                }
            };
            // A recipient another dispatcher held is passed again once its lease may be free: that
            // one may have read the board before the write this pass was asked for.
            let more = if reached {
                short_since = None;
                false
            } else {
                short_since.get_or_insert_with(Instant::now).elapsed() < RECIPIENT_PATIENCE
            };
            // The pass ended: every ask it began after is served, even when this holder's lease
            // lapsed meanwhile. Then the next pass, if one is asked for, begins after every ask in.
            let next: Next = self.retrying(|| {
                self.transact(
                    Statement::from(pg::passed()),
                    json!({"host": self.host, "revision": revision, "began": began, "more": more, "now": self.now(), "lease_seconds": LEASE_SECONDS}),
                )
            })?;
            if next.next != "again" {
                return Ok(passes);
            }
            began = next.began.unwrap_or(began);
            if more {
                #[expect(
                    clippy::disallowed_methods,
                    reason = "no-timed-waits baseline: paces the dispatcher's passes by a length of time"
                )]
                std::thread::sleep(BACK_OFF);
            }
        }
    }
}
