//! What an orchestrator was told, whichever generation acknowledged its PULL: this one's
//! acknowledgement records what the PULL withheld, an older one's only the entries it records,
//! read over the PULL this one left awaiting. An older write that queues no PULL of its own leaves
//! that exact; only an older dispatcher's own PULL of the same entries leaves the same record, so
//! work withheld from the orchestrator is counted once more after one when a hand-over makes it
//! its own, never lost, and nothing repeats after.

use super::hand_over_outage::Outage;
use super::orchestrators::heard;
use super::pending_pull::{Attempts, pull_lines, said_once, waited, waiting_pull};
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeSet;
use symphony_board::NewTask;

/// A digest entry: its list, and the slug or handle it names.
type Entry = (String, String);

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
    let client = fixture.store.client();
    let found = client.query(
        "RETURN SELECT pull_entries, pull_queued FROM ONLY type::record('callback_state', $handle);",
        json!({"handle":former}),
    );
    let before: Recorded = serde_json::from_value(found.unwrap().remove(0)).unwrap();
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
        let (mine, whole) = ("PULL executing=0 ready_unstaffed=9 ", "ready_unstaffed=10 ");
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

#[test]
fn an_older_observation_never_hides_work_withheld_from_the_orchestrator_until_it_is_its_own() {
    let outage = Outage::new();
    let Outage {
        fixture,
        elsewhere,
        former,
        successor,
        sender,
    } = &outage;
    // The successor's task has a child awaiting review; neither is the former's.
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
    (fixture
        .store
        .move_to("scratch/theirs/review", "review", sender))
    .unwrap();
    outage.cut();
    assert!(waiting_pull(&outage).is_none());
    // A post has an older dispatcher write its observation: it queues no PULL, and none is
    // acknowledged.
    (elsewhere.say("scratch/one", sender, "An ordinary thread note", None)).unwrap();
    older_dispatch(&outage, Older::Observes);
    assert!(waiting_pull(&outage).is_none());
    // The successor's task becomes the former's: the ROLE names it, and the PULL its child too.
    (elsewhere.hand_over("scratch/theirs", successor, former)).unwrap();
    let back = outage.back();
    let told = heard(fixture, sender, &[(&back, former)]).remove(0);
    let moved = format!("ROLE scratch/theirs {former} orchestrator, ready (from {successor})");
    let pull = "PULL executing=1 ready_unstaffed=1 in_progress_orphaned=0 review_unreviewed=1 landed_open=0 idle=0";
    let expected = [
        moved.as_str(),
        pull,
        "ready_unstaffed p2 scratch/theirs — Jump departure",
        "review_unreviewed p2 scratch/theirs/review — Needs review",
    ];
    assert_eq!(told, expected);
    said_once(&outage, &back);
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
