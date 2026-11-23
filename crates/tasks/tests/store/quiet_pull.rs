//! Durable PULL delivery and unanswered-post identity across fresh dispatchers.

use super::*;

#[path = "pull_recovery.rs"]
mod recovery;

#[test]
fn the_first_pull_is_delivered_once_even_when_its_lists_are_empty() {
    for existing_gap in [false, true] {
        let f = Fixture::new(crate::common::Seed::default());
        let orch = f.session("orchestrator");
        let sender = f.session("worker");
        if existing_gap {
            f.task("quiet-test/initial-ready");
        }
        let o = listen(&f, &orch);
        let expected = if existing_gap {
            "PULL executing=0 ready_unstaffed=1 in_progress_orphaned=0 review_unreviewed=0 landed_open=0 idle=0\nready_unstaffed p2 quiet-test/initial-ready — Jump departure"
        } else {
            EMPTY_PULL
        };
        assert_eq!(o.text(), with_volume(&orch, 0, expected));
        assert_eq!(callback_state(&f, &orch)["delivery_version"], 1);
        assert_eq!(
            callback_state(&f, &orch)["pull_entries"],
            if existing_gap {
                json!([
                    ["low_volume", orch],
                    ["ready_unstaffed", "quiet-test/initial-ready"]
                ])
            } else {
                json!([["low_volume", orch]])
            }
        );
        for age in [0.0, 601.0, 86_400.0] {
            f.advance(age);
            f.store.touch(&orch).unwrap();
            f.store.dispatch_callbacks().unwrap();
            assert!(o.drain(&f, &sender, &orch).is_empty());
        }
        f.store
            .callback(
                &orch,
                Some(&format!("uds:{}", o.path.display())),
                false,
                false,
            )
            .unwrap();
        assert!(o.drain(&f, &sender, &orch).is_empty());
    }
}

#[test]
fn native_unanswered_lines_survive_fresh_dispatchers_without_repeating_a_post() {
    let f = Fixture::new(crate::common::Seed::default());
    let orch = f.session("orchestrator");
    let worker = f.session("worker");
    let barrier = f.session("worker");
    f.task("tools/unanswered");
    f.store.claim("tools/unanswered", &worker).unwrap();
    let listener = listen_orchestrator(&f, &orch, BUSY_PULL);
    let first = f
        .store
        .say("tools/unanswered", "owner", "continue", Some("@executor"))
        .unwrap();
    listener.drain(&f, &barrier, &orch);
    f.advance(29.0 * 60.0);
    // Inspect the age cutoff without scheduling a periodic PULL at minute 29. The first due
    // stream observation below is minute 31, rather than ten minutes after an intervening check.
    assert_eq!(
        symphony_board::cli::pull::digest(&f.store, &[], None, f.now(), &[])
            .unwrap()
            .count("unanswered"),
        0
    );
    f.advance(2.0 * 60.0);
    f.other().dispatch_callbacks().unwrap();
    let mut received = listener.drain(&f, &barrier, &orch);
    assert_eq!(
        received
            .iter()
            .filter(|line| line.starts_with("unanswered "))
            .count(),
        1,
        "{received:?}"
    );
    f.task("tools/another-gap");
    received.extend(listener.drain(&f, &barrier, &orch));
    f.store.touch(&worker).unwrap();
    f.other().dispatch_callbacks().unwrap();
    received.extend(listener.drain(&f, &barrier, &orch));
    f.advance(31.0 * 60.0);
    f.other().dispatch_callbacks().unwrap();
    received.extend(listener.drain(&f, &barrier, &orch));
    assert_eq!(
        received
            .iter()
            .filter(|line| line.starts_with("unanswered ")
                && line.contains(&format!("post #{} —", first.id)))
            .count(),
        1,
        "{received:?}"
    );
    let next = f
        .store
        .say("tools/unanswered", "owner", "next request", Some(&worker))
        .unwrap();
    listener.drain(&f, &barrier, &orch);
    f.advance(31.0 * 60.0);
    f.other().dispatch_callbacks().unwrap();
    let received = listener.drain(&f, &barrier, &orch);
    let pending: Vec<_> = received
        .iter()
        .filter(|line| line.starts_with("unanswered "))
        .collect();
    assert_eq!(pending.len(), 1, "{received:?}");
    assert!(pending[0].contains(&format!("post #{} —", next.id)));
}

/// A launch record no reader decodes, on the task the scan asks about, which fails the scan's read
/// of that task's thread and launches.
const UNREADABLE_LAUNCH: &str = "DEFINE FIELD OVERWRITE started ON task_session TYPE any;
UPSERT task_session:['tools/unanswered', 'nobody'] CONTENT { slug: 'tools/unanswered',
    handle: 'nobody', role: 'executor', claude_session: '', worktree: '', branch: '', pane: '',
    model: '', started: 'unreadable', closed: 1.0, closed_why: '', restore: '' } RETURN NONE;";
