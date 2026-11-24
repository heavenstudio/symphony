//! What an orchestrator was told, whichever generation acknowledged its PULL: this one's
//! acknowledgement records what the PULL withheld, an older one's only the entries it records,
//! read over what this one withheld from its first PULL on and the PULL it left awaiting. Work it
//! hears and was not told stays out of that record until a PULL naming it is acknowledged, so
//! whatever an older dispatcher writes or queues after, it is never counted again; nor is work
//! acknowledged from the inbox, which takes no lease, while a dispatch builds the digest again,
//! even when a presence sweep prunes what that receipt delivered.
//! Only an older dispatcher's own PULL of the same entries reads as the one awaited, so work
//! withheld from the orchestrator is counted once more after one when a hand-over makes it its
//! own, never lost, and nothing repeats after.

use super::common::sweep_presence;
use super::hand_over_outage::Outage;
use super::mixed_dispatchers::{older_fields, older_record};
use super::orchestrators::heard;
use super::pending_pull::{Attempts, pull_lines, said_once, waited, waiting_pull};
use super::quiet_callbacks::listen;
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use symphony_board::Database;
use symphony_board::{NewTask, Raw};

/// A digest entry: its list, and the slug or handle it names.
type Entry = (String, String);

/// An older dispatcher delivers what waits for the former orchestrator: it records the entries of
/// the PULL waiting as told and its queue as ended, and nothing of what that PULL left untold.
fn older_acknowledges(outage: &Outage) {
    let pull = waiting_pull(outage).expect("a PULL waits");
    let raw = outage.fixture.store.raw();
    let entries = raw
        .row("callback_event", json!([outage.former, pull.key]))
        .field("pull_entries")
        .unwrap();
    raw.row("callback_state", outage.former.as_str())
        .set(json!({"pull_entries":entries,"pull_queued":null}))
        .unwrap();
    raw.table("callback_event")
        .matching(json!({"handle":outage.former}))
        .set(json!({"delivered":true}))
        .unwrap();
}

/// The callback record of the former orchestrator an older dispatcher reads.
#[derive(Deserialize)]
struct Recorded {
    pull_entries: Option<BTreeSet<Entry>>,
    pull_queued: Option<BTreeSet<Entry>>,
}

/// What one dispatch of an older dispatcher does with the digest.
#[derive(Clone, Copy, PartialEq)]
enum Older {
    /// Writes its observation only, as a post or a registration has it do.
    Observes,
    /// Builds the digest again.
    Computes,
}

