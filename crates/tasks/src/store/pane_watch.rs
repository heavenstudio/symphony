//! The existing clock watch, planned and reconciled again under its recipient lease. Only
//! recognizable generated pane rows are withdrawn; messages and other events are untouched.

use super::{Callback, Error, Event, Queued, Store, deliver, event_line, session_task};
use crate::Board;
use crate::cli::agents::{Answer, Pane};
use crate::cli::attach::{self, PaneWatch};
use crate::rows::TaskSessionRow;
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

fn look(
    board: &Board,
    handle: &str,
    open: &[TaskSessionRow],
    panes: &dyn Fn() -> Answer<Vec<Pane>>,
    before: &PaneWatch,
    queued: &[Queued],
) -> Plan {
    let overseen = attach::overseen(board, handle);
    let sample = OnceCell::new();
    let panes = || sample.get_or_init(panes).clone();
    let mut lines = Vec::new();
    let after = attach::pane_watch(board, &overseen, open, &panes, before, &mut |line| {
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
            let handle = callback.handle.as_str();
            let queued: Vec<Queued> = self.retrying(|| self.read(
                "RETURN SELECT key, text FROM callback_event WHERE handle = $handle AND delivered != true AND message = NONE AND string::starts_with(key, 'pane-');",
                json!({"handle":handle}),
            ))?;
            let plan = look(board, handle, &open, &panes, &callback.pane_watch, &queued);
            if plan.unchanged(&callback.pane_watch) {
                continue;
            }
            self.with_callback_lease(handle, false, |deadline, revision| {
                let saved: Saved = self.retrying(|| self.read(
                    "RETURN { watch: (SELECT VALUE pane_watch FROM ONLY type::record('callback_state', $handle)) ?? {},
                        queued: (SELECT key, text FROM callback_event WHERE handle = $handle AND delivered != true AND message = NONE AND string::starts_with(key, 'pane-')) };",
                    json!({"handle":handle}),
                ))?;
                // A role or launch can move while the first look waits for its lease.
                let (board, _) = self.callback_board()?;
                let open = self.open_task_sessions()?;
                let plan = look(&board, handle, &open, &panes, &saved.watch, &saved.queued);
                if plan.unchanged(&saved.watch) { return Ok(()); }
                let events: Vec<Event> = plan.lines.iter().enumerate().map(|(i, text)| Event {
                    key: format!("pane-{revision}-{i}"), handle: handle.into(), text: text.clone(),
                    slug: session_task(&board, handle), message: None, attempts: 0, sequence: 0,
                    pull_entries: None, pull_withheld: None, pull_said: None,
                }).collect();
                self.retrying(|| self.transact::<Value>(
                    "DELETE callback_event WHERE handle = $handle AND delivered != true AND message = NONE AND key IN $retired RETURN NONE;
                     FOR $event IN $events {
                        IF !record::exists(type::record('callback_event', [$handle, $event.key])) {
                            LET $counted = (UPSERT ONLY type::record('counter', ['callback-event', $handle]) SET last = (last ?? 0) + 1);
                            CREATE type::record('callback_event', [$handle, $event.key]) CONTENT $event RETURN NONE;
                            UPDATE type::record('callback_event', [$handle, $event.key]) SET sequence = $counted.last RETURN NONE;
                        };
                     };
                     UPDATE type::record('callback_state', $handle) SET pane_watch = $after RETURN NONE;
                     RETURN NONE;",
                    json!({"handle":handle, "events":events, "after":plan.after, "retired":plan.retired}),
                ))?;
                self.deliver_pending(handle, callback, deadline, &|text| deliver(callback, text))
            })?;
        }
        Ok(())
    }
}
