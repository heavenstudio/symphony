//! A task's stamps as its gates (docs/workflow/analysis/needed-stamps.md, "The task page"): each stamp of the
//! project's catalogue (`Catalogue::at`), needed at its branch's tip (`read::needed`) and read there
//! by its reader, met, missing or, for `owner`, held; or left out by the task's assessment, waived,
//! the assessment its reason. `owner` is held too while a card the task posted waits ([`owner`]),
//! and `review` reads the rounds ([`review`]), met only while the last approve names the tip
//! ([`review_at`]). A stamp a gate decision waived for the tip (`symphony_board::gates`) is waived,
//! the decision's author, reason and tip its own ([`decided`]); one for another tip lifts nothing.
//! A task done or cancelled with no assessment keeps the gates it had before
//! stamps ([`stamped`]): no history is judged again. A done task whose branch the sweep removed is
//! read at the tip its land merged ([`landed_tip`]); a closed task with no tip to read at reads its
//! needed stamps unknown, never missing. The task's timeline draws no lane for a stamp left out
//! ([`left_out`]).
//!
//! Until the task page is ported, the owner gives the `owner` stamp through the page's approval
//! offer: [`owner_offer`] makes the Symphony's own where no plugin offers one, and its action posts
//! the owner's line `approve TIP` ([`approve_line`]), which `read::owner_at` reads, as it reads the
//! owner's approve of every card the task's branch posted (`symphony_board::stamps::posted`).

use std::path::Path;

use symphony_api::gates::{OWNER, REVIEW};
use symphony_api::{Gate, GateState, PendingItem, TaskOffer};
use symphony_board::cli::machine_work::recorded::Record;
use symphony_board::gates::{self, Decision};
use symphony_board::stamps::posted::Posted;
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
#[derive(Clone, Copy)]
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
    /// What the cards the task's branch posted say of its `owner` stamp at a tip
    /// (`symphony_board::stamps::posted::read`), asked only where the thread's approve does not
    /// name it ([`read::owner_with`]).
    pub posted: &'a dyn Fn(&str) -> Posted,
}

