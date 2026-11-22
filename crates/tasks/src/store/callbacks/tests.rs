use super::*;

#[test]
fn plain_orchestrator_tie_break_does_not_depend_on_callback_row_order() {
    let callback = |handle: &str| {
        let mut row: Callback = serde_json::from_value(json!({
            "handle":handle, "address":"codex:abcdef01", "host":"test",
            "session":crate::rows::SessionRow::default()
        }))
        .unwrap();
        row.live_orchestrator = true;
        row
    };
    for rows in [
        vec![callback("zinc-otter"), callback("amber-heron")],
        vec![callback("amber-heron"), callback("zinc-otter")],
    ] {
        assert_eq!(recipient(&rows), Some("amber-heron"));
    }
}

#[test]
fn capacity_splits_between_events_and_never_loses_or_reorders_them() {
    let mut attempts = Vec::new();
    let mut delivered = Vec::new();
    deliver_batches(
        &[1, 2, 3, 4, 5],
        1.0,
        &|| 0.0,
        &mut |batch| {
            attempts.push(batch.to_vec());
            if batch.len() > 2 {
                Err(DeliveryError::capacity("full"))
            } else {
                Ok(())
            }
        },
        &mut |batch, result| {
            assert!(result.is_ok());
            delivered.extend_from_slice(batch);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(
        attempts,
        [
            vec![1, 2, 3, 4, 5],
            vec![1, 2],
            vec![3, 4, 5],
            vec![3],
            vec![4, 5]
        ]
    );
    assert_eq!(delivered, [1, 2, 3, 4, 5]);
}

#[test]
fn an_oversized_event_is_retained_but_does_not_block_smaller_events() {
    let mut recorded = Vec::new();
    deliver_batches(
        &[1, 2],
        1.0,
        &|| 0.0,
        &mut |batch| {
            if batch.contains(&1) {
                Err(DeliveryError::capacity("large"))
            } else {
                Ok(())
            }
        },
        &mut |batch, result| {
            recorded.push((batch.to_vec(), result.is_ok()));
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(recorded, [(vec![1], false), (vec![2], true)]);
}

#[test]
fn a_transport_failure_retains_the_whole_batch_without_splitting_or_retrying_it() {
    let mut attempts = 0;
    let mut recorded = Vec::new();
    deliver_batches(
        &[1, 2, 3],
        1.0,
        &|| 0.0,
        &mut |_| {
            attempts += 1;
            Err(DeliveryError::retry("short write"))
        },
        &mut |batch, result| {
            recorded.push((batch.to_vec(), result.is_ok()));
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(attempts, 1);
    assert_eq!(recorded, [(vec![1, 2, 3], false)]);
}

/// The batches a delivery tried, in order.
type Attempts = Vec<Vec<i32>>;
/// The batches it recorded, each with whether it was sent.
type Recorded = Vec<(Vec<i32>, bool)>;

/// Each attempt takes ten seconds of the clock it is handed. The oversized first event is tried
/// twice (the whole queue, then alone): the second event's turn comes at 20 s.
fn delivered_by(deadline: f64) -> (Attempts, Recorded) {
    let clock = std::cell::Cell::new(0.0);
    let (mut attempts, mut recorded) = (Vec::new(), Vec::new());
    deliver_batches(
        &[1, 2],
        deadline,
        &|| clock.get(),
        &mut |batch| {
            attempts.push(batch.to_vec());
            clock.set(clock.get() + 10.0);
            match batch.contains(&1) {
                true => Err(DeliveryError::capacity("large")),
                false => Ok(()),
            }
        },
        &mut |batch, result| {
            recorded.push((batch.to_vec(), result.is_ok()));
            Ok(())
        },
    )
    .unwrap();
    (attempts, recorded)
}

#[test]
fn no_batch_begins_at_or_past_the_deadline_by_the_clock_it_is_handed() {
    // The first event's lone attempt began at 10 s, before the deadline: it is recorded, though it
    // ended past it. The second's turn, at 20 s, is at or past the deadline: never tried, never
    // recorded, still due for the next dispatcher with its whole budget.
    for deadline in [15.0, 20.0] {
        assert_eq!(
            delivered_by(deadline),
            (vec![vec![1, 2], vec![1]], vec![(vec![1], false)]),
            "deadline {deadline}"
        );
    }
    // Before it, the second goes.
    assert_eq!(
        delivered_by(20.5),
        (
            vec![vec![1, 2], vec![1], vec![2]],
            vec![(vec![1], false), (vec![2], true)]
        )
    );
    // A deadline already past sends nothing at all.
    assert_eq!(delivered_by(0.0), (vec![], vec![]));
}

/// A plain line keeps what it is given, starts unattempted and carries no PULL: on the outbox it
/// is written in the very shape the literal it replaced wrote, `pull_said` left out.
#[test]
fn a_plain_line_is_unattempted_and_carries_no_pull() {
    let message = Event::line(
        "message-7".into(),
        "amber-wren",
        "MSG #7".into(),
        None,
        Some(7),
    );
    assert_eq!(
        serde_json::to_value(&message).unwrap(),
        json!({"key":"message-7", "handle":"amber-wren", "text":"MSG #7", "slug":null,
            "message":7, "attempts":0, "sequence":0, "pull_entries":null, "pull_withheld":null})
    );
    let line = Event::line(
        "event-3-0".into(),
        "zinc-otter",
        String::new(),
        Some("p/a".into()),
        None,
    );
    assert_eq!((line.slug.as_deref(), line.message), (Some("p/a"), None));
    assert_eq!(line.text, "");
    assert!(line.pull_entries.is_none() && line.pull_withheld.is_none());
    assert!(line.pull_said.is_none());
}

/// A board read afresh drops the pass's inbox pool, whether one was made or not, and reads the
/// board as it stands, the task filed since the last read on it.
#[test]
fn a_fresh_board_drops_the_inbox_pool_and_reads_the_board_as_it_stands() {
    let fixture = crate::store_fixture::Fixture::empty();
    let store = &fixture.store;
    let pooled = || (store.inbox_pool.lock().unwrap()).is_some();
    let (board, _) = store.fresh_board().unwrap();
    assert!(board.tasks.is_empty());
    assert!(!pooled());
    store.could_hear("amber-wren", 0, 0).unwrap();
    assert!(pooled(), "a pass's read of an inbox makes the pool");
    fixture.task("p/filed");
    let (board, sequence) = store.fresh_board().unwrap();
    assert!(!pooled());
    assert_eq!(sequence, store.board_marker().unwrap());
    assert_eq!(
        (board.tasks.iter().map(|t| t.slug.as_str())).collect::<Vec<_>>(),
        ["p/filed"]
    );
}
