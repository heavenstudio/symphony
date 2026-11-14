//! A task's stamps as its gates (docs/analysis/needed-stamps.md, "The task page"): each stamp of the
//! project's catalogue (`Catalogue::at`), needed at its branch's tip (`read::needed`) and read there
//! by its reader, met, missing or, for `owner`, held; or left out by the task's assessment, waived,
//! the assessment its reason. `owner` is held too while a card the task posted waits ([`owner`]),
//! and `review` reads the rounds ([`review`]), met only while the last approve names the tip
//! ([`review_at`]). A task done or cancelled with no assessment keeps the gates it had before
//! stamps ([`stamped`]): no history is judged again. A done task whose branch the sweep removed is
//! read at the tip its land merged ([`landed_tip`]); a closed task with no tip to read at reads its
//! needed stamps unknown, never missing. The task's timeline draws no lane for a stamp left out
//! ([`left_out`]).
//!
//! Until the task page is ported, the owner gives the `owner` stamp through the page's approval
//! offer: [`owner_offer`] makes the Symphony's own where no plugin offers one, and its action posts
//! the owner's line `approve TIP` ([`approve_line`]), which `read::owner` reads.

use std::path::Path;

use symphony_api::gates::{OWNER, REVIEW};
use symphony_api::{Gate, GateState, PendingItem, TaskOffer};
use symphony_board::cli::machine_work::recorded::Record;
use symphony_board::stamps::read::{self, Reading};
use symphony_board::stamps::{Catalogue, Needs, READERS, said, short};
use symphony_board::{Message, Store, Task};

use super::{Round, covered_close, first_line, owner, review, review_of, review_rounds};

/// The stamp a task's recorded runs at the tip meet.
pub const TESTS: &str = "tests";
/// The Symphony's own board action: the owner's approve of a task's tip ([`owner_offer`]).
pub const OWNER_APPROVE: &str = "owner-approve";
/// How a stamp the task's assessment left out starts its detail, the assessment's reading after.
const LEFT_OUT: &str = "Left out by the assessment";
/// The detail of a stamp the catalogue does not need by default, on a task no assessment read.
const BY_DEFAULT: &str = "Not needed by default: no assessment yet";

/// Whether the gates of `record` are its stamps: a top-level task, open or assessed. A subtask has
/// none, as it had no review gate; a task done or cancelled with no assessment keeps the reading it
/// had before stamps, its review rounds and the cards it posted.
pub fn stamped(record: &Task) -> bool {
    let closed = matches!(record.status.as_str(), "done" | "cancelled");
    record.parent.is_none() && (!closed || record.needs().is_some())
}

/// What a task's stamps are read from.
#[derive(Clone, Copy, Debug)]
pub struct Evidence<'a> {
    pub thread: &'a [Message],
    /// The recorder's runs.
    pub runs: &'a [Record],
    /// The task and the work under it, whose runs are the task's ([`family`]).
    pub ours: &'a [String],
    /// The task's artifacts folder, where its pictures go (`read::folder`).
    pub folder: &'a str,
    /// The branch its gates are answered for, and the tip its stamps are read at: that branch's,
    /// or, a done task's whose branch is gone, the one its land merged ([`landed_tip`]); `None`
    /// with neither.
    pub branch: &'a str,
    pub tip: Option<&'a str>,
    /// The cards the task posted that wait on the owner ([`super::posted_cards`]).
    pub cards: &'a [&'a PendingItem],
}

/// Each stamp of `catalogue` as a gate of the task `record`, in the catalogue's order, from
/// `evidence`; with no `owner` stamp in it, the cards the task posted hold it as before.
pub fn stamps(record: &Task, catalogue: &Catalogue, evidence: &Evidence<'_>) -> Vec<Gate> {
    let needs = record.needs();
    let needed = read::needed(catalogue, needs.as_ref(), evidence.tip);
    let defaults = catalogue.defaults();
    let (slug, status) = (record.slug.as_str(), record.status.as_str());
    let closed = matches!(status, "done" | "cancelled");
    let reviewers = record.holders("reviewer");
    let read_at = |name: &str, unmet: GateState, read: &dyn Fn(&str) -> Reading| match evidence.tip
    {
        Some(tip) => of_reading(name, slug, read(tip), unmet),
        None => no_tip(name, unmet, evidence.branch, closed),
    };
    let mut gates: Vec<Gate> = (catalogue.stamps.iter())
        .map(|stamp| {
            let name = stamp.name;
            let wanted = needed.iter().any(|need| need == name);
            let left_out = || waived(name, slug, needs.as_ref(), &defaults, evidence.thread);
            match name {
                // A task done by a covered close was reviewed by none: it says so, needed or not.
                REVIEW if status == "done" && covered_close(evidence.thread).is_some() => {
                    review_of(slug, status, &reviewers, evidence.thread)
                }
                REVIEW if wanted => {
                    let rounds = review_rounds(evidence.thread);
                    let gate = review(slug, status, &reviewers, &rounds);
                    review_at(gate, &rounds, evidence.branch, evidence.tip, closed)
                }
                OWNER => match owner(evidence.cards) {
                    Some(held) => held,
                    None if wanted => read_at(name, GateState::Held, &|tip| {
                        read::owner(evidence.thread, tip)
                    }),
                    None => left_out(),
                },
                _ if !wanted => left_out(),
                TESTS => read_at(name, GateState::Missing, &|tip| {
                    read::tests(evidence.runs, tip, |task| {
                        evidence.ours.iter().any(|ours| ours == task)
                    })
                }),
                _ => read_at(name, GateState::Missing, &|tip| {
                    read::pictures(evidence.thread, tip, evidence.folder)
                }),
            }
        })
        .collect();
    // A catalogue with no `owner` stamp still holds the task while a card it posted waits.
    if !catalogue.names().contains(&OWNER) {
        gates.extend(owner(evidence.cards));
    }
    gates
}