/// One dispatch of a dispatcher built before one orchestrator per task, as it writes the former
/// orchestrator's record, under a lease revision of its own: its observation, and once it
/// computes the digest, every entry that left it pruned from what is recorded, queued and
/// waiting; for an entry new to both, a PULL of its own of the whole digest takes the waiting
/// one's place.
fn older_dispatch(outage: &Outage, older: Older) {
    let Outage {
        fixture, former, ..
    } = outage;
    let found = (fixture.store.raw().row("callback_state", former.as_str()))
        .fields(&["pull_entries", "pull_queued"]);
    let before: Recorded = serde_json::from_value(found.unwrap()).unwrap();
    let whole = symphony_board::cli::pull::digest(&fixture.store, &[], None, fixture.now(), &[]);
    let whole = whole.unwrap();
    let entries = whole.entries();
    let computes = older == Older::Computes;
    let recorded = &before.pull_entries.unwrap_or_default() & &entries;
    let pruned = before.pull_queued.as_ref().map(|queued| queued & &entries);
    let new = computes
        && !entries.is_subset(&recorded)
        && (pruned.as_ref()).is_none_or(|queued| !entries.is_subset(queued));
    let queued = match (new, computes) {
        (true, _) => Some(entries.clone()),
        (false, true) => pruned,
        (false, false) => before.pull_queued,
    };
    let text: Vec<String> = whole.event_lines().iter().map(|l| l.to_string()).collect();
    let raw = fixture.store.raw();
    let state = raw.row("callback_state", former.as_str());
    let lease = raw.row("callback_lease", former.as_str());
    let revision = state
        .field("revision")
        .unwrap()
        .as_u64()
        .unwrap_or(0)
        .max(lease.field("revision").unwrap().as_u64().unwrap_or(0))
        + 1;
    lease.set(json!({"revision":revision})).unwrap();
    let rows = raw
        .table("callback_event")
        .matching(json!({"handle":former}))
        .get()
        .unwrap();
    for row in rows
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["delivered"] != true)
    {
        let at = raw.row("callback_event", json!([former, row["key"]]));
        if new && row["key"].as_str().unwrap().starts_with("pull-") {
            at.delete().unwrap();
        } else if computes && row.get("pull_entries").is_some() {
            let kept: BTreeSet<Entry> =
                serde_json::from_value(row["pull_entries"].clone()).unwrap();
            at.set(json!({"pull_entries": &kept & &entries})).unwrap();
        }
    }
    if new {
        let counter = raw.row("counter", json!(["callback-event", former]));
        let sequence = counter.field("last").unwrap().as_u64().unwrap_or(0) + 1;
        counter.set(json!({"last":sequence})).unwrap();
        let key = format!("pull-{revision}");
        raw.row("callback_event", json!([former,key])).put(json!({"key":key,"handle":former,"text":text.join("\n"),"attempts":0,"sequence":sequence,"pull_entries":entries})).unwrap();
    }
    let mut fields = json!({"revision":revision,"observed_registration":state.field("registration_revision").unwrap().as_u64().unwrap_or(0),"pull_queued":queued});
    if computes {
        fields["pull"] = json!(fixture.now());
        fields["pull_entries"] = json!(recorded);
    }
    state.set(fields).unwrap();
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
    /// Nothing: the PULL it delivers is the one this generation queued, which withheld the
    /// successor's task.
    Nothing,
    /// Its observation, as when the former registers its callback again: no PULL of its own.
    Observation,
    /// The successor's task leaves the digest and returns: the PULL waiting forgets it, and for
    /// its return the older dispatcher queues a PULL of its own in its place, which counts it.
    Return,
}

#[test]
fn work_handed_to_the_orchestrator_is_counted_once_whatever_an_older_dispatcher_acknowledged() {
    let cases = [
        OlderWrites::Nothing,
        OlderWrites::Observation,
        OlderWrites::Return,
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
        let (mine, whole) = ("PULL ready_unstaffed=9 (was 2)\n", "ready_unstaffed=10 ");
        let ours = waiting_pull(&outage).expect("a PULL waits");
        assert!(ours.text.starts_with(mine), "{case:?}: {ours:?}");
        match case {
            OlderWrites::Nothing => {}
            OlderWrites::Observation => older_dispatch(&outage, Older::Observes),
            OlderWrites::Return => {
                (elsewhere.move_to("scratch/theirs", "backlog", sender)).unwrap();
                older_dispatch(&outage, Older::Computes);
                (elsewhere.move_to("scratch/theirs", "ready", sender)).unwrap();
                older_dispatch(&outage, Older::Computes);
                let own = waiting_pull(&outage).expect("its own PULL waits");
                assert_ne!(own.key, ours.key, "{case:?}");
                assert!(own.text.contains(whole), "{case:?}: {own:?}");
            }
        }
        older_acknowledges(&outage);
        // Once the task is the former's, one PULL counts it: after the older one's own PULL, once
        // more. Then neither generation says a PULL again.
        (elsewhere.hand_over("scratch/theirs", successor, former)).unwrap();
        let back = outage.back();
        let told = heard(fixture, sender, &[(&back, former)]).remove(0);
        let moved = format!("ROLE scratch/theirs {former} orchestrator, ready (from {successor})");
        assert_eq!(told[0], moved, "{case:?}: {told:?}");
        let heads: Vec<&String> = told.iter().filter(|l| l.starts_with("PULL ")).collect();
        assert_eq!(
            heads,
            [&format!(
                "PULL executing=0 {whole}in_progress_orphaned=0 review_unreviewed=0 landed_open=0 idle=0"
            )],
            "{case:?}"
        );
        said_once(&outage, &back);
    }
}

