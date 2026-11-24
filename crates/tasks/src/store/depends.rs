//! What a task waits on, changed after it was made: a dependent of a superseded task follows its
//! successor, and once nothing it waits on is cancelled it leaves the block a cancellation put it in.

use super::postgres::depends as pg;
use super::resume::back_to;
use super::statement::Statement;
use super::work::{BLOCK_LINE, no_task};
use super::{Error, Failure, Opens, Store, Task, Which, executors_or, refused};
use crate::slots::OPEN;
use crate::stages::UNBLOCK_LINE;
use serde::Deserialize;
use serde_json::json;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;

/// What put a blocked task in `blocked`, read back from its thread.
#[derive(Debug, PartialEq)]
enum Blocked<'a> {
    /// A move by hand, or nothing the thread records: it stays blocked.
    ByHand,
    /// A cancelled prerequisite, its line recording the status the task held.
    From(&'a str),
    /// A cancelled prerequisite, on a line from before the board recorded the status held.
    Unrecorded,
}

/// What blocked a task, from its system lines, oldest first: the newest line that blocked it, where
/// a block line recording `(was blocked)` defers to the one before it.
fn blocked_by(lines: &[String]) -> Blocked<'_> {
    let mut deferred = false;
    for line in lines.iter().rev() {
        if let Some(rest) = line.strip_prefix(BLOCK_LINE) {
            match rest
                .strip_suffix(')')
                .and_then(|rest| rest.rsplit_once(" (was "))
            {
                Some((_, "blocked")) => deferred = true,
                Some((_, was)) => return Blocked::From(was),
                None => return Blocked::Unrecorded,
            }
        } else if line.contains(" moved it ") {
            return Blocked::ByHand;
        }
    }
    if deferred {
        Blocked::Unrecorded
    } else {
        Blocked::ByHand
    }
}

/// Where a task a cancelled prerequisite blocked goes back to once nothing it waits on is
/// cancelled, `executing` whether an executor holds it, with what its line adds to `back to TO`:
/// where a resume sends it ([`back_to`]), but backlog work goes back to the backlog, which a
/// resume leaves blocked, and a block line that recorded no status says so. One blocked by hand
/// stays blocked.
fn unblocked_to(blocked: &Blocked<'_>, executing: bool) -> Option<(&'static str, &'static str)> {
    match *blocked {
        Blocked::From("backlog") => Some(("backlog", "")),
        Blocked::From(was) => back_to(Some(was), executing).map(|to| (to, "")),
        Blocked::Unrecorded => {
            back_to(None, executing).map(|to| (to, " (its block line recorded no earlier status)"))
        }
        Blocked::ByHand => None,
    }
}

/// Whether `from` waits on `to`, itself or through the tasks it waits on.
fn waits_on(deps: &HashMap<&str, Vec<&str>>, from: &str, to: &str) -> bool {
    let mut seen: HashSet<&str> = HashSet::new();
    let mut next = vec![from];
    while let Some(at) = next.pop() {
        if at == to {
            return true;
        }
        if seen.insert(at) {
            next.extend(deps.get(at).into_iter().flatten());
        }
    }
    false
}

