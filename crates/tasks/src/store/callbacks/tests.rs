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
        Instant::now() + Duration::from_secs(1),
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
        Instant::now() + Duration::from_secs(1),
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
        Instant::now() + Duration::from_secs(1),
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
