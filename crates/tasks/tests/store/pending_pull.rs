//! A PULL waiting out an outage while the digest changes beneath it, by one rule over what the
//! orchestrator hears now, what the PULLs it acknowledged told it and what the PULL waiting tells:
//! taken back once nothing it would say is new beyond what was acknowledged; replaced, under a new
//! key with a retry budget of its own, for an entry new beyond the PULL waiting too; otherwise
//! said under its own key and attempts, as the digest is at delivery. Whichever, what older
//! dispatchers read records the digest then, and what the orchestrator was told changes only when
//! a PULL is acknowledged, whichever generation records it (`pull_acknowledgements`).

use super::callbacks::Listener;
use super::hand_over_outage::{FREE, Outage};
use super::mixed_dispatchers::{older_fields, older_record};
use super::orchestrators::heard;
use serde::Deserialize;
use serde_json::json;
use symphony_board::NewTask;
use symphony_board::cli::pull::Said;

/// The PULL waiting for the former orchestrator.
#[derive(Debug, Deserialize)]
pub(super) struct Waiting {
    pub(super) key: String,
    pub(super) text: String,
    attempts: u32,
}

pub(super) fn waiting_pull(outage: &Outage) -> Option<Waiting> {
    let events = outage.fixture.store.raw().table("callback_event");
    let found = (events.matching(json!({"handle": outage.former})))
        .fields(&["key", "text", "attempts", "delivered"])
        .unwrap();
    let mut waiting: Vec<Waiting> = (found.as_array().unwrap().iter())
        .filter(|event| event["delivered"] != json!(true))
        .filter(|event| event["key"].as_str().unwrap().starts_with("pull-"))
        .map(|event| serde_json::from_value(event.clone()).unwrap())
        .collect();
    assert!(waiting.len() < 2, "{waiting:?}");
    waiting.pop()
}

/// What the last PULL `handle` acknowledged said, as its callback state records it.
pub(super) fn said(fixture: &super::common::Fixture, handle: &str) -> Said {
    let said = fixture.store.raw().row("callback_state", handle);
    let said = said.field("pull_said").unwrap();
    serde_json::from_value(said).expect("a PULL it acknowledged said this")
}

/// The former orchestrator's digest now, as its PULL says it: what changed since the PULL it
/// heard before the outage, which told it every entry it named that is still there.
fn digest_now(outage: &Outage) -> String {
    let Outage {
        fixture, former, ..
    } = outage;
    let board = fixture.store.board().unwrap();
    let digest = symphony_board::cli::pull::digest(&fixture.store, &[], None, fixture.now(), &[]);
    let digest = digest.unwrap().only_for(&board, former);
    let said = said(fixture, former);
    let told = &said.entries & &digest.entries();
    let lines = (digest.changes_since(&said, &told).into_iter())
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "));
    lines.collect::<Vec<_>>().join("\n")
}

/// The lines of the PULL among those `told`.
pub(super) fn pull_lines(told: &[String]) -> String {
    let digest = |line: &&String| {
        ["PULL ", "ready_unstaffed ", "resolved "]
            .iter()
            .any(|start| line.starts_with(start))
    };
    let lines: Vec<&str> = told.iter().filter(digest).map(String::as_str).collect();
    lines.join("\n")
}

/// Once the former orchestrator heard what waited at `back`, what older dispatchers read records
/// the digest as it is, and at the next periodic look neither generation says a PULL again.
pub(super) fn said_once(outage: &Outage, back: &Listener) {
    let Outage {
        fixture,
        former,
        sender,
        ..
    } = outage;
    assert_eq!(older_fields(fixture, former), older_record(fixture, former));
    outage.pass(601.0, &[]);
    let again = heard(fixture, sender, &[(back, former)]);
    assert_eq!(again, [Vec::<String>::new()]);
}

