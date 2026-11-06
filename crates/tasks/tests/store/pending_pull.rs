//! A PULL waiting out an outage while the digest changes beneath it, by one rule over what the
//! orchestrator hears now, what the PULLs it acknowledged told it and what the PULL waiting tells:
//! taken back once nothing it would say is new beyond what was acknowledged; replaced, under a new
//! key with a retry budget of its own, for an entry new beyond the PULL waiting too; otherwise
//! said under its own key and attempts, as the digest is at delivery. Whichever, what older
//! dispatchers read records the digest then, and what the orchestrator was told changes only when
//! a PULL is acknowledged, whichever generation records it; an older acknowledgement this one cannot
//! attribute counts every entry recorded as told, so nothing it may have told is said again.

use super::callbacks::Listener;
use super::hand_over_outage::{Outage, pull};
use super::mixed_dispatchers::{older_fields, older_record};
use super::orchestrators::heard;
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeSet;
use symphony_board::NewTask;

/// A digest entry: its list, and the slug or handle it names.
type Entry = (String, String);

/// The PULL waiting for the former orchestrator.
#[derive(Debug, Deserialize)]
struct Waiting {
    key: String,
    text: String,
    attempts: u32,
}

fn waiting_pull(outage: &Outage) -> Option<Waiting> {
    let found = outage.fixture.store.client().query(
        "RETURN SELECT key, text, attempts FROM callback_event WHERE handle = $handle AND delivered != true AND string::starts_with(key, 'pull-');",
        json!({"handle":outage.former}),
    );
    let mut waiting: Vec<Waiting> = serde_json::from_value(found.unwrap().remove(0)).unwrap();
    assert!(waiting.len() < 2, "{waiting:?}");
    waiting.pop()
}

/// The former orchestrator's digest now, as its PULL says it.
fn digest_now(outage: &Outage) -> String {
    let Outage {
        fixture, former, ..
    } = outage;
    let board = fixture.store.board().unwrap();
    let digest = symphony_board::cli::pull::digest(&fixture.store, &[], None, fixture.now(), &[]);
    let digest = digest.unwrap().only_for(&board, former);
    let lines = (digest.event_lines().into_iter())
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "));
    lines.collect::<Vec<_>>().join("\n")
}

/// The lines of the PULL among those `told`.
fn pull_lines(told: &[String]) -> String {
    let digest = |line: &&String| line.starts_with("PULL ") || line.starts_with("ready_unstaffed ");
    let lines: Vec<&str> = told.iter().filter(digest).map(String::as_str).collect();
    lines.join("\n")
}

/// Once the former orchestrator heard what waited at `back`, what older dispatchers read records
/// the digest as it is, and at the next periodic look neither generation says a PULL again.
fn said_once(outage: &Outage, back: &Listener) {
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
    let outage = Outage::new();
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
    assert_eq!(told, [free_new, &pull(1), free_row]);
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
        "PULL executing=1 ready_unstaffed=3 in_progress_orphaned=0 review_unreviewed=0 landed_open=0 idle=0",
        free_row,
        "ready_unstaffed p2 scratch/theirs — Jump departure",
        "ready_unstaffed p2 scratch/theirs/new — Jump departure",
    ];
    assert_eq!(told, theirs);
}

