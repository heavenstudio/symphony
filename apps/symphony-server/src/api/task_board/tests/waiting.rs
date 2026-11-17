use super::*;
use symphony_api::{Task, WaitingColumn, WaitsOn};

fn waiting(slug: &str, status: &str, kind: &str, since: f64) -> Task {
    Task {
        from: (status == "blocked").then(|| "review".into()),
        waits_on: Some(WaitsOn {
            kind: kind.into(),
            who: if kind == "parked" { "" } else { "named-thing" }.into(),
            why: "its reason".into(),
            since,
        }),
        ..task(slug.into(), status, 2, since)
    }
}

async fn answer(records: &[Task], query: &str) -> TaskListing {
    listing_of(records, &format!("view=waiting&{query}")).await
}

fn column<'a>(list: &'a TaskListing, status: &str) -> &'a WaitingColumn {
    list.waiting
        .as_ref()
        .unwrap()
        .columns
        .iter()
        .find(|c| c.status == status)
        .unwrap()
}

#[tokio::test]
async fn blocked_rows_stay_in_their_original_column_with_parked_last() {
    let records = vec![
        waiting("parked", "blocked", "parked", 1.0),
        waiting("ordinary-new", "review", "queued", 60.0),
        waiting("task", "blocked", "task", 2.0),
        waiting("check", "blocked", "check", 30.0),
        waiting("decision-new", "blocked", "decision", 50.0),
        waiting("nobody", "blocked", "nobody", 10.0),
        waiting("decision-old", "blocked", "decision", 40.0),
        waiting("ordinary-old", "review", "working", 20.0),
        waiting("other-stage", "design", "queued", 1.0),
    ];
    let list = answer(&records, "status=review").await;
    assert_eq!(
        slugs(&list),
        [
            "decision-old",
            "decision-new",
            "nobody",
            "check",
            "task",
            "ordinary-old",
            "ordinary-new",
            "parked"
        ]
    );
    assert_eq!(total(&list, "review"), 8);
    assert_eq!(total(&list, "design"), 1);
    assert!(!list.statuses.contains(&"blocked".into()));
    assert_eq!(
        column(&list, "review"),
        &WaitingColumn {
            status: "review".into(),
            ordinary: 2,
            blocked: 6,
            oldest: Some(1.0)
        }
    );
    let counts = answer(&records, "counts=1").await;
    assert!(counts.tasks.is_empty());
    assert_eq!(counts.waiting, list.waiting);
    assert_eq!(counts.counts, list.counts);
    assert_eq!(
        list.waiting.as_ref().unwrap().kinds,
        vec![
            ("decision".into(), 2),
            ("nobody".into(), 1),
            ("check".into(), 1),
            ("task".into(), 1),
            ("working".into(), 1),
            ("queued".into(), 2),
            ("parked".into(), 1),
        ]
    );
}

#[tokio::test]
async fn missing_and_invalid_blocks_are_ordinary_work_and_named_legacy_blocks_have_a_home() {
    let mut records = Vec::new();
    for (slug, kind, who) in [
        ("unknown", "new-kind", "thing"),
        ("empty", "decision", ""),
        ("person", "working", "handle"),
        ("queued", "queued", ""),
        ("no-reason", "nobody", ""),
    ] {
        let mut row = waiting(slug, "blocked", kind, 10.0);
        row.waits_on.as_mut().unwrap().who = who.into();
        records.push(row);
    }
    let mut missing = waiting("missing", "blocked", "check", 10.0);
    missing.waits_on = None;
    missing.from = Some("invented".into());
    records.push(missing);
    let mut named = waiting("named", "blocked", "check", 10.0);
    named.from = None;
    records.push(named);
    let list = answer(&records, "all=1").await;
    assert_eq!(column(&list, "review").ordinary, 5);
    assert_eq!(column(&list, "review").blocked, 0);
    assert_eq!(column(&list, "ready").ordinary, 1);
    assert_eq!(column(&list, "ready").blocked, 1);
    for row in &list.tasks {
        if row.slug != "named" {
            assert_ne!(row.status, "blocked");
            let waits = row.waits_on.as_ref().unwrap();
            assert_eq!(
                (&*waits.kind, &*waits.why),
                ("nobody", "no reason recorded")
            );
        }
    }
    // The default endpoint still returns exactly the lifecycle rows it was given.
    let default = listing_of(&records, "all=1").await;
    assert!(default.tasks.iter().all(|row| row.status == "blocked"));
    assert!(default.waiting.is_none());
}