impl Store {
    /// `slug` waits on `add` too and no longer on `remove`, on its thread. A task a cancelled
    /// prerequisite blocked that then waits on nothing cancelled goes back to the status its block line
    /// recorded; one blocked by hand stays blocked. Back to in_progress or review starts it again: the
    /// change is refused when that raises an orchestrator's open tasks past the cap
    /// ([`crate::slots`]). A hold on the task ends.
    pub fn change_depends(
        &self,
        slug: &str,
        add: &[String],
        remove: &[String],
        by: &str,
    ) -> Result<Task, Error> {
        #[derive(Deserialize)]
        struct Read {
            tasks: Vec<(String, String)>,
            deps: Vec<(String, String)>,
            edits: i64,
            version: Option<i64>,
            lines: Vec<String>,
            executing: bool,
        }
        let add: BTreeSet<&str> = add.iter().map(String::as_str).collect();
        let remove: BTreeSet<&str> = remove.iter().map(String::as_str).collect();
        if add.is_empty() && remove.is_empty() {
            return Err(refused(
                "name a dependency to add, remove or replace".into(),
            ));
        }
        if let Some(both) = add.intersection(&remove).next() {
            return Err(refused(format!("{both} cannot be both added and removed")));
        }
        let acts = !self.is_person(by);
        let mut said = format!("{by} changed its dependencies:");
        for (verb, slugs) in [("removed", &remove), ("added", &add)] {
            if !slugs.is_empty() {
                let named: Vec<&str> = slugs.iter().copied().collect();
                let _ = write!(said, " {verb} {};", named.join(", "));
            }
        }
        let said = said.trim_end_matches(';').to_string();
        self.retrying(|| {
            let read: Read =
                self.read_together(Statement::from(pg::READ), json!({"slug": slug}))?;
            let statuses: HashMap<&str, &str> = (read.tasks.iter())
                .map(|(slug, status)| (slug.as_str(), status.as_str()))
                .collect();
            let status = *statuses
                .get(slug)
                .ok_or_else(|| Failure::Stop(no_task(slug)))?;
            if status == "done" || status == "cancelled" {
                return Err(Failure::Stop(refused(format!("{slug} is {status}"))));
            }
            let mut deps: HashMap<&str, Vec<&str>> = HashMap::new();
            for (from, to) in &read.deps {
                deps.entry(from.as_str()).or_default().push(to.as_str());
            }
            let mine: BTreeSet<&str> = deps.get(slug).into_iter().flatten().copied().collect();
            if let Some(gone) = remove.iter().find(|r| !mine.contains(*r)) {
                return Err(Failure::Stop(refused(format!(
                    "{slug} does not depend on {gone}"
                ))));
            }
            for &new in &add {
                let Some(&state) = statuses.get(new) else {
                    return Err(Failure::Stop(no_task(new)));
                };
                let why = if mine.contains(new) {
                    format!("{slug} already depends on {new}")
                } else if state == "cancelled" {
                    format!("{new} is cancelled: nothing waiting on it can start")
                } else if new == slug {
                    format!("{slug} cannot wait on itself")
                } else if waits_on(&deps, new, slug) {
                    format!("{slug} cannot wait on {new}: {new} already waits on {slug}")
                } else {
                    continue;
                };
                return Err(Failure::Stop(refused(why)));
            }
            let kept: BTreeSet<&str> = mine.difference(&remove).chain(&add).copied().collect();
            let watch: Vec<_> = (kept.iter())
                .map(|dep| json!({"slug": dep, "status": statuses.get(dep)}))
                .collect();
            let cancelled = kept
                .iter()
                .any(|dep| statuses.get(dep) == Some(&"cancelled"));
            let back = if status != "blocked" || cancelled {
                None
            } else {
                unblocked_to(&blocked_by(&read.lines), read.executing)
            };
            // Back to work or review it was blocked from starts it again, as a move would: refused
            // past the cap.
            let slots = match (back, self.slots()) {
                (Some((to, _)), Some(cap)) if OPEN.contains(&to) => {
                    let view = self.view(&Which::Starting(slug))?;
                    let opens = Opens {
                        status: Some(to.into()),
                        by: executors_or(view.executors(), Some(by)),
                        what: format!("{slug} cannot go from blocked back to {to}"),
                        ..Opens::default()
                    };
                    self.slots_taken(&view, slug, &opens, cap)?
                }
                _ => Vec::new(),
            };
            let guarded = !slots.is_empty();
            let vars = self.acting(
                by,
                (self.clock)(),
                json!({
                    "slug": slug, "edits": read.edits, "version": read.version, "watch": watch,
                    "add": add, "drop": remove, "said": said,
                    "status": back.map(|(to, _)| to),
                    "back": back.map(|(to, why)| {
                        format!("{UNBLOCK_LINE}{to}{why}")
                    }),
                    "slots": slots,
                }),
            );
            let body = Statement::from(pg::change(acts, guarded)).posting();
            self.written(body, vars, slug)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(texts: &[&str]) -> Vec<String> {
        texts.iter().map(|t| (*t).to_string()).collect()
    }

    #[test]
    fn the_newest_block_line_names_the_status_held_before_it() {
        let thread = lines(&[
            "created by owner",
            "amber-heron moved it backlog → ready",
            "blocked: cancelled dependencies: sim/a (was ready)",
            "amber-heron joined as orchestrator",
        ]);
        assert_eq!(blocked_by(&thread), Blocked::From("ready"));
        let moved_since = lines(&[
            "blocked: cancelled dependencies: sim/a (was ready)",
            "amber-heron moved it blocked → design",
            "blocked: cancelled dependencies: sim/a (was design)",
        ]);
        assert_eq!(blocked_by(&moved_since), Blocked::From("design"));
    }

    #[test]
    fn a_block_of_blocked_work_defers_to_what_blocked_it_first() {
        let twice = lines(&[
            "blocked: cancelled dependencies: sim/a (was review)",
            "blocked: cancelled dependencies: sim/b (was blocked)",
            "owner changed its dependencies: removed sim/a; added sim/c",
        ]);
        assert_eq!(blocked_by(&twice), Blocked::From("review"));
        let by_hand_first = lines(&[
            "amber-heron moved it ready → blocked",
            "blocked: cancelled dependencies: sim/a (was blocked)",
        ]);
        assert_eq!(blocked_by(&by_hand_first), Blocked::ByHand);
        let legacy_first = lines(&[
            "blocked: cancelled dependencies: sim/a",
            "blocked: cancelled dependencies: sim/b (was blocked)",
        ]);
        assert_eq!(blocked_by(&legacy_first), Blocked::Unrecorded);
        let nothing_first = lines(&["blocked: cancelled dependencies: sim/b (was blocked)"]);
        assert_eq!(blocked_by(&nothing_first), Blocked::Unrecorded);
    }

    #[test]
    fn a_block_line_from_before_the_record_or_a_move_by_hand_names_no_status() {
        let legacy = lines(&[
            "created by teal-owl",
            "blocked: cancelled dependencies: game/lineup-pilot",
        ]);
        assert_eq!(blocked_by(&legacy), Blocked::Unrecorded);
        let by_hand = lines(&[
            "blocked: cancelled dependencies: sim/a (was ready)",
            "unblocked: no cancelled dependency left; back to ready",
            "amber-heron moved it ready → blocked",
        ]);
        assert_eq!(blocked_by(&by_hand), Blocked::ByHand);
        assert_eq!(blocked_by(&lines(&["created by owner"])), Blocked::ByHand);
        assert_eq!(blocked_by(&[]), Blocked::ByHand);
    }

    #[test]
    fn a_task_unblocked_goes_back_to_where_its_block_found_it_and_backlog_to_the_backlog() {
        for was in ["concept", "design", "ready", "review"] {
            for executing in [false, true] {
                assert_eq!(
                    unblocked_to(&Blocked::From(was), executing),
                    Some((was, ""))
                );
            }
        }
        for executing in [false, true] {
            assert_eq!(
                unblocked_to(&Blocked::From("backlog"), executing),
                Some(("backlog", ""))
            );
            // A resume leaves blocked backlog where it is.
            assert_eq!(back_to(Some("backlog"), executing), None);
            for was in ["done", "cancelled", "blocked", "elsewhere"] {
                assert_eq!(unblocked_to(&Blocked::From(was), executing), None, "{was}");
            }
            assert_eq!(unblocked_to(&Blocked::ByHand, executing), None);
        }
        // Work in progress goes back to it only with an executor still on it.
        assert_eq!(
            unblocked_to(&Blocked::From("in_progress"), true),
            Some(("in_progress", ""))
        );
        assert_eq!(
            unblocked_to(&Blocked::From("in_progress"), false),
            Some(("ready", ""))
        );
        let unrecorded = " (its block line recorded no earlier status)";
        assert_eq!(
            unblocked_to(&Blocked::Unrecorded, true),
            Some(("in_progress", unrecorded))
        );
        assert_eq!(
            unblocked_to(&Blocked::Unrecorded, false),
            Some(("ready", unrecorded))
        );
    }

    #[test]
    fn waiting_follows_dependencies_through_others_and_round_a_loop() {
        let deps: HashMap<&str, Vec<&str>> =
            [("a", vec!["b"]), ("b", vec!["c", "d"]), ("d", vec!["b"])]
                .into_iter()
                .collect();
        assert!(waits_on(&deps, "a", "c"));
        assert!(waits_on(&deps, "d", "c"));
        assert!(waits_on(&deps, "a", "a"));
        assert!(!waits_on(&deps, "c", "a"));
        assert!(!waits_on(&deps, "b", "a"));
        assert!(!waits_on(&deps, "unknown", "a"));
    }
}