/// Each stamp of `catalogue` as a gate of the task `record`, in the catalogue's order, from
/// `evidence`; with no `owner` stamp in it, the cards the task posted hold it as before.
pub fn stamps(record: &Task, catalogue: &Catalogue, evidence: &Evidence<'_>) -> Vec<Gate> {
    let needs = record.needs();
    let needed = read::needed(catalogue, needs.as_ref(), evidence.tip);
    let decisions = record.gates();
    let (waivers, _) = gates::waivers(&decisions, catalogue, &needed, evidence.tip);
    let defaults = catalogue.defaults();
    let (slug, status) = (record.slug.as_str(), record.status.as_str());
    let reviewers = record.holders("reviewer");
    let read_at = |name: &str, unmet: GateState, read: &dyn Fn(&str) -> Reading| match evidence.tip
    {
        Some(tip) => of_reading(name, slug, read(tip), unmet),
        None => no_tip(name, unmet, evidence.branch, status),
    };
    let mut gates: Vec<Gate> = (catalogue.stamps.iter())
        .map(|stamp| {
            let name = stamp.name;
            let wanted = needed.iter().any(|need| need == name);
            let left_out = || waived(name, slug, needs.as_ref(), &defaults, evidence.thread);
            if let Some(waiver) = waivers.iter().find(|waiver| waiver.gate == name) {
                return decided(name, slug, waiver, evidence.thread);
            }
            match name {
                // A task done by a covered close was reviewed by none: it says so, needed or not.
                REVIEW if status == "done" && covered_close(evidence.thread).is_some() => {
                    review_of(slug, status, &reviewers, evidence.thread)
                }
                REVIEW if wanted => {
                    let rounds = review_rounds(evidence.thread);
                    let gate = review(slug, status, &reviewers, &rounds);
                    review_at(gate, &rounds, evidence.branch, evidence.tip, status)
                }
                OWNER => match owner(evidence.cards) {
                    Some(held) => held,
                    None if wanted => read_at(name, GateState::Held, &|tip| {
                        read::owner_with(evidence.thread, tip, || (evidence.posted)(tip))
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

/// The review stamp at `tip` ([`review`]'s gate from its `rounds`; `None` with no tip, as the
/// task's `status` says, [`without_tip`]): met only while the last round's approve names the tip,
/// else missing, naming the commit it approved; a task done with no round landed without review,
/// which its assessment did not waive: missing.
pub fn review_at(
    gate: Gate,
    rounds: &[Round],
    branch: &str,
    tip: Option<&str>,
    status: &str,
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
                let (state, none) = without_tip(GateState::Missing, status);
                let none = match none {
                    NoTip::Unstarted => "it has no branch yet".to_string(),
                    NoTip::Open => format!("no branch {branch} is in this repository"),
                    NoTip::Closed => format!("no branch {branch} is in this repository{NOR}"),
                };
                return Gate {
                    state,
                    detail: format!("{}, but {none}", gate.detail),
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

/// The stamp `name` with no tip to read it at: the repository has no branch `branch`, a task in
/// `status` not started has none yet, and a closed task's thread names no commit its land merged
/// ([`without_tip`]).
fn no_tip(name: &str, unmet: GateState, branch: &str, status: &str) -> Gate {
    let (state, none) = without_tip(unmet, status);
    let detail = match none {
        NoTip::Unstarted => NO_BRANCH_YET.to_string(),
        NoTip::Open => format!("No branch {branch} in this repository"),
        NoTip::Closed => format!("No branch {branch} in this repository{NOR}"),
    };
    Gate {
        name: name.into(),
        state,
        why: (state == GateState::Held).then(owner_waits),
        detail,
        ..Gate::default()
    }
}

/// The statuses a task is in before its work starts: it has no branch yet, and none is missing.
const UNSTARTED: [&str; 4] = ["backlog", "concept", "design", "ready"];

/// A needed stamp's detail on a task not started, which has no branch to read it at yet.
pub const NO_BRANCH_YET: &str = "No branch yet";

/// What a closed task's detail adds to its missing branch.
const NOR: &str = ", nor a land on its thread naming the commit it merged";

/// Why a task has no tip to read a stamp at ([`without_tip`]).
enum NoTip {
    /// Not started: no branch yet.
    Unstarted,
    /// Started and open: its branch is not in this repository.
    Open,
    /// Done or cancelled: its branch is gone, and no land on its thread names what it merged.
    Closed,
}

/// What a needed stamp of a task in `status` with no tip to read it at is, and why there is none:
/// an open task's stands on nothing yet, `unmet`, one not started having no branch yet; a closed
/// task's stood on a branch that is gone, with no land on its thread naming the commit it merged,
/// so where it stood cannot be read: unknown, never missing.
fn without_tip(unmet: GateState, status: &str) -> (GateState, NoTip) {
    if matches!(status, "done" | "cancelled") {
        (GateState::Unknown, NoTip::Closed)
    } else if UNSTARTED.contains(&status) {
        (unmet, NoTip::Unstarted)
    } else {
        (unmet, NoTip::Open)
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

/// The stamp `name` of the task `slug` a gate decision waived at the tip (`waiver`): waived, its
/// reason, author and tip the decision's, its line on the `thread` the link.
fn decided(name: &str, slug: &str, waiver: &Decision, thread: &[Message]) -> Gate {
    let said = waiver.line();
    let line = (thread.iter()).rfind(|line| line.kind == "system" && line.text == said);
    Gate {
        name: name.into(),
        state: GateState::Waived,
        by: Some(waiver.by.clone()),
        at: Some(waiver.at),
        commit: waiver.tip.clone(),
        why: Some(waiver.why.clone()),
        detail: format!(
            "{DECIDED} {}",
            short(waiver.tip.as_deref().unwrap_or_default())
        ),
        link: line.map(|line| format!("/task/{slug}#message-{}", line.id)),
        past: Vec::new(),
    }
}

/// How a stamp a gate decision waived starts its detail, the tip after.
const DECIDED: &str = "Waived by a gate decision at";

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

/// The task and every task under it, whose runs are its own: the board's reader, which the land
/// reads the `tests` stamp by too.
pub use symphony_board::stamps::family;

/// The owner's approve the task page offers on `record`, its branch `branch` in the checkout
/// `root`: while the task is top-level, has a tip, needs its `owner` stamp there and waits on it
/// ([`awaits_owner`]). Its payload is the whole tip, which [`OWNER_APPROVE`]'s action posts as the
/// owner's line ([`approve_line`]).
pub fn owner_offer(store: &Store, record: &Task, root: &Path, branch: &str) -> Option<TaskOffer> {
    if record.parent.is_some() || record.status == "cancelled" {
        return None;
    }
    let tip = super::tip(root, branch)?;
    let catalogue = Catalogue::at(root).ok()?;
    let needed = read::needed(&catalogue, record.needs().as_ref(), Some(&tip));
    if !needed.iter().any(|need| need == OWNER) {
        return None;
    }
    let thread = store.thread(&record.slug).ok()?;
    awaits_owner(&record.status, &thread, &tip, root).then(|| TaskOffer {
        action: OWNER_APPROVE.into(),
        label: format!("Approve {}", short(&tip)),
        branch: branch.into(),
        payload: Some(tip),
    })
}

/// Whether a task of `status`, its `thread` this, waits on the owner's approve of `tip`: its
/// `owner` stamp does not stand there (`read::owner_at`, the cards its branch posted read in the
/// checkout `root`), and the task is open, or done by an approve no landing has closed
/// (`symphony_board::approved`), where the land leaves a task that lacks only the owner's stamp.
/// A task cancelled, or done by a landing or a covered close, waits on nothing.
pub fn awaits_owner(status: &str, thread: &[Message], tip: &str, root: &Path) -> bool {
    let standing = match status {
        "done" => symphony_board::approved(thread).is_some(),
        "cancelled" => false,
        _ => true,
    };
    standing && !read::owner_at(thread, tip, root).met
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
