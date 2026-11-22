//! The existing clock watch, planned and reconciled again under its recipient lease. Only
//! recognizable generated pane rows are withdrawn; messages and other events are untouched.

use super::{
    Callback, DeliveryError, Error, Event, Queued, Store, Visit, deliver, event_line, session_task,
};
use crate::Board;
use crate::cli::agents::{Answer, Pane};
use crate::cli::attach::{self, PaneWatch};
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
    pub(super) fn watch_panes(
        &self,
        board: &Board,
        callbacks: &BTreeMap<String, Callback>,
    ) -> Result<(), Error> {
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
                .unwrap_or_else(|e| e.into_inner())
                .get_or_insert_with(|| crate::cli::probes::hc_terminals(&env))
                .clone()
        };
        for callback in recipients {
            self.watch_one(board, callback, &open, &panes, false, &|text| {
                deliver(callback, text)
            })?;
        }
        Ok(())
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
        self.watch_one(board, callback, open, panes, true, &|text| {
            visit.send(callback, text)
        })
    }

    /// One recipient's watch: what its panes say since its saved watermark, queued and sent by
    /// `send` under its lease, which a collecting look (`collect`) takes with no channel registered.
    fn watch_one(
        &self,
        board: &Board,
        callback: &Callback,
        open: &[TaskSessionRow],
        panes: &dyn Fn() -> Answer<Vec<Pane>>,
        collect: bool,
        send: &dyn Fn(&str) -> Result<(), DeliveryError>,
    ) -> Result<(), Error> {
        let handle = callback.handle.as_str();
        // Both looks judge the one sample at the moment of the first.
        let now = self.now();
        let queued: Vec<Queued> =
            self.retrying(|| self.read(both(sql::QUEUED, pg::queued()), json!({"handle":handle})))?;
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
            return Ok(());
        }
        self.with_callback_lease(handle, collect, |revision| {
            let saved: Saved = self.retrying(|| self.read(
                both(sql::SAVED, pg::saved()),
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
        Ok(())
    }
}