/// The attempts a PULL waiting made to reach its orchestrator.
#[derive(Clone, Copy, Debug)]
enum Attempts {
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
fn waited(attempts: Attempts) -> Outage {
    let outage = Outage::new();
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
struct HandOver<'a> {
    slug: &'a str,
    from: &'a str,
    to: &'a str,
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

/// An older dispatcher delivers what waits for the former orchestrator: it records the entries of
/// the PULL waiting as told and its queue as ended, and nothing of what that PULL left untold.
fn older_acknowledges(outage: &Outage) {
    let pull = waiting_pull(outage).expect("a PULL waits");
    outage.fixture.store.client().query(
        "LET $pull = type::record('callback_event', [$handle, $key]);
         UPDATE type::record('callback_state', $handle) SET pull_entries = $pull.pull_entries, pull_queued = NONE RETURN NONE;
         UPDATE callback_event SET delivered = true WHERE handle = $handle RETURN NONE;",
        json!({"handle":outage.former, "key":pull.key}),
    ).unwrap();
}

/// The callback record of the former orchestrator an older dispatcher reads.
#[derive(Deserialize)]
struct Recorded {
    pull_entries: Option<BTreeSet<Entry>>,
    pull_queued: Option<BTreeSet<Entry>>,
}

/// One dispatch of a dispatcher built before one orchestrator per task, as it writes the former
/// orchestrator's record, under a lease revision of its own: its observation, and once it
/// `computes` the digest, every entry that left it pruned from what is recorded, queued and
/// waiting; for an entry new to both, a PULL of its own of the whole digest takes the waiting
/// one's place.
fn older_dispatch(outage: &Outage, computes: bool) {
    let Outage {
        fixture, former, ..
    } = outage;
    let client = fixture.store.client();
    let found = client.query(
        "RETURN SELECT pull_entries, pull_queued FROM ONLY type::record('callback_state', $handle);",
        json!({"handle":former}),
    );
    let before: Recorded = serde_json::from_value(found.unwrap().remove(0)).unwrap();
    let whole = symphony_board::cli::pull::digest(&fixture.store, &[], None, fixture.now(), &[]);
    let whole = whole.unwrap();
    let entries = whole.entries();
    let recorded = &before.pull_entries.unwrap_or_default() & &entries;
    let pruned = before.pull_queued.as_ref().map(|queued| queued & &entries);
    let new = computes
        && !entries.is_subset(&recorded)
        && pruned
            .as_ref()
            .is_none_or(|queued| !entries.is_subset(queued));
    let queued = match (new, computes) {
        (true, _) => Some(entries.clone()),
        (false, true) => pruned,
        (false, false) => before.pull_queued,
    };
    let text: Vec<String> = whole.event_lines().iter().map(|l| l.to_string()).collect();
    client.query(
        "LET $s = (SELECT revision FROM ONLY type::record('callback_state', $handle));
         LET $revision = math::max([type::record('callback_lease', $handle).revision ?? 0, $s.revision ?? 0]) + 1;
         UPSERT type::record('callback_lease', $handle) SET revision = $revision RETURN NONE;
         IF $computes { UPDATE callback_event SET pull_entries = pull_entries.filter(|$entry| $entry IN $entries)
            WHERE handle = $handle AND delivered != true AND pull_entries != NONE RETURN NONE; };
         IF $new {
            DELETE callback_event WHERE handle = $handle AND delivered != true AND string::starts_with(key, 'pull-') RETURN NONE;
            LET $key = string::concat('pull-', <string> $revision);
            LET $counted = (UPSERT ONLY type::record('counter', ['callback-event', $handle]) SET last = (last ?? 0) + 1);
            CREATE type::record('callback_event', [$handle, $key]) CONTENT { key: $key, handle: $handle, text: $text, attempts: 0,
                sequence: $counted.last, pull_entries: $entries } RETURN NONE;
         };
         UPDATE type::record('callback_state', $handle) SET revision = $revision, observed_registration = registration_revision ?? 0,
            pull = IF $computes { $now } ELSE { pull }, pull_entries = IF $computes { $recorded } ELSE { pull_entries }, pull_queued = $queued RETURN NONE;",
        json!({"handle":former, "computes":computes, "new":new, "entries":entries, "recorded":recorded,
            "queued":queued, "text":text.join("\n"), "now":fixture.now()}),
    ).unwrap();
}

#[test]
fn an_older_dispatcher_acknowledging_the_pull_this_one_queued_records_what_it_told() {
    let outage = waited(Attempts::One);
    let Outage {
        fixture,
        elsewhere,
        former,
        successor,
        ..
    } = &outage;
    // The successor's task, withheld from the former until now, is its own: a new PULL counts it.
    let before = waiting_pull(&outage).expect("a PULL waits");
    (elsewhere.hand_over("scratch/theirs", successor, former)).unwrap();
    fixture.store.dispatch_callbacks_snapshot().unwrap();
    let pull = waiting_pull(&outage).expect("a PULL waits");
    assert_ne!(pull.key, before.key);
    assert!(pull.text.contains(" ready_unstaffed=10 "), "{}", pull.text);
    older_acknowledges(&outage);
    let back = outage.back();
    said_once(&outage, &back);
}

/// What an older dispatcher writes before it delivers the PULL waiting for the former orchestrator.
#[derive(Clone, Copy, Debug)]
enum OlderWrites {
    Nothing,
    /// Its observation, as when the former registers its callback again: no PULL of its own.
    Observation,
    /// The successor's task leaves the digest and returns: the PULL waiting forgets it, and for
    /// its return the older dispatcher queues a PULL of its own in its place, which names it.
    Return,
}

/// An older acknowledgement, and whether the PULL the successor's task makes when it becomes the
/// former orchestrator's counts it.
#[derive(Debug)]
struct OlderAcknowledgement {
    writes: OlderWrites,
    counted: bool,
}

#[test]
fn work_an_older_acknowledgement_may_have_told_is_never_said_again_by_a_pull() {
    let cases = [
        // Of the PULL this generation queued: it withheld the successor's task, untold since.
        OlderAcknowledgement {
            writes: OlderWrites::Nothing,
            counted: true,
        },
        // Of a PULL the older dispatcher may have queued since: its record says which no more.
        OlderAcknowledgement {
            writes: OlderWrites::Observation,
            counted: false,
        },
        // Of its own, which told the successor's task.
        OlderAcknowledgement {
            writes: OlderWrites::Return,
            counted: false,
        },
    ];
    for case in cases {
        let outage = waited(Attempts::One);
        let Outage {
            fixture,
            elsewhere,
            former,
            successor,
            sender,
        } = &outage;
        // Nine of the ten ready tasks are the former's; the PULL waiting counts those.
        let (mine, whole) = ("PULL executing=0 ready_unstaffed=9 ", "ready_unstaffed=10 ");
        let ours = waiting_pull(&outage).expect("a PULL waits");
        assert!(ours.text.starts_with(mine), "{case:?}: {ours:?}");
        match case.writes {
            OlderWrites::Nothing => {}
            OlderWrites::Observation => older_dispatch(&outage, false),
            OlderWrites::Return => {
                (elsewhere.move_to("scratch/theirs", "backlog", sender)).unwrap();
                older_dispatch(&outage, true);
                (elsewhere.move_to("scratch/theirs", "ready", sender)).unwrap();
                older_dispatch(&outage, true);
                let own = waiting_pull(&outage).expect("its own PULL waits");
                assert_ne!(own.key, ours.key, "{case:?}");
                assert!(own.text.contains(whole), "{case:?}: {own:?}");
            }
        }
        older_acknowledges(&outage);
        (elsewhere.hand_over("scratch/theirs", successor, former)).unwrap();
        let back = outage.back();
        let told = heard(fixture, sender, &[(&back, former)]).remove(0);
        let moved = format!("ROLE scratch/theirs {former} orchestrator, ready (from {successor})");
        assert_eq!(told[0], moved, "{case:?}: {told:?}");
        let pull = pull_lines(&told);
        let counted = pull.starts_with(&format!("PULL executing=0 {whole}"));
        assert_eq!(counted, case.counted, "{case:?}: {told:?}");
        assert_eq!(pull.is_empty(), !case.counted, "{case:?}: {told:?}");
        said_once(&outage, &back);
    }
}

#[test]
fn an_inbox_receipt_of_a_pull_keeps_what_it_withheld_untold() {
    let outage = waited(Attempts::One);
    let Outage {
        fixture,
        elsewhere,
        former,
        successor,
        sender,
    } = &outage;
    // The former reads what waits from its inbox: the PULL withheld the successor's task.
    let inbox = fixture.store.callback_inbox(former).unwrap();
    let keys: Vec<String> = inbox.into_iter().map(|(key, _)| key).collect();
    fixture.store.ack_callbacks(former, &keys).unwrap();
    // That task becomes the former's: it was never told it, so a PULL counts it.
    (elsewhere.hand_over("scratch/theirs", successor, former)).unwrap();
    let back = outage.back();
    let told = heard(fixture, sender, &[(&back, former)]).remove(0);
    let pull = pull_lines(&told);
    let head = "PULL executing=0 ready_unstaffed=10 ";
    assert!(pull.starts_with(head), "{told:?}");
    said_once(&outage, &back);
}