/// The review stamp at `tip` ([`review`]'s gate from its `rounds`; `None` with no tip, as the task
/// is `closed` or not, [`without_tip`]): met only while the last round's approve names the tip,
/// else missing, naming the commit it approved; a task done with no round landed without review,
/// which its assessment did not waive: missing.
pub fn review_at(
    gate: Gate,
    rounds: &[Round],
    branch: &str,
    tip: Option<&str>,
    closed: bool,
) -> Gate {
    match (gate.state, rounds.last()) {
        (GateState::Waived, None) => Gate {
            state: GateState::Missing,
            why: None,
            detail: "Landed with no review verdict".into(),
            ..gate
        },
        (GateState::Met, Some(last)) => {
            let Some(tip) = tip else {
                let (state, nor) = without_tip(GateState::Missing, closed);
                return Gate {
                    state,
                    detail: format!(
                        "{}, but no branch {branch} is in this repository{nor}",
                        gate.detail
                    ),
                    ..gate
                };
            };
            if read::names(&last.said, tip) {
                return Gate {
                    commit: Some(tip.to_string()),
                    ..gate
                };
            }
            let (commit, approved) = match read::named(&last.said) {
                Some(commit) => (Some(commit.to_string()), format!("at {}", short(commit))),
                None => (None, "naming no commit".into()),
            };
            Gate {
                state: GateState::Missing,
                commit,
                detail: format!("{} {approved}, not the tip {}", gate.detail, short(tip)),
                ..gate
            }
        }
        _ => gate,
    }
}

/// What the stamp `name` of the task `slug` reads (`reading`) as its gate: met, else `unmet`.
fn of_reading(name: &str, slug: &str, reading: Reading, unmet: GateState) -> Gate {
    let state = if reading.met { GateState::Met } else { unmet };
    Gate {
        name: name.into(),
        state,
        by: reading.by,
        at: reading.at,
        commit: reading.commit,
        why: (state == GateState::Held).then(owner_waits),
        detail: reading.detail,
        link: reading.line.map(|id| format!("/task/{slug}#message-{id}")),
        past: Vec::new(),
    }
}

/// Why a needed `owner` stamp holds the task.
fn owner_waits() -> String {
    "the task needs the owner's approve of its tip, which its page offers".into()
}

/// The stamp `name` with no tip to read it at: the repository has no branch `branch`, and a
/// `closed` task's thread names no commit its land merged ([`without_tip`]).
fn no_tip(name: &str, unmet: GateState, branch: &str, closed: bool) -> Gate {
    let (state, nor) = without_tip(unmet, closed);
    Gate {
        name: name.into(),
        state,
        why: (state == GateState::Held).then(owner_waits),
        detail: format!("No branch {branch} in this repository{nor}"),
        ..Gate::default()
    }
}

/// What a needed stamp with no tip to read it at is, and what its detail adds to the missing
/// branch: an open task's stands on nothing yet, `unmet`; a closed task's stood on a branch that
/// is gone, with no land on its thread naming the commit it merged, so where it stood cannot be
/// read: unknown, never missing.
fn without_tip(unmet: GateState, closed: bool) -> (GateState, &'static str) {
    if closed {
        (
            GateState::Unknown,
            ", nor a land on its thread naming the commit it merged",
        )
    } else {
        (unmet, "")
    }
}

/// The tip the land of the task `slug` merged, a whole commit id in the repository at `root`, read
/// from its `thread` once the sweep has removed its branch: the second parent of the merge its
/// landing's line names (`landed SLUG on DEFAULT at MERGE`, `symphony_board::landing`), else the
/// commit the land's own line names (`land SLUG: merged TIP as MERGE`). `None` when the thread
/// names neither or the repository holds neither commit.
pub fn landed_tip(root: &Path, slug: &str, thread: &[Message]) -> Option<String> {
    let merged = format!("land {slug}: merged ");
    let from_merge = || super::commit(root, &format!("{}^2", symphony_board::landing(thread)?));
    let from_land = || {
        (thread.iter().rev())
            .filter_map(|line| line.text.strip_prefix(merged.as_str()))
            .find_map(|rest| read::named(rest.split_whitespace().next()?))
            .and_then(|tip| super::commit(root, tip))
    };
    from_merge().or_else(from_land)
}