/// The former orchestrator's task is executing, and the successor's has a child awaiting review;
/// neither of those is the former's.
fn theirs_in_review(outage: &Outage) {
    let Outage {
        fixture,
        successor,
        sender,
        ..
    } = outage;
    fixture.store.claim("scratch/one", sender).unwrap();
    fixture.task("scratch/theirs");
    let theirs = fixture
        .store
        .take_role("scratch/theirs", successor, "orchestrator");
    theirs.unwrap();
    fixture.task_with(NewTask {
        parent: Some("scratch/theirs".into()),
        ..NewTask::new("scratch/theirs/review", "Needs review", "owner")
    });
    fixture
        .store
        .claim("scratch/theirs/review", sender)
        .unwrap();
    fixture.historical_review("scratch/theirs/review");
}

/// The head of the PULL the former orchestrator hears of the successor's task and its child: what
/// changed since the first PULL it acknowledged, its own task executing all along.
const CHANGED: &str = "PULL ready_unstaffed=1 (was 0) review_unreviewed=1 (was 0)";
/// That PULL's head when no PULL it acknowledged was recorded as said.
const WHOLE: &str = "PULL executing=1 ready_unstaffed=1 in_progress_orphaned=0 review_unreviewed=1 landed_open=0 idle=0";

/// What the former orchestrator hears once the successor's task is its own: the ROLE, and a PULL
/// (`head`) that counts the task and its child.
fn handed_over(outage: &Outage, head: &str) -> [String; 4] {
    let Outage {
        former, successor, ..
    } = outage;
    [
        format!("ROLE scratch/theirs {former} orchestrator, ready (from {successor})"),
        head.into(),
        "ready_unstaffed p2 scratch/theirs — Jump departure".into(),
        "review_unreviewed p2 scratch/theirs/review — Needs review".into(),
    ]
}

#[test]
fn an_older_observation_never_hides_work_withheld_from_the_orchestrator_until_it_is_its_own() {
    let outage = Outage::on();
    let Outage {
        fixture,
        elsewhere,
        former,
        successor,
        sender,
    } = &outage;
    theirs_in_review(&outage);
    outage.cut();
    let waiting = waiting_pull(&outage);
    assert!(waiting.is_none(), "{waiting:?}");
    // A post has an older dispatcher write its observation: it queues no PULL, and none is
    // acknowledged.
    (elsewhere.say("scratch/one", sender, "An ordinary thread note", None)).unwrap();
    older_dispatch(&outage, Older::Observes);
    let waiting = waiting_pull(&outage);
    assert!(waiting.is_none(), "{waiting:?}");
    // The successor's task becomes the former's: the ROLE names it, and the PULL its child too.
    (elsewhere.hand_over("scratch/theirs", successor, former)).unwrap();
    let back = outage.back();
    let told = heard(fixture, sender, &[(&back, former)]).remove(0);
    assert_eq!(told, handed_over(&outage, CHANGED));
    said_once(&outage, &back);
}

/// Which generation acknowledges the former orchestrator's first PULL.
#[derive(Clone, Copy, Debug)]
enum FirstReceipt {
    /// This one, from the inbox: it records what the PULL withheld.
    ThisGeneration,
    /// An older one, which records every entry of the PULL as told.
    Older,
}

