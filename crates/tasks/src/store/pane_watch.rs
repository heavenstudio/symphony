//! The existing clock watch, planned and reconciled again under its recipient lease. Only
//! recognizable generated pane rows are withdrawn; messages and other events are untouched.

use super::{
    Callback, DeliveryError, Error, Event, Queued, Store, Visit, deliver, event_line,
    recipient_rows, session_task, taken,
};
use crate::Board;
use crate::cli::agents::{Answer, Pane};
use crate::cli::attach::{self, PaneWatch};
use crate::lock::Recovered as _;
use crate::rows::TaskSessionRow;
use crate::store::postgres::pane_watch as pg;
use crate::store::statement::both;
use crate::store::surreal::pane_watch as sql;
use serde::Deserialize;
use serde_json::{Value, json};
use std::cell::OnceCell;
use std::collections::BTreeMap;

#[derive(Deserialize)]
struct Saved {
    watch: PaneWatch,
    queued: Vec<Queued>,
}

struct Plan {
    after: PaneWatch,
    lines: Vec<String>,
    retired: Vec<String>,
}

impl Plan {
    fn unchanged(&self, before: &PaneWatch) -> bool {
        self.lines.is_empty() && self.retired.is_empty() && self.after == *before
    }
}

/// One look at `now`, the moment of the tick's sample.
fn look(
    board: &Board,
    handle: &str,
    open: &[TaskSessionRow],
    panes: &dyn Fn() -> Answer<Vec<Pane>>,
    now: f64,
    before: &PaneWatch,
    queued: &[Queued],
) -> Plan {
    let overseen = attach::overseen(board, handle);
    let sample = OnceCell::new();
    let panes = || sample.get_or_init(panes).clone();
    let mut lines = Vec::new();
    let after = attach::pane_watch(board, &overseen, open, &panes, now, before, &mut |line| {
        lines.push(event_line(line))
    });
    let retired = queued
        .iter()
        .filter(|row| before.retires(board, &overseen, open, sample.get(), &row.key, &row.text))
        .map(|row| row.key.clone())
        .collect();
    Plan {
        after,
        lines,
        retired,
    }
}

impl Store {
    /// One HC sample per tick, shared with PULL and agents. Re-read the saved watermark and
    /// pending generated rows under the existing lease before publishing or withdrawing a line.
    /// Every recipient's queued pane lines are read at once. Whether a watch took a lease to
    /// record or send what its panes said: with none taken, the watch wrote nothing.
    pub(super) fn watch_panes(
        &self,
        board: &Board,
        callbacks: &BTreeMap<String, Callback>,
    ) -> Result<bool, Error> {
        let recipients: Vec<_> = callbacks.values().filter(|c| c.host == self.host).collect();
        let open = if recipients
            .iter()
            .any(|c| !attach::overseen(board, &c.handle).is_empty())
        {
            self.open_task_sessions()?
        } else {
            Vec::new()
        };
        let env = crate::cli::Env::of_process();
        let panes = || {
            self.clock_panes
                .lock()
                .recover()
                .get_or_insert_with(|| crate::cli::probes::hc_terminals(&env))
                .clone()
        };
        let mut queued = self.queued_panes(&recipients)?;
        let mut watched = false;
        for callback in recipients {
            let queued = taken(&mut queued, &callback.handle).unwrap_or_default();
            watched |= self.watch_one(board, callback, &open, &panes, false, queued, &|text| {
                deliver(callback, text)
            })?;
        }
        Ok(watched)
    }

    /// The pane lines still queued for each of `recipients`, with its handle, in their order, all
    /// of them in one read ([`recipient_rows`]).
    pub(super) fn queued_panes(
        &self,
        recipients: &[&Callback],
    ) -> Result<Vec<(String, Vec<Queued>)>, Error> {
        if recipients.is_empty() {
            return Ok(Vec::new());
        }
        let handles: Vec<Value> = (recipients.iter())
            .map(|callback| json!({"handle": callback.handle}))
            .collect();
        let vars = json!({"recipients": handles});
        let answer: Value =
            self.retrying(|| self.read(both(sql::QUEUED.as_str(), pg::queued()), vars.clone()))?;
        recipient_rows(&answer, "struct Lines", |row| row.field("queued"))
    }

    /// The watch for a session with no channel, at its waiting look (`tasks wait`): the panes it
    /// oversees, sampled by `panes`, and what they say printed by `visit` with the rest of the look.
    pub(super) fn watch_pane(
        &self,
        board: &Board,
        callback: &Callback,
        open: &[TaskSessionRow],
        panes: &dyn Fn() -> Answer<Vec<Pane>>,
        visit: &Visit,
    ) -> Result<(), Error> {
        let queued =
            taken(&mut self.queued_panes(&[callback])?, &callback.handle).unwrap_or_default();
        self.watch_one(board, callback, open, panes, true, queued, &|text| {
            visit.send(callback, text)
        })
        .map(|_| ())
    }

    /// One recipient's watch: what its panes say since its saved watermark, with the pane lines
    /// `queued` for it, queued and sent by `send` under its lease, which a collecting look
    /// (`collect`) takes with no channel registered. Whether it took the lease.
    #[expect(
        clippy::too_many_arguments,
        reason = "the watch's one sample, read once a tick for every recipient, and the lines read for each at once"
    )]
    fn watch_one(
        &self,
        board: &Board,
        callback: &Callback,
        open: &[TaskSessionRow],
        panes: &dyn Fn() -> Answer<Vec<Pane>>,
        collect: bool,
        queued: Vec<Queued>,
        send: &dyn Fn(&str) -> Result<(), DeliveryError>,
    ) -> Result<bool, Error> {
        let handle = callback.handle.as_str();
        // Both looks judge the one sample at the moment of the first.
        let now = self.now();
        let plan = look(
            board,
            handle,
            open,
            panes,
            now,
            &callback.pane_watch,
            &queued,
        );
        if plan.unchanged(&callback.pane_watch) {
            return Ok(false);
        }
        self.with_callback_lease(handle, collect, |revision| {
            let saved: Saved = self.retrying(|| self.read(
                both(sql::SAVED.as_str(), pg::saved()),
                json!({"handle":handle}),
            ))?;
            // A role or launch can move while the first look waits for its lease.
            let (board, _) = self.callback_board()?;
            let open = self.open_task_sessions()?;
            let plan = look(&board, handle, &open, panes, now, &saved.watch, &saved.queued);
            if plan.unchanged(&saved.watch) { return Ok(true); }
            let events: Vec<Event> = plan.lines.iter().enumerate().map(|(i, text)| {
                let key = format!("pane-{revision}-{i}");
                Event::line(key, handle, text.clone(), session_task(&board, handle), None)
            }).collect();
            self.retrying(|| self.transact::<Value>(
                both(sql::RECORD, pg::record()),
                json!({"handle":handle, "events":events, "after":plan.after, "retired":plan.retired}),
            ))?;
            self.deliver_pending(handle, callback, revision, send)
        })?;
        Ok(true)
    }
}