/// The stamp `name` of the task `slug` its assessment `needs` left out: waived, its reason the
/// assessment's, its assessor the who, its line on the `thread` the when and the link. With no
/// assessment, a stamp the catalogue does not need by default (`defaults`).
fn waived(
    name: &str,
    slug: &str,
    needs: Option<&Needs>,
    defaults: &[String],
    thread: &[Message],
) -> Gate {
    let Some(needs) = needs else {
        return Gate {
            name: name.into(),
            state: GateState::Waived,
            why: Some(format!(
                "a task no assessment has read needs only the catalogue's defaults: {}",
                said(defaults)
            )),
            detail: BY_DEFAULT.into(),
            ..Gate::default()
        };
    };
    // The newest line an assessment by its assessor wrote (`Needs::line`).
    let by = format!("; by {}", needs.by);
    let line = (thread.iter()).rfind(|line| {
        line.kind == "system" && line.text.starts_with("needs ") && line.text.ends_with(&by)
    });
    Gate {
        name: name.into(),
        state: GateState::Waived,
        by: Some(needs.by.clone()),
        at: line.map(|line| line.ts),
        commit: needs.tip.clone(),
        why: Some(needs.why.clone()),
        detail: format!("{LEFT_OUT} {}", needs.read()),
        link: line.map(|line| format!("/task/{slug}#message-{}", line.id)),
        past: Vec::new(),
    }
}

/// Whether `gate` is a stamp left out ([`waived`]): by the task's assessment, or by default with
/// none. The task page's gates list it; its timeline draws no lane for it, which would say the
/// same thing over the task's whole life.
pub fn left_out(gate: &Gate) -> bool {
    gate.state == GateState::Waived
        && READERS.contains(&gate.name.as_str())
        && (gate.detail.starts_with(LEFT_OUT) || gate.detail == BY_DEFAULT)
}

/// The gate saying the project's catalogue cannot be read (`why`): its stamps are unknown, and the
/// task's other gates are read as before stamps.
pub fn unreadable(why: &str) -> Gate {
    Gate {
        name: "stamps".into(),
        state: GateState::Unknown,
        detail: first_line(why).to_string(),
        ..Gate::default()
    }
}

/// The task `slug` and every task under it, at any depth, whose runs are its own: read a level at a
/// time, so a task with no subtask costs one small read. A level that cannot be read adds nothing.
pub fn family(store: &Store, slug: &str) -> Vec<String> {
    let mut found = vec![slug.to_string()];
    let mut next = 0;
    while let Some(at) = found.get(next).cloned() {
        for child in store.children(&at).unwrap_or_default() {
            if !found.contains(&child.slug) {
                found.push(child.slug);
            }
        }
        next += 1;
    }
    found
}

/// The owner's approve the task page offers on `record`, its branch `branch` in the checkout
/// `root`: while the task is open and top-level, has a tip, and needs its `owner` stamp there,
/// which no approve of the owner's meets yet. Its payload is the whole tip, which
/// [`OWNER_APPROVE`]'s action posts as the owner's line ([`approve_line`]).
pub fn owner_offer(store: &Store, record: &Task, root: &Path, branch: &str) -> Option<TaskOffer> {
    if record.parent.is_some() || matches!(record.status.as_str(), "done" | "cancelled") {
        return None;
    }
    let tip = super::tip(root, branch)?;
    let catalogue = Catalogue::at(root).ok()?;
    let needed = read::needed(&catalogue, record.needs().as_ref(), Some(&tip));
    if !needed.iter().any(|need| need == OWNER) {
        return None;
    }
    let thread = store.thread(&record.slug).ok()?;
    if read::owner(&thread, &tip).met {
        return None;
    }
    Some(TaskOffer {
        action: OWNER_APPROVE.into(),
        label: format!("Approve {}", short(&tip)),
        branch: branch.into(),
        payload: Some(tip),
    })
}

/// The owner's line [`OWNER_APPROVE`]'s action posts for `payload`, the commit the offer showed:
/// `approve COMMIT`. Refused unless it is one commit's id (7 to 40 lowercase hex digits).
pub fn approve_line(payload: &str) -> Result<String, String> {
    let commit = payload.trim();
    if read::named(commit) != Some(commit) {
        return Err(format!(
            "the owner's approve names the commit it approves, 7 to 40 hex digits: not {commit:?}"
        ));
    }
    Ok(format!("approve {commit}"))
}

#[cfg(test)]
mod tests;