#[test]
fn work_the_first_pull_withheld_is_counted_after_a_hand_over_whoever_acknowledged_it() {
    for first in [FirstReceipt::ThisGeneration, FirstReceipt::Older] {
        let outage = Outage::on();
        let Outage {
            fixture,
            elsewhere,
            former,
            successor,
            sender,
        } = &outage;
        theirs_in_review(&outage);
        // Another dispatcher holds the former's lease while it registers, and its listener is
        // gone before the first PULL is sent: that PULL waits, and nothing is recorded as told.
        let lease = || fixture.store.raw().row("callback_lease", former.as_str());
        let held = json!({"until": fixture.now() + 60.0});
        lease().put(held).unwrap();
        drop(listen(fixture, former));
        lease().put(json!({"until": 0})).unwrap();
        fixture.store.dispatch_callbacks_snapshot().unwrap();
        let pull = waiting_pull(&outage).expect("the first PULL waits");
        assert!(!pull.text.contains("scratch/theirs"), "{first:?}: {pull:?}");
        match first {
            FirstReceipt::ThisGeneration => {
                let inbox = fixture.store.callback_inbox(former).unwrap();
                let keys: Vec<String> = inbox.into_iter().map(|(key, _)| key).collect();
                fixture.store.ack_callbacks(former, &keys).unwrap();
            }
            FirstReceipt::Older => older_acknowledges(&outage),
        }
        // The successor's task becomes the former's: the PULL counts it and its child, once.
        (elsewhere.hand_over("scratch/theirs", successor, former)).unwrap();
        let back = outage.back();
        let told = heard(fixture, sender, &[(&back, former)]).remove(0);
        // Its first PULL, acknowledged here, says what changed since; one an older dispatcher
        // acknowledged was not recorded as said, so the whole digest is said again.
        let head = match first {
            FirstReceipt::ThisGeneration => CHANGED,
            FirstReceipt::Older => WHOLE,
        };
        assert_eq!(told, handed_over(&outage, head), "{first:?}");
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
    // What changed since the PULL the receipt took, which counted nine.
    let pull = pull_lines(&told);
    let changed =
        "PULL ready_unstaffed=10 (was 9)\nready_unstaffed p2 scratch/theirs — Jump departure";
    assert_eq!(pull, changed, "{told:?}");
    said_once(&outage, &back);
}

/// The entries `handle`'s callback record holds as told, of either generation's acknowledgement.
fn recorded(client: &Database, handle: &str) -> BTreeSet<Entry> {
    let found = Raw::of(client).row("callback_state", handle);
    serde_json::from_value(found.field("pull_entries").unwrap()).unwrap()
}

/// What a leased dispatch met: the entries an inbox receipt during it recorded as told, the keys it
/// acknowledged still there after it, and whether the dispatch read the board again after it.
#[derive(Default)]
struct Receipt {
    recorded: Option<BTreeSet<Entry>>,
    remaining: Vec<String>,
    read_again: bool,
}

/// What follows the former's inbox receipt before the dispatch writes.
#[derive(Clone, Copy, Debug, PartialEq)]
enum AfterReceipt {
    /// Nothing: the events it acknowledged stay, delivered.
    Nothing,
    /// A presence sweep on this host, which takes no lease either, prunes them.
    SweptHere,
    /// One on another host prunes them.
    SweptElsewhere,
}

#[test]
fn an_inbox_receipt_while_a_dispatch_builds_the_digest_again_is_never_undone() {
    let cases = [
        AfterReceipt::Nothing,
        AfterReceipt::SweptHere,
        AfterReceipt::SweptElsewhere,
    ];
    for after in cases {
        let outage = Outage::on();
        let Outage {
            fixture,
            elsewhere,
            former,
            successor,
            sender,
        } = &outage;
        fixture.task("scratch/foreign");
        let foreign = fixture
            .store
            .take_role("scratch/foreign", successor, "orchestrator");
        foreign.unwrap();
        outage.cut();
        // A PULL of new work waits, and the former reads it from its inbox.
        let free = NewTask::new("scratch/free", "A heard row", "owner");
        elsewhere.create(&free).unwrap();
        fixture.store.dispatch_callbacks_snapshot().unwrap();
        let pull = waiting_pull(&outage).expect("a PULL waits");
        assert!(pull.text.contains("scratch/free"), "{after:?}: {pull:?}");
        let inbox = fixture.store.callback_inbox(former).unwrap();
        let keys: Vec<String> = inbox.into_iter().map(|(key, _)| key).collect();
        let acknowledged = keys.clone();
        // The successor's new work has the next dispatch build the digest again. Once that
        // dispatch holds the former's lease and has read its record (past its board's read), the
        // former acknowledges what it read, taking no lease, and a sweep may follow.
        let new = NewTask {
            parent: Some("scratch/foreign".into()),
            ..NewTask::new("scratch/foreign/new", "Another row", "owner")
        };
        elsewhere.create(&new).unwrap();
        let (client, other, here) = (
            fixture.other_database(),
            fixture.elsewhere(),
            fixture.other(),
        );
        let round = std::env::temp_dir().join(format!("{}-sweep.json", client.database()));
        let met = Arc::new(Mutex::new(Receipt::default()));
        let (recipient, during) = (former.clone(), Arc::clone(&met));
        let dispatcher = fixture.busy(move || {
            let mut during = during.lock().unwrap();
            let lease = || {
                let until = Raw::of(&client).row("callback_lease", recipient.as_str());
                let until = until.field("until").unwrap();
                !until.is_null() && until.as_f64() != Some(0.0)
            };
            if during.read_again || !lease() {
                return;
            }
            let trace = std::backtrace::Backtrace::force_capture().to_string();
            let board = trace.contains("callback_board");
            match during.recorded {
                None if trace.contains("dispatch_one") && !board => {
                    other.ack_callbacks(&recipient, &keys).unwrap();
                    during.recorded = Some(recorded(&client, &recipient));
                    match after {
                        AfterReceipt::Nothing => {}
                        AfterReceipt::SweptHere => sweep_presence(&here, round.clone()),
                        AfterReceipt::SweptElsewhere => sweep_presence(&other, round.clone()),
                    }
                    let there = |key: &&String| {
                        let event = json!([recipient, key]);
                        (Raw::of(&client).row("callback_event", event).exists()).unwrap()
                    };
                    during.remaining = keys.iter().filter(there).cloned().collect();
                }
                Some(_) => during.read_again = board,
                None => {}
            }
        });
        dispatcher.dispatch_callbacks_snapshot().unwrap();
        let Receipt {
            recorded: at_receipt,
            remaining,
            read_again,
        } = std::mem::take(&mut *met.lock().unwrap());
        let at_receipt = at_receipt.expect("the former acknowledged during the leased dispatch");
        let row = ("ready_unstaffed".to_string(), "scratch/free".to_string());
        assert!(at_receipt.contains(&row), "{after:?}: {at_receipt:?}");
        let kept = match after {
            AfterReceipt::Nothing => acknowledged,
            AfterReceipt::SweptHere | AfterReceipt::SweptElsewhere => Vec::new(),
        };
        assert_eq!(remaining, kept, "{after:?}");
        // The dispatch had read what it was told before the receipt: it reads and decides again.
        assert!(
            read_again,
            "{after:?}: the dispatch never read the board again"
        );
        // What the receipt recorded stands: no PULL waits, and none says that row again.
        let now = recorded(fixture.store.database(), former);
        assert!(now.contains(&row), "{after:?}: {now:?}");
        let waiting = waiting_pull(&outage);
        assert!(waiting.is_none(), "{after:?}: {waiting:?}");
        let back = outage.back();
        let told = heard(fixture, sender, &[(&back, former)]);
        assert_eq!(told, [Vec::<String>::new()], "{after:?}");
        outage.pass(601.0, &[]);
        let later = heard(fixture, sender, &[(&back, former)]);
        assert_eq!(later, [Vec::<String>::new()], "{after:?}");
        let older = older_fields(fixture, former);
        assert_eq!(older, older_record(fixture, former), "{after:?}");
    }
}

/// What the former orchestrator heard of the successor's task before it was handed over.
#[derive(Clone, Copy, Debug)]
enum Before {
    /// Nothing: the PULL an older dispatcher delivered was this generation's, which withheld it.
    Withheld,
    /// An older dispatcher's own PULL, queued for the task's departure and return, told it the
    /// whole board: the PULL after the hand-over is the one repeat.
    ToldByAnOlderPull,
}

#[test]
fn work_a_hand_over_brings_is_counted_in_one_pull_however_many_an_older_dispatcher_queues_after() {
    for before in [Before::Withheld, Before::ToldByAnOlderPull] {
        let outage = waited(Attempts::One);
        let Outage {
            fixture,
            elsewhere,
            former,
            successor,
            sender,
        } = &outage;
        if let Before::ToldByAnOlderPull = before {
            (elsewhere.move_to("scratch/theirs", "backlog", sender)).unwrap();
            older_dispatch(&outage, Older::Computes);
            (elsewhere.move_to("scratch/theirs", "ready", sender)).unwrap();
            older_dispatch(&outage, Older::Computes);
        }
        older_acknowledges(&outage);
        // The successor orchestrates other work too, which the former never hears.
        let foreign = NewTask::new("scratch/foreign", "Jump departure", "owner");
        elsewhere.create(&foreign).unwrap();
        (elsewhere.take_role("scratch/foreign", successor, "orchestrator")).unwrap();
        // One hand-over: one PULL counts the task, and an older dispatcher delivers it.
        (elsewhere.hand_over("scratch/theirs", successor, former)).unwrap();
        fixture.store.dispatch_callbacks_snapshot().unwrap();
        let counted = waiting_pull(&outage).expect("a PULL waits");
        let head = "PULL executing=0 ready_unstaffed=10 ";
        assert!(counted.text.starts_with(head), "{before:?}: {counted:?}");
        older_acknowledges(&outage);
        // Before this generation looks again, it queues a PULL of its own for the successor's new
        // work, and again each time more comes: none says anything new to the former, and each is
        // taken back.
        for cycle in 0..3 {
            let new = NewTask {
                parent: Some("scratch/foreign".into()),
                ..NewTask::new(&format!("scratch/foreign/{cycle}"), "A row", "owner")
            };
            elsewhere.create(&new).unwrap();
            older_dispatch(&outage, Older::Computes);
            let own = waiting_pull(&outage).expect("its own PULL waits");
            let whole = format!(" ready_unstaffed={} ", 12 + cycle);
            assert!(
                own.text.contains(&whole),
                "{before:?}, cycle {cycle}: {own:?}"
            );
            fixture.store.dispatch_callbacks_snapshot().unwrap();
            let waiting = waiting_pull(&outage);
            assert!(waiting.is_none(), "{before:?}, cycle {cycle}: {waiting:?}");
        }
        let back = outage.back();
        let told = heard(fixture, sender, &[(&back, former)]).remove(0);
        assert_eq!(told, Vec::<String>::new(), "{before:?}");
        said_once(&outage, &back);
    }
}

/// A dispatcher that keeps `pull_awaiting` but not `pull_said` (one built before PULLs said only
/// what changed) delivers what waits for the former orchestrator: it records the PULL's entries as
/// told and ends what was awaited, and leaves `pull_said` as this generation last recorded it.
fn unsaid_acknowledges(outage: &Outage) {
    let pull = waiting_pull(outage).expect("a PULL waits");
    let raw = outage.fixture.store.raw();
    let pull = raw
        .row("callback_event", json!([outage.former, pull.key]))
        .get()
        .unwrap();
    let state = raw.row("callback_state", outage.former.as_str());
    let mut unanswered: BTreeSet<Entry> =
        serde_json::from_value(state.field("pull_unanswered").unwrap()).unwrap_or_default();
    let entries: BTreeSet<Entry> = serde_json::from_value(pull["pull_entries"].clone()).unwrap();
    let withheld: BTreeSet<Entry> =
        serde_json::from_value(pull["pull_withheld"].clone()).unwrap_or_default();
    unanswered.extend(
        entries
            .iter()
            .filter(|e| e.0 == "unanswered" && !withheld.contains(*e))
            .cloned(),
    );
    state.set(json!({"pull_unanswered":unanswered,"pull_entries":entries,"pull_withheld":withheld,"pull_awaiting":null,"pull_queued":null})).unwrap();
    raw.table("callback_event")
        .matching(json!({"handle":outage.former}))
        .set(json!({"delivered":true}))
        .unwrap();
}

#[test]
fn a_pull_acknowledged_without_its_baseline_is_followed_by_the_whole_digest() {
    let outage = Outage::on();
    let Outage {
        fixture,
        elsewhere,
        former,
        sender,
        ..
    } = &outage;
    // This generation recorded what the first PULL said; the next, telling scratch/free, is
    // delivered by a dispatcher that does not record it.
    outage.cut();
    let free = NewTask::new("scratch/free", "Jump departure", "owner");
    elsewhere.create(&free).unwrap();
    fixture.store.dispatch_callbacks_snapshot().unwrap();
    unsaid_acknowledges(&outage);
    let more = NewTask::new("scratch/more", "Jump departure", "owner");
    elsewhere.create(&more).unwrap();
    let back = outage.back();
    let told = heard(fixture, sender, &[(&back, former)]).remove(0);
    // Measured against the first PULL it would say `ready_unstaffed=3 (was 1)`, though the one
    // it heard since counted two: it hears the whole digest, which names the row it was told.
    let pull = pull_lines(&told);
    let whole = "PULL executing=0 ready_unstaffed=3 in_progress_orphaned=0 review_unreviewed=0 landed_open=0 idle=0
ready_unstaffed p2 scratch/free — Jump departure
ready_unstaffed p2 scratch/more — Jump departure
ready_unstaffed p2 scratch/one — Jump departure";
    assert_eq!(pull, whole, "{told:?}");
    said_once(&outage, &back);
}