const READABLE_LAUNCHES: &str = "DELETE task_session:['tools/unanswered', 'nobody'] RETURN NONE;
DEFINE FIELD OVERWRITE started ON task_session TYPE float;";

// SurrealDB alone: injects a failed scan through a SurrealDB field definition and HTTP gate.
#[test]
fn a_failed_unanswered_scan_delivers_the_rest_and_never_repeats_a_told_post() {
    use crate::common::{HOST, RawWrite as _};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let f = Fixture::empty();
    let orch = f.session("orchestrator");
    let worker = f.session("worker");
    let barrier = f.session("worker");
    f.task("tools/unanswered");
    f.store.claim("tools/unanswered", &worker).unwrap();
    let listener = listen_orchestrator(&f, &orch, BUSY_PULL);
    let post = f
        .store
        .say("tools/unanswered", "owner", "continue", Some("@executor"))
        .unwrap();
    listener.drain(&f, &barrier, &orch);
    f.advance(31.0 * 60.0);
    f.other().dispatch_callbacks().unwrap();
    let told = |lines: &[String]| {
        (lines.iter())
            .filter(|line| line.starts_with("unanswered "))
            .count()
    };
    let header = |lines: &[String]| {
        let pull = lines.iter().find(|line| line.starts_with("PULL "));
        pull.unwrap_or_else(|| panic!("no PULL: {lines:?}")).clone()
    };
    let naming = |lines: &[String], slug: &str| {
        (lines.iter()).any(|line| line.starts_with(&format!("ready_unstaffed p2 {slug} ")))
    };
    let first = listener.drain(&f, &barrier, &orch);
    assert_eq!(told(&first), 1, "{first:?}");
    assert!(header(&first).contains(" unanswered=1 "), "{first:?}");
    assert!(
        first
            .iter()
            .any(|line| line.contains(&format!("post #{} —", post.id))),
        "{first:?}"
    );

    // A dispatcher whose scan alone fails: the launch records are unreadable from the digest's
    // read just before the scan until the request after the scan's own read of them, so the
    // agents view earlier in the same pass, and all that follows, read the board as it is.
    let (writer, skips) = (f.other_client(), Arc::new(AtomicUsize::new(0)));
    let (counted, mut unreadable, mut scanned) = (Arc::clone(&skips), false, false);
    let relay = crate::relay::Relay::with_before(
        f.store.client().addr(),
        0,
        Some(Box::new(move |sql| {
            if sql.contains("'landed ' + slug + ' on main'") {
                writer.write(UNREADABLE_LAUNCH, json!({})).unwrap();
                (unreadable, scanned) = (true, false);
            } else if unreadable && sql.contains("launches: array::flatten($launched") {
                counted.fetch_add(1, Ordering::SeqCst);
                scanned = true;
            } else if unreadable && scanned {
                writer.write(READABLE_LAUNCHES, json!({})).unwrap();
                unreadable = false;
            }
        })),
    );
    let now = f.now();
    let scanless = symphony_board::Store::with(
        relay.client("sf", f.store.database().database()),
        Box::new(move || now),
        Box::new(|_| true),
        HOST,
    )
    .unwrap();
    f.elsewhere()
        .create(&NewTask::new("tools/gap-while-skipped", "Gap", "owner"))
        .unwrap();
    scanless.dispatch_callbacks().unwrap();
    assert!(
        skips.load(Ordering::SeqCst) > 0,
        "the scan was never failed"
    );
    drop(scanless);
    drop(relay);
    f.store
        .client()
        .write(READABLE_LAUNCHES, json!({}))
        .unwrap();
    // The barrier's own dispatch scans again: the post is back in the digest and is not told again.
    let skipped = listener.drain(&f, &barrier, &orch);
    assert!(naming(&skipped, "tools/gap-while-skipped"), "{skipped:?}");
    assert!(!header(&skipped).contains("unanswered="), "{skipped:?}");
    assert_eq!(told(&skipped), 0, "{skipped:?}");
    f.other().dispatch_callbacks().unwrap();
    assert!(listener.drain(&f, &barrier, &orch).is_empty());

    // A PULL another entry causes counts the post and still does not tell it twice.
    f.elsewhere()
        .create(&NewTask::new("tools/gap-after", "Gap", "owner"))
        .unwrap();
    f.other().dispatch_callbacks().unwrap();
    let back = listener.drain(&f, &barrier, &orch);
    assert!(naming(&back, "tools/gap-after"), "{back:?}");
    assert!(header(&back).contains(" unanswered=1 "), "{back:?}");
    assert_eq!(told(&back), 0, "{back:?}");
}
