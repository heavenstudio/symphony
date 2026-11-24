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
