//! The lines queued for an orchestrator and not yet delivered, judged again against what it hears
//! now: a hand-over since they were queued can take what they name away from it. Each line is put
//! to the test a dispatch makes of whom to tell it ([`Queued::rescope`]), on the recipient's look
//! now: one it would not be told now is dropped, and a LOST is said again of the work it is still
//! told of. A PULL is never judged by what it says: the dispatcher builds it again from the
//! recipient's digest now, and says that or nothing ([`pull_plan`]).
//!
//! [`pull_plan`]: super::pull_account::pull_plan
//!
//! | Line | Said to the recipient now when |
//! |---|---|
//! | NEW | it hears the task ([`Owners::reaches`], as its look takes in ready work) |
//! | STATUS | its look hears the move ([`Seen::hears_move`]) |
//! | ROLE | it holds a role on the task ([`Seen::hears_roles`]) |
//! | ROLE of a move | it is a party to the move ([`moved_for`]) |
//! | GIVEN | always: the board gave it those tasks, and a later move of one says itself |
//! | BLOCKED | it oversees the session on that task ([`Overseen::hears_of`]) |
//! | STALE, LIVENESS UNKNOWN | it is told of the session going quiet ([`Seen::hears_quiet`]) |
//! | LOST | that, and of the work named, what it is told of ([`told`]) |
//! | PULL | its digest now names an entry no PULL it acknowledged told it ([`pull_plan`]) |
//!
//! Addressed messages and verdicts stay: they are their sender's words to whoever held the role
//! when they were sent, and a session taking a role over hears no earlier ones, so dropping one
//! would leave it said to nobody.

use super::callback_state::Entry;
use crate::Board;
use crate::cli::attach::{Overseen, Seen, overseen};
use crate::orchestration::{Owners, moved_for, told};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// An event queued for a recipient and not yet delivered. A PULL also carries the entries its
/// acknowledgement records: every entry of the whole digest, and, as this generation writes it,
/// those it does not tell.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct Queued {
    pub(super) key: String,
    pub(super) text: String,
    pub(super) sequence: u64,
    pub(super) pull_entries: Option<BTreeSet<Entry>>,
    pub(super) pull_withheld: Option<BTreeSet<Entry>>,
    /// What a PULL this generation queued says, which its acknowledgement records.
    pub(super) pull_said: Option<crate::cli::pull::Said>,
}

/// The queued lines a dispatch takes back (`dropped`) or says again (`rewritten`, key and text).
#[derive(Debug, Default, PartialEq, Serialize)]
pub(super) struct Rescoped {
    pub(super) dropped: Vec<String>,
    pub(super) rewritten: Vec<(String, String)>,
}

impl Rescoped {
    pub(super) fn is_empty(&self) -> bool {
        self.dropped.is_empty() && self.rewritten.is_empty()
    }
}

/// What one queued line comes to now.
#[derive(Debug, PartialEq)]
enum Now {
    Kept,
    Dropped,
    Said(String),
}

/// What an orchestrator hears now, to judge its queued lines by.
pub(super) struct Hearing<'board> {
    board: &'board Board,
    handle: &'board str,
    owners: Owners<'board>,
    /// Its look now ([`crate::cli::attach::seen`]), as a dispatch now takes it.
    seen: &'board Seen,
    overseen: Overseen<'board>,
}

impl<'board> Hearing<'board> {
    pub(super) fn of(
        board: &'board Board,
        handle: &'board str,
        seen: &'board Seen,
    ) -> Hearing<'board> {
        Hearing {
            board,
            handle,
            owners: Owners::new(&board.tasks),
            seen,
            overseen: overseen(board, handle),
        }
    }

    /// The queued lines, but PULL, that a hand-over took from it or narrowed.
    pub(super) fn rescope(&self, queued: &[Queued]) -> Rescoped {
        let mut rescoped = Rescoped::default();
        let lines = queued.iter().filter(|line| !line.is_pull());
        for line in lines {
            match line.rescope(self) {
                Now::Kept => {}
                Now::Dropped => rescoped.dropped.push(line.key.clone()),
                Now::Said(text) => rescoped.rewritten.push((line.key.clone(), text)),
            }
        }
        rescoped
    }
}

const CLAIMABLE: &str = " — its work is claimable: ";

