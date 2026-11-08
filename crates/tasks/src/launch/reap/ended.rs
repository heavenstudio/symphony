//! A terminal whose program ended: its open records closed with their restore line ([`ENDED`]),
//! then it is removed, out of the owner's sight; kept when it runs again, its records open again.

use super::{Closed, ENDED, Round, name};
use crate::launch::cleanup::restore_line;
use crate::launch::hc::{Object, text_of};
use crate::launch::screen::{self, NotClosed};
use crate::rows::TaskSessionRow;
use crate::{Error, Store};
use std::cell::Cell;

impl Round<'_> {
    /// A terminal `hc ls` lists as ended: the open records naming it closed with their restore line
    /// (nothing to say when none is), then it is removed, out of the owner's sight; kept, saying
    /// why, when the close's last look finds it on screen, running again or not to be read.
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
        let restore = records
            .last()
            .map_or(String::new(), |s| restore_line(&s.slug, &s.handle));
        let live = Cell::new(false);
        let again = |now: &Object| {
            if text_of(now, "state") == "live" {
                live.set(true);
                Err("its program runs again".to_string())
            } else {
                Ok(())
            }
        };
        let kept = if self.how.apply {
            if let Err(problem) = closing_records(self.store, &records) {
                let _ = writeln!(
                    self.warn,
                    "· idle sweep kept {address} ({handle}): the board took no record: {problem}"
                );
                let _ = self.warn.flush();
                let why = format!("the board took no record of its end: {problem}");
                self.keep(address, &handle, why, false);
                return Ok(());
            }
            let kept = match screen::close_unseen(self.hc, address, &again) {
                Ok(()) => None,
                Err(NotClosed::Kept(why)) => Some(why),
                Err(NotClosed::Refused(problem)) => {
                    Some(format!("hc refused to remove it: {problem}"))
                }
            };
            if live.get() {
                // its session did not end after all
                for s in &records {
                    self.store
                        .reopen_task_session(&s.slug, &s.handle, address)?;
                }
            }
            kept
        } else {
            match again(&terminal).and_then(|()| {
                screen::on_screen(self.hc, &terminal).map_err(|p| screen::no_information(&p))
            }) {
                Ok(false) => None,
                Ok(true) => Some(screen::ON_SCREEN.into()),
                Err(why) => Some(why),
            }
        };
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
}

/// Close `records`, the open records of a terminal whose program ended, each with its restore line.
fn closing_records(store: &Store, records: &[TaskSessionRow]) -> Result<(), Error> {
    for s in records {
        store.close_task_session(&s.slug, &s.handle, ENDED, &restore_line(&s.slug, &s.handle))?;
    }
    Ok(())
}