#[test]
fn a_pull_reading_the_same_at_delivery_records_the_work_withheld_from_it_meanwhile() {
    let outage = Outage::on();
    let Outage {
        fixture,
        elsewhere,
        former,
        successor,
        sender,
    } = &outage;
    fixture.store.claim("scratch/one", sender).unwrap();
    fixture.task("scratch/theirs");
    let theirs = fixture
        .store
        .take_role("scratch/theirs", successor, "orchestrator");
    theirs.unwrap();
    outage.cut();
    let free = NewTask::new("scratch/free", "Jump departure", "owner");
    elsewhere.create(&free).unwrap();
    fixture.store.dispatch_callbacks_snapshot().unwrap();
    let before = waiting_pull(&outage).expect("a PULL waits");
    // The successor's task gains ready work, which the former's PULL never names.
    let new = NewTask {
        parent: Some("scratch/theirs".into()),
        ..NewTask::new("scratch/theirs/new", "Jump departure", "owner")
    };
    elsewhere.create(&new).unwrap();
    fixture.store.dispatch_callbacks_snapshot().unwrap();
    let after = waiting_pull(&outage).expect("the PULL waits still");
    assert_eq!((&after.key, &after.text), (&before.key, &before.text));
    let back = outage.back();
    let free_row = "ready_unstaffed p2 scratch/free — Jump departure";
    let told = back.drain(fixture, sender, former);
    let free_new = "NEW scratch/free ready — Jump departure";
    assert_eq!(told, [free_new, FREE, free_row]);
    let recorded = older_fields(fixture, former);
    let entries = recorded["pull_entries"].as_array().unwrap();
    let new = json!(["ready_unstaffed", "scratch/theirs/new"]);
    assert!(entries.contains(&new), "{recorded}");
    said_once(&outage, &back);
    // Withheld from it, that work is said once its task is the former's.
    (elsewhere.hand_over("scratch/theirs", successor, former)).unwrap();
    outage.pass(601.0, &[]);
    let told = heard(fixture, sender, &[(&back, former)]).remove(0);
    let moved = format!("ROLE scratch/theirs {former} orchestrator, ready (from {successor})");
    let theirs = [
        moved.as_str(),
        "NEW scratch/theirs/new ready — Jump departure",
        // What changed since the PULL of scratch/free: that row is not said again.
        "PULL ready_unstaffed=3 (was 1)",
        "ready_unstaffed p2 scratch/theirs — Jump departure",
        "ready_unstaffed p2 scratch/theirs/new — Jump departure",
    ];
    assert_eq!(told, theirs);
}

/// The attempts a PULL waiting made to reach its orchestrator.
#[derive(Clone, Copy, Debug)]
pub(super) enum Attempts {
    One,
    /// Every attempt it has.
    All,
}

impl Attempts {
    fn made(self) -> u32 {
        match self {
            Attempts::One => 1,
            Attempts::All => 3,
        }
    }
}

/// The former orchestrator heard a PULL of `scratch/one` and `scratch/mine`, its own, while
/// `scratch/theirs`, the successor's, was withheld from it. Unreachable since, it has a PULL
/// waiting of seven new ready rows, `scratch/row-00` to `-06`, which shows four of them and counts
/// the rest, after the `attempts` it made.
pub(super) fn waited(attempts: Attempts) -> Outage {
    let outage = Outage::on();
    let Outage {
        fixture,
        elsewhere,
        former,
        successor,
        ..
    } = &outage;
    for (slug, orchestrator) in [("scratch/mine", former), ("scratch/theirs", successor)] {
        fixture.task(slug);
        let held = fixture.store.take_role(slug, orchestrator, "orchestrator");
        held.unwrap();
    }
    outage.cut();
    for row in 0..7 {
        let row = NewTask::new(&format!("scratch/row-{row:02}"), "A row", "owner");
        elsewhere.create(&row).unwrap();
    }
    fixture.store.dispatch_callbacks_snapshot().unwrap();
    for _retry in 1..attempts.made() {
        fixture.advance(6.0);
        fixture.store.dispatch_callbacks_snapshot().unwrap();
    }
    outage
}

/// Whose work changes while the PULL waits.
#[derive(Clone, Copy, Debug)]
enum Scope {
    Same,
    /// `scratch/theirs` is handed to the former orchestrator.
    Gained,
    /// `scratch/mine` is handed to the successor.
    Lost,
}

/// Which ready work changes while the PULL waits.
#[derive(Clone, Copy, Debug)]
enum Change {
    Nothing,
    /// `scratch/added` comes up, and is shown.
    Added,
    /// `scratch/row-00`, shown, goes back to the backlog.
    Departed,
    /// Every row does.
    AllDeparted,
    /// `scratch/row-06`, counted but not shown, does, as `scratch/row-07` comes up in its place.
    Replaced,
}

/// What becomes of the PULL waiting.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Outcome {
    /// Taken back.
    Taken,
    /// Kept: its key and attempts.
    Kept,
    /// Replaced, under a new key, sent once at once.
    New,
}

/// One change to the digest while the PULL waits, and what it comes to.
#[derive(Clone, Copy, Debug)]
struct Case {
    change: Change,
    outcome: Outcome,
    /// Whether the PULL reads the same as before.
    same_text: bool,
}

/// The orchestrator role on `slug` passing from `from` to `to`.
struct HandOver<'outage> {
    slug: &'outage str,
    from: &'outage str,
    to: &'outage str,
}