#[tokio::test]
async fn tied_rows_page_without_duplicates_in_numeric_slug_order() {
    let records: Vec<_> = (1..=75)
        .rev()
        .map(|n| waiting(&format!("task-{n}"), "blocked", "check", 1.0))
        .collect();
    let first = answer(&records, "status=review").await;
    let second = answer(
        &records,
        &format!("status=review&cursor={}", cursor(&first)),
    )
    .await;
    let third = answer(
        &records,
        &format!("status=review&cursor={}", cursor(&second)),
    )
    .await;
    let last = answer(
        &records,
        &format!("status=review&cursor={}", cursor(&third)),
    )
    .await;
    assert_eq!(
        [
            first.tasks.len(),
            second.tasks.len(),
            third.tasks.len(),
            last.tasks.len()
        ],
        [30, 30, 15, 0]
    );
    let actual: Vec<_> = [&first, &second, &third]
        .into_iter()
        .flat_map(slugs)
        .collect();
    assert_eq!(
        actual,
        (1..=75).map(|n| format!("task-{n}")).collect::<Vec<_>>()
    );
    for page in [&first, &second, &third, &last] {
        assert_eq!(column(page, "review").blocked, 75);
        assert_eq!(total(page, "review"), 75);
    }
    assert!(last.next.is_none());
    let long = answer(&records, "status=review&rows=45").await;
    assert_eq!(long.tasks.len(), 45);
    let after = answer(&records, &format!("status=review&cursor={}", cursor(&long))).await;
    assert_eq!(after.tasks.first().unwrap().slug, "task-46");
    let offset = answer(&records, "status=review&offset=60").await;
    assert_eq!(offset.tasks.len(), 15);
}

#[tokio::test]
async fn a_wait_kind_lists_across_stages_and_keeps_the_other_facets_complete() {
    let records = vec![
        waiting("later", "ready", "check", 5.0),
        waiting("earlier", "design", "check", 1.0),
        waiting("blocked", "blocked", "check", 3.0),
        waiting("queued", "ready", "queued", 1.0),
        waiting("finished", "done", "check", 1.0),
    ];
    let list = answer(&records, "wait=check&older=1").await;
    assert_eq!(slugs(&list), ["blocked", "earlier", "later"]);
    assert_eq!(list.counts.iter().map(|(_, n)| n).sum::<usize>(), 3);
    assert_eq!(
        list.waiting.as_ref().unwrap().kinds[5],
        ("queued".into(), 1)
    );
    assert_eq!(column(&list, "ready").ordinary, 1);
    assert_eq!(column(&list, "review").blocked, 1);
    let selected = answer(&records, "wait=check&status=design").await;
    assert_eq!(slugs(&selected), ["earlier"]);
    assert_eq!(selected.counts, list.counts);
    let empty = answer(&records, "wait=decision").await;
    assert!(empty.tasks.is_empty());
    assert!(empty.counts.is_empty());
    assert!(empty.waiting.as_ref().unwrap().columns.is_empty());
    assert_eq!(empty.waiting.unwrap().kinds[2], ("check".into(), 3));
}

#[tokio::test]
async fn search_membership_activity_and_finished_window_precede_complete_counts() {
    let scores = BTreeMap::from([
        ("in_progress-build-00".into(), 1.0),
        ("in_progress-build-01".into(), 10.0),
        ("done-test-00".into(), 1.0),
    ]);
    let list = page("view=waiting&activity=1", "found", &scores).await;
    assert_eq!(
        slugs(&list),
        ["in_progress-build-00", "in_progress-build-01"]
    );
    assert_eq!(column(&list, "build").ordinary, 2);
    assert_eq!(list.waiting.unwrap().kinds[1], ("nobody".into(), 2));
    let offline = page("view=waiting&activity=0&counts=1", "found", &scores).await;
    assert_eq!(column(&offline, "in_progress").ordinary, 2);
    assert!(offline.tasks.is_empty());
    let older = page("view=waiting&activity=1&older=1", "found", &scores).await;
    assert_eq!(column(&older, "done").ordinary, 1);
    assert_eq!(
        older
            .waiting
            .as_ref()
            .unwrap()
            .kinds
            .iter()
            .map(|(_, n)| n)
            .sum::<usize>(),
        2
    );
    let without = page("view=waiting&activity=1&work=0", "found", &scores).await;
    assert_eq!(column(&without, "build").ordinary, 2);
    assert_eq!(without.machine_work, MachineWork::default());
}

#[tokio::test]
async fn waiting_projection_refuses_other_views_bad_kinds_and_other_cursors() {
    for query in [
        "parent=x",
        "holder=x",
        "roots=1",
        "role=executor",
        "status=signoff",
        "status=blocked",
        "wait=unknown",
        "cursor=bad",
        "cursor=%5B%22review%22,1,2,%22slug%22%5D",
    ] {
        let (tasks, work) = fixture();
        let request = Request::builder()
            .uri(format!("/api/task-board?view=waiting&{query}"))
            .body(())
            .unwrap();
        let response = project(&request, tasks, "", &BTreeMap::new(), work, vec![]);
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{query}");
    }
    let empty = answer(&[], "counts=1").await;
    assert_eq!(
        empty
            .waiting
            .unwrap()
            .kinds
            .iter()
            .map(|(_, n)| n)
            .sum::<usize>(),
        0
    );
    assert!(empty.counts.is_empty());
}

