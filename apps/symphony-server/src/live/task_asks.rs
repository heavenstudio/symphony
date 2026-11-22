//! What the task board asks of each person beyond the cards ([`symphony_api::TaskAsks`]): the
//! questions agents asked them on open tasks that still wait on them, and the newest reviewer's
//! verdict. A board observation reads them before it moves anything ([`Hub::read_asks`]) and
//! applies them once the message cursor moved ([`Hub::apply_asks`]): the hub keeps them for the
//! one server's project list (`/api/projects`, [`Hub::task_asks`]) and sends them on the feed, in
//! a region of their own, each time they change.

use std::collections::BTreeMap;
use symphony_board::lock::Recovered as _;

use serde_json::Value;
use symphony::taskview;
use symphony_api::{TaskAsks, TaskVerdict};

use super::Hub;

/// What the hub holds of the asks.
#[derive(Default)]
pub(super) struct Asked {
    /// The questions waiting, by person, and the message cursor and open tasks they were counted
    /// at: counted again only when either moved.
    counted: Option<(i64, Vec<String>, BTreeMap<String, u32>)>,
    /// Whether the board's newest verdict was read: once, at the first observation, after which the
    /// hub follows the lines after it.
    followed: bool,
    /// The newest reviewer's verdict the board holds.
    verdict: Option<TaskVerdict>,
    /// What the feed was last sent: what the project list reads.
    sent: Option<TaskAsks>,
}

/// What one observation read of the asks before it moved anything.
pub(super) struct Read {
    /// The message cursor and open tasks the questions were counted at.
    at: (i64, Vec<String>),
    questions: BTreeMap<String, u32>,
    /// The board's newest verdict, when this observation read it: the first's.
    newest: Option<Option<TaskVerdict>>,
}

impl Hub {
    /// The asks as the board holds them at the message cursor `latest` over `tasks`: the questions
    /// counted again only when the cursor or the open tasks moved since they were last, the newest
    /// verdict read only before the hub follows the lines.
    pub(super) fn read_asks(
        &self,
        store: &symphony_board::Store,
        latest: i64,
        tasks: &[symphony_api::Task],
    ) -> Result<Read, String> {
        let open = taskview::open(tasks);
        let (counted, followed) = {
            let asked = self.task_asks.lock().recover();
            let counted = asked
                .counted
                .as_ref()
                .filter(|(at, was, _)| *at == latest && *was == open)
                .map(|(_, _, questions)| questions.clone());
            (counted, asked.followed)
        };
        let questions = match counted {
            Some(questions) => questions,
            None => taskview::questions(store, &open).map_err(|e| e.to_string())?,
        };
        let newest = if followed {
            None
        } else {
            let newest = store.newest_verdict().map_err(|e| e.to_string())?;
            Some(newest.as_ref().and_then(taskview::verdict))
        };
        Ok(Read {
            at: (latest, open),
            questions,
            newest,
        })
    }

    /// Take `read`, and `lines`, the board's lines after the message cursor the hub held before
    /// (none at its first observation): the newest reviewer's verdict among them is the board's
    /// newest. The region the feed sends when the asks changed, else none.
    pub(super) fn apply_asks(
        &self,
        read: Read,
        lines: &[symphony_board::Message],
    ) -> Option<Value> {
        let mut asked = self.task_asks.lock().recover();
        if let Some(newest) = read.newest {
            asked.verdict = newest;
            asked.followed = true;
        }
        if let Some(newest) = lines.iter().rev().find_map(taskview::verdict) {
            asked.verdict = Some(newest);
        }
        let asks = taskview::asks(read.questions.clone(), asked.verdict.as_ref());
        let (latest, open) = read.at;
        asked.counted = Some((latest, open, read.questions));
        if asked.sent.as_ref() == Some(&asks) {
            return None;
        }
        asked.sent = Some(asks.clone());
        drop(asked);
        Some(serde_json::to_value(symphony_api::Change::TaskAsks(asks)).expect("asks serialize"))
    }

    /// What the board asks of each person as the feed last sent it; none before its first
    /// observation. It is set before the event that sends it is published, so the project list,
    /// which reads it with the pages held, names beside it the feed's version of that event or of
    /// the one before, and a feed opened there hears it again.
    pub(crate) fn task_asks(&self) -> Option<TaskAsks> {
        let asked = self.task_asks.lock().recover();
        asked.sent.clone()
    }

    /// One observation of the board `store` serves, as the worker's tick makes it, with no agents:
    /// a test's.
    #[cfg(test)]
    pub(crate) fn observe(&self, store: &symphony_board::Store) {
        self.update_board(
            store,
            store.board().unwrap(),
            symphony_api::Agents {
                machine_work: symphony_api::MachineWork::default(),
                generated: store.now(),
                probes: symphony_api::AgentProbes {
                    panes: "ok".into(),
                    slots: "ok".into(),
                },
                agents: Vec::new(),
            },
        )
        .unwrap();
    }
}

#[cfg(test)]
#[path = "task_asks_tests.rs"]
mod tests;
