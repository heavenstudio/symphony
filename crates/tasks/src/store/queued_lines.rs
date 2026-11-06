//! The lines queued for an orchestrator and not yet delivered, judged again against what it hears
//! now: a hand-over since they were queued can take a task they name away from it. A line about
//! work it no longer hears is dropped, a LOST is said again of the work it still hears
//! ([`Queued::rescope`]), and a PULL naming such work is built again by the dispatcher
//! ([`Queued::unheard`]). Addressed messages and verdicts stay: they are their sender's words to
//! whoever held the role when they were sent, and a session taking a role over hears no earlier
//! ones, so dropping one would leave it said to nobody.

use super::callback_state::Entry;
use crate::cli::attach::{Overseen, Seen, overseen};
use crate::orchestration::{Owners, claimable_work, told};
use crate::{Board, Task};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};

/// An event queued for a recipient and not yet delivered.
#[derive(Debug, Deserialize)]
pub(super) struct Queued {
    pub(super) key: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    pull_entries: Option<BTreeSet<Entry>>,
    /// The entries a PULL's rows name. Older dispatchers name every entry they record.
    #[serde(default)]
    pull_told: Option<BTreeSet<Entry>>,
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
pub(super) struct Hearing<'b> {
    board: &'b Board,
    handle: &'b str,
    owners: Owners<'b>,
    /// Its look now ([`crate::cli::attach::seen`]): the tasks whose status it follows and the roles
    /// on those it holds one on.
    seen: &'b Seen,
    work: HashMap<&'b str, Vec<&'b Task>>,
    overseen: Overseen<'b>,
}