impl Queued {
    pub(super) fn is_pull(&self) -> bool {
        self.key.starts_with("pull-")
    }

    /// What this line comes to for the orchestrator `now` hears: whether a dispatch would tell it
    /// this line now, by the test that dispatch makes ([`crate::cli::attach::diff`],
    /// [`crate::cli::attach::pane_watch`]); the table above names each.
    fn rescope(&self, now: &Hearing) -> Now {
        let kept = |kept: bool| if kept { Now::Kept } else { Now::Dropped };
        let words: Vec<&str> = self.text.split(' ').collect();
        match words.as_slice() {
            ["NEW", slug, ..] => kept(now.owners.reaches(slug, now.handle)),
            ["STATUS", slug, .., status] => kept(now.seen.hears_move(slug, status)),
            ["ROLE", slug, ..] => kept(match self.role_move() {
                Some((from, to)) => moved_for(&from, to, now.handle),
                None => now.seen.hears_roles(slug),
            }),
            ["BLOCKED", who, slug, ..] => {
                kept(now.overseen.hears_of(now.board, &now.owners, who, slug))
            }
            ["STALE", child, ..] | ["LIVENESS", "UNKNOWN", child, ..] => {
                kept(now.seen.hears_quiet(child))
            }
            ["LOST", child, ..] => {
                let Some((head, named)) = self.text.split_once(CLAIMABLE) else {
                    return kept(now.seen.hears_quiet(child));
                };
                let named: Vec<String> = named.split(", ").map(str::to_owned).collect();
                let heard = told(now.board, child, now.handle, named.clone());
                if !now.seen.hears_quiet(child) || heard.is_empty() {
                    Now::Dropped
                } else if heard == named {
                    Now::Kept
                } else {
                    Now::Said(format!("{head}{CLAIMABLE}{}", heard.join(", ")))
                }
            }
            // GIVEN, addressed messages and verdicts.
            _ => Now::Kept,
        }
    }

