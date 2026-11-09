//! A terminal whose program ended: its open records closed with their restore line ([`ENDED`]),
//! then it is removed, out of the owner's sight; kept when it runs again, its records open again.
//! The sweep holds each record's session while it writes them ([`Store::take_session`]), as a
//! close or a restore does, and writes each only as it read it: the same launch, in that terminal.
//! A record a restore moved to another terminal since is that terminal's, never closed for this
//! one's end, nor opened again here.

use super::{Closed, ENDED, Round, name};
use crate::launch::cleanup::restore_line;
use crate::launch::hc::{Object, text_of};
use crate::launch::screen::{self, NotClosed};
use crate::rows::TaskSessionRow;
use crate::{Error, Store};
use std::cell::Cell;
use std::collections::BTreeMap;

/// What the sweep's hold of a session says it does, to a close or a restore it refuses.
const DOING: &str = "the idle sweep's removal of its ended terminal";

/// What a removal came to: why the terminal stays, if it does, and the records it closed.
type Removed = (Option<String>, Vec<TaskSessionRow>);

impl Round<'_> {
    /// A terminal `hc ls` lists as ended: the open records naming it closed with their restore line
    /// (nothing to say when none is), then it is removed, out of the owner's sight; kept, saying
    /// why, when a close or a restore of its session is under way, or the close's last look finds
    /// it on screen, running again or not to be read.
    pub(super) fn ended(&mut self, address: &str) -> Result<(), Error> {
        let terminal = match self.hc.get(address) {
            Ok(Some(terminal)) => terminal,
            Ok(None) => return Ok(()),
            Err(problem) => {
                self.keep(address, "", screen::no_information(&problem), false);
                return Ok(());
            }
        };
        let records: Vec<TaskSessionRow> = (self.open.iter())
            .filter(|s| s.pane == address)
            .cloned()
            .collect();
        let handle = (records.last().map(|s| s.handle.clone()))
            .or_else(|| name(&terminal))
            .unwrap_or_default();
        let (kept, closed) = if self.how.apply {
            let held = match take(self.store, &records) {
                Ok(held) => held,
                Err(problem) => {
                    let why = format!("the board took no record of its end: {problem}");
                    self.keep(address, &handle, why, false);
                    return Ok(());
                }
            };
            let removed = self.remove(address, &handle, &records, &held);
            let released = release(self.store, &held);
            let removed = removed?;
            released?;
            match removed {
                Some(removed) => removed,
                None => return Ok(()),
            }
        } else {
            let kept = match runs_again(&terminal, &Cell::new(false)).and_then(|()| {
                screen::on_screen(self.hc, &terminal).map_err(|p| screen::no_information(&p))
            }) {
                Ok(false) => None,
                Ok(true) => Some(screen::ON_SCREEN.into()),
                Err(why) => Some(why),
            };
            (kept, records)
        };
        let restore = closed
            .last()
            .map_or(String::new(), |s| restore_line(&s.slug, &s.handle));
        match kept {
            None => self.result.closed.push(Closed {
                pane: address.into(),
                handle,
                why: ENDED.into(),
                restore,
            }),
            Some(why) => self.keep(address, &handle, why, false),
        }
        Ok(())
    }

    /// Close `records` (their sessions `held`), then remove the terminal: the records it closed and
    /// why it stays, if it does; None when the board took no record, said and kept.
    fn remove(
        &mut self,
        address: &str,
        handle: &str,
        records: &[TaskSessionRow],
        held: &BTreeMap<String, u64>,
    ) -> Result<Option<Removed>, Error> {
        let closed = match closing_records(self.store, records, held) {
            Ok(closed) => closed,
            Err(problem) => {
                let _ = writeln!(
                    self.warn,
                    "· idle sweep kept {address} ({handle}): the board took no record: {problem}"
                );
                let _ = self.warn.flush();
                let why = format!("the board took no record of its end: {problem}");
                self.keep(address, handle, why, false);
                return Ok(None);
            }
        };
        let live = Cell::new(false);
        let kept = match screen::close_unseen(self.hc, address, &|now| runs_again(now, &live)) {
            Ok(()) => None,
            Err(NotClosed::Kept(why)) => Some(why),
            Err(NotClosed::Refused(problem)) => Some(format!("hc refused to remove it: {problem}")),
        };
        if live.get() {
            // its session did not end after all
            for (s, hold) in &closed {
                self.store.reopen_task_session_as(s, address, *hold)?;
            }
        }
        Ok(Some((kept, closed.into_iter().map(|(s, _)| s).collect())))
    }
}

/// Whether the terminal, as the close's own look finds it (`now`), still has its program ended:
/// else why it stays, and `live` says so.
fn runs_again(now: &Object, live: &Cell<bool>) -> Result<(), String> {
    if text_of(now, "state") == "live" {
        live.set(true);
        Err("its program runs again".to_string())
    } else {
        Ok(())
    }
}

/// Hold the session of each of `records` for the removal ([`Store::take_session`]): each handle's
/// hold. Refused, holding none, while a close or a restore of one of them is under way.
fn take(store: &Store, records: &[TaskSessionRow]) -> Result<BTreeMap<String, u64>, Error> {
    let mut held = BTreeMap::new();
    for s in records {
        if held.contains_key(&s.handle) {
            continue;
        }
        match store.take_session(&s.handle, DOING) {
            Ok(hold) => {
                held.insert(s.handle.clone(), hold);
            }
            Err(problem) => {
                let _ = release(store, &held); // the refusal is what the sweep must say
                return Err(problem);
            }
        }
    }
    Ok(held)
}

/// Let go of every hold in `held`: the first failure, once each was tried.
fn release(store: &Store, held: &BTreeMap<String, u64>) -> Result<(), Error> {
    held.iter()
        .map(|(handle, hold)| store.release_session(handle, *hold))
        .fold(Ok(()), Result::and)
}

/// Close each of `records`, the open records of a terminal whose program ended, with its restore
/// line, only as the sweep read it: the same launch, still open, in that terminal. The ones it
/// closed, each with its session's hold.
fn closing_records(
    store: &Store,
    records: &[TaskSessionRow],
    held: &BTreeMap<String, u64>,
) -> Result<Vec<(TaskSessionRow, u64)>, Error> {
    let mut closed = Vec::new();
    for s in records {
        let Some(&hold) = held.get(&s.handle) else {
            continue;
        };
        if store.close_task_session_as(s, ENDED, &restore_line(&s.slug, &s.handle), hold)? {
            closed.push((s.clone(), hold));
        }
    }
    Ok(closed)
}