impl<'b> Hearing<'b> {
    pub(super) fn of(board: &'b Board, handle: &'b str, seen: &'b Seen) -> Hearing<'b> {
        Hearing {
            board,
            handle,
            owners: Owners::new(&board.tasks),
            seen,
            work: claimable_work(board),
            overseen: overseen(board, handle),
        }
    }

    /// Of `child`'s work, `held`, what the orchestrator is told of when it goes quiet.
    fn told(&self, child: &str, held: Vec<String>) -> Vec<String> {
        told(self.board, child, self.handle, held)
    }

    /// The work `child` holds that another could take.
    fn held(&self, child: &str) -> Vec<String> {
        let held = self.work.get(child).map_or(&[][..], Vec::as_slice);
        held.iter().map(|task| task.slug.clone()).collect()
    }

    /// Whether the orchestrator holds a role on `slug`.
    fn holds(&self, slug: &str) -> bool {
        (self.seen.roles.iter()).any(|(on, who, _)| on == slug && who == self.handle)
    }

    /// The queued lines, but PULL, that a hand-over took from it or narrowed.
    pub(super) fn rescope(&self, queued: &[Queued]) -> Rescoped {
        let mut rescoped = Rescoped::default();
        for line in queued.iter().filter(|line| !line.is_pull()) {
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

    /// Whether a PULL tells the recipient of a task it does not hear (`hears`) or of an entry
    /// `withheld` from it. An idle row names a session, which only `withheld` judges.
    pub(super) fn unheard(&self, hears: &dyn Fn(&str) -> bool, withheld: &BTreeSet<Entry>) -> bool {
        let told = (self.pull_told.as_ref()).or(self.pull_entries.as_ref());
        let unheard = |(list, name): &Entry| list != "idle" && !hears(name);
        (told.into_iter().flatten()).any(|entry| withheld.contains(entry) || unheard(entry))
    }

    /// What this line comes to for the orchestrator `now` hears: each kind as its dispatch decides
    /// whom to tell ([`crate::cli::attach::diff`], [`crate::cli::attach::pane_watch`]).
    fn rescope(&self, now: &Hearing) -> Now {
        let kept = |kept: bool| if kept { Now::Kept } else { Now::Dropped };
        let words: Vec<&str> = self.text.split(' ').collect();
        match words.as_slice() {
            ["NEW", slug, ..] => kept(now.owners.reaches(slug, now.handle)),
            ["STATUS", slug, ..] => kept(now.seen.statuses.contains_key(*slug)),
            ["ROLE", slug, ..] => kept(now.holds(slug) || self.moved_for(now.handle)),
            ["BLOCKED", who, slug, ..] => {
                kept(now.overseen.hears_of(now.board, &now.owners, who, slug))
            }
            ["STALE", child, ..] | ["LIVENESS", "UNKNOWN", child, ..] => {
                let held = now.held(child);
                kept(held.is_empty() || !now.told(child, held).is_empty())
            }
            ["LOST", child, ..] => {
                let Some((head, named)) = self.text.split_once(CLAIMABLE) else {
                    return Now::Kept;
                };
                let named: Vec<String> = named.split(", ").map(str::to_owned).collect();
                let heard = now.told(child, named.clone());
                if heard.is_empty() {
                    Now::Dropped
                } else if heard == named {
                    Now::Kept
                } else {
                    Now::Said(format!("{head}{CLAIMABLE}{}", heard.join(", ")))
                }
            }
            _ => Now::Kept,
        }
    }

    /// Whether this ROLE line says the orchestrator role moved to or from `handle`:
    /// `ROLE slug to orchestrator, status (from a, b)`.
    fn moved_for(&self, handle: &str) -> bool {
        let Some((line, from)) = self.text.rsplit_once(" (from ") else {
            return false;
        };
        let from = from.strip_suffix(')').unwrap_or(from);
        line.split(' ').nth(2) == Some(handle) || from.split(", ").any(|who| who == handle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rows::SessionRow;
    use crate::{Health, Role, Session};
    use serde_json::json;

    fn queued(event: serde_json::Value) -> Queued {
        serde_json::from_value(event).unwrap()
    }

    #[test]
    fn a_pull_is_unheard_when_it_names_a_task_or_entry_kept_from_the_recipient() {
        let hears = |slug: &str| slug.starts_with("t/mine");
        let none = BTreeSet::new();
        let entry = |list: &str, name: &str| (list.to_string(), name.to_string());
        let (mine, theirs, idle) = (
            entry("ready_unstaffed", "t/mine"),
            entry("ready_unstaffed", "t/theirs"),
            entry("idle", "calm-vole"),
        );
        // A newer dispatcher's PULL names only what it told; an older one's, every entry.
        let pull = queued(
            json!({"key":"pull-1", "pull_entries":[mine, theirs, idle], "pull_told":[mine, idle]}),
        );
        assert!(pull.is_pull(), "{pull:?}");
        assert!(
            !pull.unheard(&hears, &none),
            "{pull:?} names t/mine and calm-vole"
        );
        let withheld = BTreeSet::from([idle.clone()]);
        assert!(pull.unheard(&hears, &withheld), "{pull:?}, {withheld:?}");
        let older = queued(json!({"key":"pull-1", "pull_entries":[mine, idle]}));
        assert!(
            !older.unheard(&hears, &none),
            "a session is judged by `withheld` alone: {older:?}"
        );
        let older = queued(json!({"key":"pull-1", "pull_entries":[mine, theirs]}));
        assert!(older.unheard(&hears, &none), "{older:?} names t/theirs");
    }

    /// `orch` handed `gone` to `next`, and kept `kept`; nobody orchestrates `free`. `kid`,
    /// its child, executes `gone` and `kept`; `quiet`, its child too, executes `gone` alone;
    /// `idle`, its child, holds nothing.
    fn handed_over() -> Board {
        let task = |slug: &str, status: &str, roles: &[(&str, &str)]| Task {
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
            ],
            latest_message: 9,
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
            ("BLOCKED quiet gone w1:p2 — its pane waits", Now::Dropped),
            ("BLOCKED kid kept w1:p3 — its pane waits", Now::Kept),
            ("STALE quiet — no sign of it for 90 s", Now::Dropped),
            ("STALE kid — no sign of it for 90 s", Now::Kept),
            (
                "LIVENESS UNKNOWN quiet — native presence sweep overdue",
                Now::Dropped,
            ),
            ("STALE idle — no sign of it for 90 s", Now::Kept),
            ("LOST quiet — its work is claimable: gone", Now::Dropped),
            (
                "LOST kid — its work is claimable: gone, kept",
                Now::Said("LOST kid — its work is claimable: kept".into()),
            ),
            ("LOST kid — its work is claimable: kept", Now::Kept),
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
        assert!(!rescoped.is_empty() && Rescoped::default().is_empty());
    }
}