/// Each case once with one attempt made and once with every attempt made: the PULL that waits
/// after it is the digest as it is at delivery, and the one the former orchestrator hears when it
/// is back; what older dispatchers read then records the digest, and no PULL follows.
fn judge(scope: Scope, cases: &[Case]) {
    let runs = (cases.iter()).flat_map(|&case| [Attempts::One, Attempts::All].map(|a| (case, a)));
    for (case, attempts) in runs {
        let outage = waited(attempts);
        let Outage {
            fixture,
            elsewhere,
            former,
            successor,
            sender,
        } = &outage;
        let before = waiting_pull(&outage).expect("a PULL waits");
        assert_eq!(before.attempts, attempts.made());
        let (mine, theirs) = ("scratch/mine", "scratch/theirs");
        let handed = match scope {
            Scope::Same => None,
            Scope::Gained => Some(HandOver {
                slug: theirs,
                from: successor,
                to: former,
            }),
            Scope::Lost => Some(HandOver {
                slug: mine,
                from: former,
                to: successor,
            }),
        };
        if let Some(HandOver { slug, from, to }) = handed {
            elsewhere.hand_over(slug, from, to).unwrap();
        }
        let backlog = |row: u8| {
            let slug = format!("scratch/row-{row:02}");
            elsewhere.move_to(&slug, "backlog", sender).unwrap();
        };
        let ready = |slug: &str| {
            let task = NewTask::new(slug, "A row", "owner");
            elsewhere.create(&task).unwrap();
        };
        match case.change {
            Change::Nothing => {}
            Change::Added => ready("scratch/added"),
            Change::Departed => backlog(0),
            Change::AllDeparted => {
                for row in 0..7 {
                    backlog(row);
                }
            }
            Change::Replaced => {
                backlog(6);
                ready("scratch/row-07");
            }
        }
        // Two dispatches, and nothing acknowledged: the second judges what the first left.
        for _ in 0..2 {
            fixture.store.dispatch_callbacks_snapshot().unwrap();
        }
        let after = waiting_pull(&outage);
        let what = format!("{scope:?} {case:?} {attempts:?}: {before:?} to {after:?}");
        match (case.outcome, &after) {
            (Outcome::Taken, None) => {}
            (Outcome::Kept, Some(after)) => {
                let kept = (&after.key, after.attempts);
                assert_eq!(kept, (&before.key, before.attempts), "{what}");
            }
            (Outcome::New, Some(after)) => {
                assert_ne!(after.key, before.key, "{what}");
                assert_eq!(after.attempts, 1, "{what}");
            }
            _ => panic!("{what}"),
        }
        let text = after.map(|after| after.text);
        if let Some(text) = &text {
            assert_eq!(*text == before.text, case.same_text, "{what}");
            assert_eq!(*text, digest_now(&outage), "{what}");
        }
        let back = outage.back();
        let told = back.drain(fixture, sender, former);
        assert_eq!(pull_lines(&told), text.unwrap_or_default(), "{what}");
        said_once(&outage, &back);
    }
}

#[test]
fn a_pull_waiting_while_the_orchestrator_keeps_its_work_is_judged_by_what_is_new_beyond_it() {
    judge(
        Scope::Same,
        &[
            Case {
                change: Change::Nothing,
                outcome: Outcome::Kept,
                same_text: true,
            },
            Case {
                change: Change::Added,
                outcome: Outcome::New,
                same_text: false,
            },
            Case {
                change: Change::Departed,
                outcome: Outcome::Kept,
                same_text: false,
            },
            Case {
                change: Change::AllDeparted,
                outcome: Outcome::Taken,
                same_text: false,
            },
            Case {
                change: Change::Replaced,
                outcome: Outcome::New,
                same_text: true,
            },
        ],
    );
}

#[test]
fn a_pull_waiting_while_the_orchestrator_gains_work_withheld_from_it_names_it_anew() {
    let new = |change| Case {
        change,
        outcome: Outcome::New,
        same_text: false,
    };
    let changes = [
        Change::Nothing,
        Change::Added,
        Change::Departed,
        Change::AllDeparted,
        Change::Replaced,
    ];
    judge(Scope::Gained, &changes.map(new));
}

#[test]
fn a_pull_waiting_while_the_orchestrator_loses_work_it_was_told_of_keeps_its_budget() {
    judge(
        Scope::Lost,
        &[
            Case {
                change: Change::Nothing,
                outcome: Outcome::Kept,
                same_text: false,
            },
            Case {
                change: Change::Added,
                outcome: Outcome::New,
                same_text: false,
            },
            Case {
                change: Change::Departed,
                outcome: Outcome::Kept,
                same_text: false,
            },
            Case {
                change: Change::AllDeparted,
                outcome: Outcome::Taken,
                same_text: false,
            },
            Case {
                change: Change::Replaced,
                outcome: Outcome::New,
                same_text: false,
            },
        ],
    );
}