    /// The move of the orchestrator role a ROLE line says, as the sessions it left and the one it
    /// reached: `ROLE slug to orchestrator, status (from a, b)`. None for any other ROLE line.
    fn role_move(&self) -> Option<(Vec<&str>, &str)> {
        let (line, from) = self.text.rsplit_once(" (from ")?;
        let from = from.strip_suffix(')').unwrap_or(from);
        Some((from.split(", ").collect(), line.split(' ').nth(2)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rows::SessionRow;
    use crate::{Health, Role, Session, Task};
    use serde_json::json;

    fn queued(event: serde_json::Value) -> Queued {
        serde_json::from_value(event).unwrap()
    }

    /// `orch` handed `gone` to `next`, and kept `kept`; nobody orchestrates `free`. `kid`,
    /// its child, executes `gone` and `kept`; `quiet`, its child too, executes `gone` alone;
    /// `idle`, its child, holds nothing, and so does `theirs`, the child of `next`.
    fn handed_over() -> Board {
        let task = |slug: &str, status: &str, roles: &[(&str, &str)]| Task {
            branch: None,
            id: 0,
            slug: slug.into(),
            title: "T".into(),
            body: String::new(),
            status: status.into(),
            priority: 2,
            parent: slug.rsplit_once('/').map(|(parent, _)| parent.to_owned()),
            created_by: String::new(),
            created: 0.0,
            updated: 0.0,
            links: Vec::new(),
            depends: Vec::new(),
            roles: (roles.iter())
                .map(|(handle, role)| Role {
                    handle: handle.to_string(),
                    role: role.to_string(),
                    since: 0.0,
                    expires: None,
                    health: Health::Live,
                })
                .collect(),
            instruction: false,
            fields: Default::default(),
        };
        let session = |handle: &str, kind: &str, parent: Option<&str>| Session {
            row: SessionRow {
                handle: handle.into(),
                kind: kind.into(),
                parent: parent.map(str::to_owned),
                ..SessionRow::default()
            },
            health: Health::Live,
        };
        let gone = [
            ("next", "orchestrator"),
            ("kid", "executor"),
            ("quiet", "executor"),
        ];
        Board {
            tasks: vec![
                task("gone", "in_progress", &gone),
                task("gone/sub", "ready", &[]),
                task(
                    "kept",
                    "in_progress",
                    &[("orch", "orchestrator"), ("kid", "executor")],
                ),
                task("free", "ready", &[]),
            ],
            sessions: vec![
                session("orch", "orchestrator", None),
                session("next", "orchestrator", None),
                session("kid", "worker", Some("orch")),
                session("quiet", "worker", Some("orch")),
                session("idle", "worker", Some("orch")),
                session("theirs", "worker", Some("next")),
            ],
            latest_message: 9,
            workflow_generation: None,
        }
    }

    #[test]
    fn a_hand_over_takes_back_each_queued_line_about_the_work_it_moved_and_no_other() {
        let board = handed_over();
        let seen = crate::cli::attach::seen(&board, "orch", "orchestrator");
        let now = Hearing::of(&board, "orch", &seen);
        let lines = [
            ("NEW gone/sub ready — T", Now::Dropped),
            ("NEW free ready — T", Now::Kept),
            ("STATUS gone ready → in_progress", Now::Dropped),
            ("STATUS kept ready → in_progress", Now::Kept),
            // Work nobody orchestrates is heard only as it blocks.
            ("STATUS free backlog → ready", Now::Dropped),
            ("STATUS free ready → blocked", Now::Kept),
            ("ROLE gone kid executor, in_progress", Now::Dropped),
            ("ROLE kept kid executor, in_progress", Now::Kept),
            (
                "ROLE gone next orchestrator, in_progress (from orch)",
                Now::Kept,
            ),
            (
                "ROLE gone orch orchestrator, in_progress (from next, other)",
                Now::Kept,
            ),
            // A move is said to its parties, not to every holder of a role on the task.
            (
                "ROLE kept other orchestrator, in_progress (from next)",
                Now::Dropped,
            ),
            ("BLOCKED quiet gone t2 — its terminal waits", Now::Dropped),
            ("BLOCKED kid kept t3 — its terminal waits", Now::Kept),
            ("STALE quiet — no sign of it for 90 s", Now::Dropped),
            ("STALE kid — no sign of it for 90 s", Now::Kept),
            (
                "LIVENESS UNKNOWN quiet — native presence sweep overdue",
                Now::Dropped,
            ),
            // A child holding no work is its parent's to hear of, and nobody else's.
            ("STALE idle — no sign of it for 90 s", Now::Kept),
            ("STALE theirs — no sign of it for 90 s", Now::Dropped),
            ("LOST quiet — its work is claimable: gone", Now::Dropped),
            // Work it named that `orch` still holds, but its work now is another's.
            ("LOST quiet — its work is claimable: kept", Now::Dropped),
            (
                "LOST kid — its work is claimable: gone, kept",
                Now::Said("LOST kid — its work is claimable: kept".into()),
            ),
            ("LOST kid — its work is claimable: kept", Now::Kept),
            ("LOST idle — its work is claimable: free", Now::Kept),
            ("LOST theirs — its work is claimable: free", Now::Dropped),
            ("MSG #7 gone from w — to @orchestrator", Now::Kept),
            ("VERDICT #8 gone from w — approve", Now::Kept),
            ("AGENT from w to orch — hello", Now::Kept),
        ];
        for (text, expected) in lines {
            let line = queued(json!({"key":"event-1-0", "text":text}));
            assert_eq!(line.rescope(&now), expected, "{text}");
        }
        let queued: Vec<Queued> = [
            json!({"key":"event-1-0", "text":"NEW gone/sub ready — T"}),
            json!({"key":"event-1-1", "text":"LOST kid — its work is claimable: gone, kept"}),
            json!({"key":"pull-1", "text":"PULL", "pull_entries":[["ready_unstaffed", "gone/sub"]]}),
            json!({"key":"message-7", "text":"MSG #7 gone from w — to @orchestrator"}),
        ]
        .into_iter()
        .map(queued)
        .collect();
        // A PULL is the dispatcher's to build again, from the digest.
        let rescoped = now.rescope(&queued);
        let expected = Rescoped {
            dropped: vec!["event-1-0".into()],
            rewritten: vec![(
                "event-1-1".into(),
                "LOST kid — its work is claimable: kept".into(),
            )],
        };
        assert_eq!(rescoped, expected);
        let empty = (rescoped.is_empty(), Rescoped::default().is_empty());
        assert_eq!(empty, (false, true), "{rescoped:?}");
    }
}