#[tokio::test]
async fn pages_cross_block_kinds_ordinary_and_parked_without_losing_rows() {
    let records: Vec<_> = [
        ("decision", "blocked", "decision"),
        ("check", "blocked", "check"),
        ("ordinary", "review", "working"),
        ("parked", "blocked", "parked"),
    ]
    .into_iter()
    .flat_map(|(prefix, status, kind)| {
        (1..=21)
            .rev()
            .map(move |n| waiting(&format!("{prefix}-{n}"), status, kind, 10.0))
    })
    .collect();
    let all = answer(&records, "status=review&all=1").await;
    assert_eq!(all.tasks.len(), 84);
    let first = answer(&records, "status=review").await;
    let second = answer(
        &records,
        &format!("status=review&cursor={}", cursor(&first)),
    )
    .await;
    let third = answer(
        &records,
        &format!("status=review&cursor={}", cursor(&second)),
    )
    .await;
    let actual: Vec<_> = [&first, &second, &third]
        .into_iter()
        .flat_map(slugs)
        .collect();
    assert_eq!(actual, slugs(&all));
    assert_eq!(actual[20], "decision-21");
    assert_eq!(actual[21], "check-1");
    assert_eq!(actual[42], "ordinary-1");
    assert_eq!(actual[63], "parked-1");
    // A cursor is a key, not a search for its former row: removal of that row skips nothing.
    let without_last: Vec<_> = records
        .iter()
        .filter(|row| row.slug != first.tasks.last().unwrap().slug)
        .cloned()
        .collect();
    let next = answer(
        &without_last,
        &format!("status=review&cursor={}", cursor(&first)),
    )
    .await;
    assert_eq!(slugs(&next), slugs(&second));
}

#[tokio::test]
async fn every_original_stage_places_blocks_and_activity_does_not_move_them() {
    let mut records = Vec::new();
    for from in [
        "concept",
        "design",
        "backlog",
        "ready",
        "in_progress",
        "review",
    ] {
        let mut row = waiting(from, "blocked", "decision", 10.0);
        row.from = Some(from.into());
        records.push(row);
    }
    let list = answer(&records, "all=1").await;
    for row in &records {
        assert_eq!(column(&list, &row.slug).blocked, 1);
        assert_eq!(column(&list, &row.slug).ordinary, 0);
    }
    let (mut tasks, work) = fixture();
    let row = &mut tasks.tasks[0];
    row.status = "blocked".into();
    row.from = Some("in_progress".into());
    row.waits_on = Some(WaitsOn {
        kind: "check".into(),
        who: "tests".into(),
        why: "a check".into(),
        since: 10.0,
    });
    let request = Request::builder()
        .uri("/api/task-board?view=waiting&activity=1&wait=check")
        .body(())
        .unwrap();
    let response = project(&request, tasks, "", &BTreeMap::new(), work, vec![]);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let listing: TaskListing = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(slugs(&listing), ["in_progress-build-00"]);
    assert_eq!(column(&listing, "in_progress").blocked, 1);
    assert_eq!(total(&listing, "build"), 0);
}

#[tokio::test]
async fn finished_work_is_newest_first_and_has_no_wait_facet() {
    let records = vec![
        waiting("done-10", "done", "working", 20.0),
        waiting("done-2", "done", "working", 20.0),
        waiting("done-1", "done", "working", 10.0),
    ];
    let list = answer(&records, "older=1&status=done").await;
    assert_eq!(slugs(&list), ["done-2", "done-10", "done-1"]);
    assert_eq!(column(&list, "done").ordinary, 3);
    assert_eq!(
        list.waiting
            .unwrap()
            .kinds
            .iter()
            .map(|(_, n)| n)
            .sum::<usize>(),
        0
    );
}

#[tokio::test]
async fn review_counts_survive_wait_filter_paging_and_closed_columns() {
    let mut records: Vec<_> = (0..35)
        .map(|n| {
            let mut task = waiting(&format!("task-{n}"), "review", "working", n as f64);
            task.review_rounds = Some(n);
            task
        })
        .collect();
    for status in ["done", "cancelled"] {
        let mut closed = task(status.into(), status, 2, 0.0);
        closed.review_rounds = Some(4);
        records.push(closed);
    }
    let first = answer(&records, "wait=working").await;
    assert_eq!(first.tasks.len(), 30);
    assert_eq!(first.tasks[0].review_rounds, Some(0));
    assert_eq!(first.tasks[29].review_rounds, Some(29));
    let cursor = cursor(&first);
    let next = answer(&records, &format!("wait=working&cursor={cursor}")).await;
    assert_eq!(
        next.tasks
            .iter()
            .map(|t| t.review_rounds)
            .collect::<Vec<_>>(),
        (30..35).map(Some).collect::<Vec<_>>()
    );
    for view in ["", "view=waiting&"] {
        for status in ["done", "cancelled"] {
            let list = listing_of(&records, &format!("{view}status={status}&older=1")).await;
            assert_eq!(list.tasks.len(), 1);
            assert_eq!(list.tasks[0].review_rounds, Some(4));
        }
    }
}
