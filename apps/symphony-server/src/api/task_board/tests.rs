use super::*;
use serde_json::json;
use std::collections::BTreeMap;
use symphony_api::{MachineWork, TaskListing, Tasks};

fn fixture() -> (Tasks, MachineWork) {
    let mut records = Vec::new();
    let mut operations = Vec::new();
    for (status, kind, count) in [
        ("in_progress", "build", 37),
        ("in_progress", "test", 3),
        ("in_progress", "command", 2),
        ("review", "build", 1),
        ("done", "test", 1),
    ] {
        for index in 0..count {
            let slug = format!("{status}-{kind}-{index:02}");
            records.push(
                serde_json::from_value(json!({"slug":slug,"title":slug,"body":"brief",
                "status":status,"priority":1,"parent":null,"created_by":"owner","created":0.0,
                "updated":0.0,"links":[],"depends":[],"roles":[],"instruction":false}))
                .unwrap(),
            );
            operations.push(
                json!({"machine_id":"alpha","operation_id":slug,"parent_id":null,
                "handle":"worker","task":slug,"checkout":"opaque","recipe":"fixture",
                "kind":kind,"state":"running","resource":"native","reason":"","phase_since":null}),
            );
        }
    }
    let work = serde_json::from_value(json!({"freshness":"fresh","machines":[{
        "machine_id":"alpha","label":"Alpha","producer_id":"fixture","sequence":1,
        "age_ms":0,"freshness":"fresh","complete":true,"reason":"","counts":null,
        "operations":operations}],"unassigned":[]}))
    .unwrap();
    (
        Tasks {
            version: String::new(),
            tasks: records,
            sessions: vec![],
            orchestrators: vec![],
            unstarted: 0,
        },
        work,
    )
}

async fn page(query: &str, search: &str, scores: &BTreeMap<String, f64>) -> TaskListing {
    let (tasks, work) = fixture();
    let request = Request::builder()
        .uri(format!("/api/task-board?{query}"))
        .body(())
        .unwrap();
    let response = project(
        &request,
        tasks,
        search,
        scores,
        work,
        vec!["review-build-00".into()],
        None,
    );
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

fn total(page: &TaskListing, stage: &str) -> usize {
    page.counts
        .iter()
        .find(|(s, _)| s == stage)
        .map_or(0, |(_, n)| *n)
}

#[tokio::test]
async fn build_pages_count_the_complete_window_before_paging_and_keep_lifecycle_unchanged() {
    let scores = BTreeMap::new();
    let first = page("status=build&activity=1", "", &scores).await;
    assert_eq!(first.tasks.len(), 30);
    assert_eq!(total(&first, "build"), 37);
    assert_eq!(total(&first, "test"), 3);
    assert_eq!(total(&first, "in_progress"), 2);
    assert_eq!(total(&first, "review"), 1);
    assert_eq!(
        total(&first, "done"),
        0,
        "old finished task is outside the window"
    );
    assert!(
        first
            .tasks
            .iter()
            .all(|task| task.status == "in_progress" && task.body.is_empty())
    );
    assert_eq!(
        first
            .lifecycle_counts
            .iter()
            .find(|(s, _)| s == "in_progress")
            .unwrap()
            .1,
        42
    );
    let cursor: String = first
        .next
        .as_ref()
        .unwrap()
        .bytes()
        .map(|byte| format!("%{byte:02X}"))
        .collect();
    let next = page(
        &format!("status=build&activity=1&cursor={cursor}"),
        "",
        &scores,
    )
    .await;
    assert_eq!(next.tasks.len(), 7);
    assert_eq!(total(&next, "build"), 37);
    assert_eq!(next.tasks[0].slug, "in_progress-build-30");
    let counts = page("counts=1&activity=1", "", &scores).await;
    assert!(counts.tasks.is_empty());
    assert_eq!(total(&counts, "build"), 37);
    let offline = page("status=in_progress&activity=0&all=1", "", &scores).await;
    assert_eq!(offline.tasks.len(), 42);
    assert_eq!(total(&offline, "build"), 0);
    assert_eq!(total(&offline, "in_progress"), 42);
    let legacy = page("status=in_progress&all=1", "", &scores).await;
    assert_eq!(legacy.tasks.len(), 42);
    assert_eq!(total(&legacy, "in_progress"), 42);
    assert!(!legacy.statuses.contains(&"build".to_string()));
    assert_eq!(
        page("status=done&older=1", "", &scores).await.tasks.len(),
        1
    );
    let signoff = page("status=signoff", "", &scores).await;
    assert_eq!(signoff.tasks[0].slug, "review-build-00");
    assert_eq!(signoff.tasks[0].status, "review");
}

/// The cursor of `page`, escaped for its query.
fn cursor(page: &TaskListing) -> String {
    page.next
        .as_ref()
        .unwrap()
        .bytes()
        .map(|byte| format!("%{byte:02X}"))
        .collect()
}

/// A task of `status` and `priority` last changed at `updated`, its slug its title.
fn task(slug: String, status: &str, priority: i64, updated: f64) -> symphony_api::Task {
    serde_json::from_value(json!({"slug":slug,"title":slug,"body":"","status":status,
        "priority":priority,"parent":null,"created_by":"owner","created":0.0,
        "updated":updated,"links":[],"depends":[],"roles":[],"instruction":false}))
    .unwrap()
}

/// The listing `query` reads of `records`, with no search, machine work or sign-off.
async fn listing_of(records: &[symphony_api::Task], query: &str) -> TaskListing {
    let request = Request::builder()
        .uri(format!("/api/task-board?{query}"))
        .body(())
        .unwrap();
    let tasks = Tasks {
        version: String::new(),
        tasks: records.to_vec(),
        sessions: vec![],
        orchestrators: vec![],
        unstarted: 0,
    };
    let response = project(
        &request,
        tasks,
        "",
        &BTreeMap::new(),
        MachineWork::default(),
        vec![],
        None,
    );
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

fn slugs(page: &TaskListing) -> Vec<String> {
    page.tasks.iter().map(|t| t.slug.clone()).collect()
}

/// A column's slugs order by their numbers (`ready-3` before `ready-30`), on its first page and
/// across the cursor to the next, which starts where the first ended and repeats none of it.
#[tokio::test]
async fn a_columns_numbered_slugs_page_in_number_order() {
    let mut records: Vec<symphony_api::Task> = (1..=35)
        .rev()
        .map(|n| task(format!("bulk/ready-{n}"), "ready", 3, 0.0))
        .collect();
    records.push(task("app/urgent".into(), "ready", 1, 0.0));
    records.extend((1..=33).map(|n| task(format!("old/done-{n}"), "done", 2, 5.0)));
    let read = |query: String| {
        let records = &records;
        async move { listing_of(records, &query).await }
    };
    let first = read("status=ready".into()).await;
    let next = read(format!("status=ready&cursor={}", cursor(&first))).await;
    let mut ready = vec!["app/urgent".to_string()];
    ready.extend((1..=35).map(|n| format!("bulk/ready-{n}")));
    assert_eq!(slugs(&first), ready[..30]);
    assert_eq!(slugs(&next), ready[30..]);
    // Finished work of one moment orders the same way, newest first.
    let first = read("status=done&older=1".into()).await;
    let next = read(format!("status=done&older=1&cursor={}", cursor(&first))).await;
    let done: Vec<_> = (1..=33).map(|n| format!("old/done-{n}")).collect();
    assert_eq!(slugs(&first), done[..30]);
    assert_eq!(slugs(&next), done[30..]);
}

/// `rows` makes a column's first page as long as it asks, never shorter than a page, and its
/// cursor goes on after the last of them: the board's refresh reads a column's loaded pages again
/// as one. A number it cannot read, or none, is a page.
#[tokio::test]
async fn rows_reads_a_page_as_long_as_it_asks_and_never_shorter_than_one() {
    let records: Vec<symphony_api::Task> = (1..=70)
        .map(|n| task(format!("bulk/ready-{n}"), "ready", 2, 0.0))
        .chain((1..=3).map(|n| task(format!("bulk/review-{n}"), "review", 2, 0.0)))
        .collect();
    let ready: Vec<_> = (1..=70).map(|n| format!("bulk/ready-{n}")).collect();
    let long = listing_of(&records, "status=ready&rows=45").await;
    assert_eq!(slugs(&long), ready[..45]);
    assert_eq!(total(&long, "ready"), 70);
    assert_eq!(total(&long, "review"), 3);
    let after = listing_of(&records, &format!("status=ready&cursor={}", cursor(&long))).await;
    assert_eq!(slugs(&after), ready[45..70]);
    for asked in ["rows=10", "rows=0", "rows=-5", "rows=many", "rows=", ""] {
        let page = listing_of(&records, &format!("status=ready&{asked}")).await;
        assert_eq!(slugs(&page), ready[..30], "{asked}");
    }
    let past_the_end = listing_of(&records, "status=ready&rows=500").await;
    assert_eq!(slugs(&past_the_end), ready);
    // However large, `rows` is bounded by the column, as `all=1` is: from its start, from an
    // offset (whose end saturates) and after a cursor. A number past the largest a `usize` holds
    // is none it can read: a page.
    let all = listing_of(&records, "status=ready&all=1").await;
    assert_eq!(slugs(&all), ready);
    let most = usize::MAX;
    let largest = listing_of(&records, &format!("status=ready&rows={most}")).await;
    assert_eq!(slugs(&largest), slugs(&all));
    assert_eq!(largest.next, all.next);
    assert_eq!(total(&largest, "ready"), 70);
    let from_offset = listing_of(&records, &format!("status=ready&offset=60&rows={most}")).await;
    assert_eq!(slugs(&from_offset), ready[60..]);
    let first = listing_of(&records, "status=ready").await;
    let after_cursor = listing_of(
        &records,
        &format!("status=ready&cursor={}&rows={most}", cursor(&first)),
    )
    .await;
    assert_eq!(slugs(&after_cursor), ready[30..]);
    let unreadable = listing_of(&records, &format!("status=ready&rows={most}0")).await;
    assert_eq!(slugs(&unreadable), ready[..30]);
    // A page further on is as long as `rows` too, by an offset as by a cursor.
    let offset = listing_of(&records, "status=ready&offset=10&rows=40").await;
    assert_eq!(slugs(&offset), ready[10..50]);
    let further = listing_of(
        &records,
        &format!("status=ready&cursor={}&rows=35", cursor(&first)),
    )
    .await;
    assert_eq!(slugs(&further), ready[30..65]);
}

#[tokio::test]
async fn search_filter_totals_are_full_and_stable_across_build_pages() {
    let scores: BTreeMap<_, _> = (0..35)
        .map(|index| (format!("in_progress-build-{index:02}"), 1.0))
        .collect();
    let first = page("status=build&activity=1", "build", &scores).await;
    assert_eq!(total(&first, "build"), 35);
    assert_eq!(total(&first, "test"), 0);
    assert!(first.signoff.is_empty());
    let cursor: String = first
        .next
        .as_ref()
        .unwrap()
        .bytes()
        .map(|byte| format!("%{byte:02X}"))
        .collect();
    let next = page(
        &format!("status=build&activity=1&cursor={cursor}"),
        "build",
        &scores,
    )
    .await;
    assert_eq!(next.tasks.len(), 5);
    assert_eq!(next.tasks[0].slug, "in_progress-build-30");
    assert_eq!(total(&next, "build"), 35);
}

#[tokio::test]
async fn finished_window_is_applied_before_stage_counts_including_searches() {
    let (mut tasks, work) = fixture();
    let finished = tasks
        .tasks
        .iter_mut()
        .find(|task| task.status == "done")
        .unwrap();
    finished.updated = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
        - 2.0 * 86_400.0;
    let scores = BTreeMap::from([(finished.slug.clone(), 1.0)]);
    for (days, expected) in [("1", 0), ("7", 1), ("invalid", 0)] {
        let request = Request::builder()
            .uri(format!("/api/task-board?counts=1&activity=1&days={days}"))
            .body(())
            .unwrap();
        let response = project(
            &request,
            tasks.clone(),
            "done",
            &scores,
            work.clone(),
            vec![],
            None,
        );
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let listing: TaskListing = serde_json::from_slice(&bytes).unwrap();
        assert!(listing.tasks.is_empty());
        assert_eq!(total(&listing, "done"), expected, "days={days}");
        assert_eq!(total(&listing, "build"), 0, "search must precede counting");
    }
}

/// A task's subtasks (`parent=`, its page's Subtasks and its branch of the task groups) list its
/// slices, its errands and an open instruction, never a finished instruction to do or review it;
/// the task groups (`roots=`) hold no task whose subtasks are all such.
#[tokio::test]
async fn subtasks_and_task_groups_leave_out_finished_instructions_to_staff_their_task() {
    let task = |slug: &str, status: &str, parent: Option<&str>, instruction: bool| {
        serde_json::from_value(
            json!({"slug":slug,"title":slug,"body":"brief","status":status,
            "priority":2,"parent":parent,"created_by":"owner","created":0.0,"updated":0.0,
            "links":[],"depends":[],"roles":[],"instruction":instruction}),
        )
        .unwrap()
    };
    let tasks = Tasks {
        version: String::new(),
        tasks: vec![
            task("feature", "in_progress", None, false),
            task("feature/slice", "done", Some("feature"), false),
            task("feature/visual", "done", Some("feature"), true),
            task("feature/work", "in_progress", Some("feature"), true),
            task("feature/review", "done", Some("feature"), true),
            task("feature/staff-review-1", "cancelled", Some("feature"), true),
            task("staffed", "done", None, false),
            task("staffed/work", "done", Some("staffed"), true),
            task("staffed/review", "cancelled", Some("staffed"), true),
            task("in-flight", "ready", None, false),
            task("in-flight/work", "ready", Some("in-flight"), true),
            task("alone", "ready", None, false),
        ],
        sessions: vec![],
        orchestrators: vec![],
        unstarted: 0,
    };
    let read = |query: &str| {
        let request = Request::builder()
            .uri(format!("/api/task-board?{query}&older=1"))
            .body(())
            .unwrap();
        let response = project(
            &request,
            tasks.clone(),
            "",
            &BTreeMap::new(),
            MachineWork::default(),
            vec![],
            None,
        );
        async move {
            let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let listing: TaskListing = serde_json::from_slice(&bytes).unwrap();
            let total: usize = listing.counts.iter().map(|(_, n)| n).sum();
            let slugs: Vec<_> = listing.tasks.into_iter().map(|t| t.slug).collect();
            (slugs, total)
        }
    };
    assert_eq!(
        read("parent=feature").await,
        (
            vec![
                "feature/slice".to_string(),
                "feature/visual".into(),
                "feature/work".into()
            ],
            3
        )
    );
    assert_eq!(read("parent=staffed").await, (vec![], 0));
    assert_eq!(
        read("parent=in-flight").await,
        (vec!["in-flight/work".to_string()], 1)
    );
    assert_eq!(
        read("roots=1").await,
        (vec!["feature".to_string(), "in-flight".into()], 2)
    );
    // The board's columns list top-level tasks, a staffed one among them, as before.
    assert_eq!(read("status=done").await.0, vec!["staffed".to_string()]);
}

/// `work=0` leaves the machine's work out of a listing, which a page then reads from
/// `/api/machine-work`, and nothing else: the stages and counts still place each task by it, so a
/// listing's validator moves with its tasks and not with a recorder's heartbeat.
#[tokio::test]
async fn a_listing_without_work_places_tasks_by_it_and_leaves_it_out() {
    let scores = BTreeMap::new();
    for query in [
        "counts=1&activity=1",
        "status=build&activity=1",
        "status=review",
    ] {
        let with = page(query, "", &scores).await;
        let without = page(&format!("{query}&work=0"), "", &scores).await;
        assert!(!with.machine_work.machines.is_empty(), "{query}");
        assert_eq!(without.machine_work, MachineWork::default(), "{query}");
        assert_eq!(
            TaskListing {
                machine_work: MachineWork::default(),
                ..with
            },
            without,
            "{query}"
        );
    }
    // Only `work=0` leaves it out.
    for query in ["work=1", "work=", "work=false"] {
        let listing = page(&format!("counts=1&activity=1&{query}"), "", &scores).await;
        assert!(!listing.machine_work.machines.is_empty(), "{query}");
    }
    let built = page("counts=1&activity=1&work=0", "", &scores).await;
    assert_eq!(total(&built, "build"), 37);
}

mod waiting;

mod workflow;

fn key(stage: &str, priority: i64, at: f64, slug: &str) -> Key {
    Key(stage.into(), priority, at, slug.into())
}

/// A search lists the best match first whatever its stage or priority, and equal scores by slug.
#[test]
fn a_search_orders_by_score_then_slug() {
    let best = key("ready", 4, 0.9, "z/last");
    let worse = key("backlog", 0, 0.2, "a/first");
    assert_eq!(best.compare(&worse, Order::Match), Ordering::Less);
    assert_eq!(worse.compare(&best, Order::Match), Ordering::Greater);
    let tied = key("done", 1, 0.9, "z/last-2");
    assert_eq!(best.compare(&tied, Order::Match), Ordering::Less);
    assert_eq!(best.compare(&best, Order::Match), Ordering::Equal);
}

/// A task's subtasks, the groups and a holder's tasks order by priority alone, then by slug: their
/// stage and their time decide nothing.
#[test]
fn a_listing_by_priority_passes_over_stage_and_time() {
    let urgent = key("review", 0, 1.0, "b/urgent");
    let later = key("backlog", 2, 9.0, "a/later");
    assert_eq!(urgent.compare(&later, Order::Priority), Ordering::Less);
    assert_eq!(later.compare(&urgent, Order::Priority), Ordering::Greater);
    let same = key("done", 0, 5.0, "b/urgent-10");
    assert_eq!(urgent.compare(&same, Order::Priority), Ordering::Less);
    let numbered = key("done", 0, 5.0, "b/urgent-9");
    assert_eq!(numbered.compare(&same, Order::Priority), Ordering::Less);
}

/// The board orders by stage first; a finished stage's rows newest first, their priority deciding
/// nothing, and every other stage's by priority, their time deciding nothing.
#[test]
fn the_board_orders_a_finished_stage_newest_first_and_any_other_by_priority() {
    let stage = key("blocked", 4, 0.0, "z");
    let after = key("ready", 0, 9.0, "a");
    assert_eq!(stage.compare(&after, Order::Stage), Ordering::Less);
    for finished in FINISHED {
        let newer = key(finished, 4, 9.0, "z/newer");
        let older = key(finished, 0, 1.0, "a/older");
        assert_eq!(newer.compare(&older, Order::Stage), Ordering::Less);
        let beside = key(finished, 2, 9.0, "z/newer-2");
        assert_eq!(newer.compare(&beside, Order::Stage), Ordering::Less);
    }
    for open in [
        "backlog",
        "ready",
        "in_progress",
        "review",
        "blocked",
        "build",
        "test",
    ] {
        let urgent = key(open, 0, 1.0, "z/urgent");
        let later = key(open, 3, 9.0, "a/later");
        assert_eq!(urgent.compare(&later, Order::Stage), Ordering::Less);
        let beside = key(open, 0, 9.0, "z/urgent-2");
        assert_eq!(urgent.compare(&beside, Order::Stage), Ordering::Less);
    }
}

/// A cursor is the four values a page's last row is keyed by, written and read as one array, and
/// one that is not is refused in the words the four values' own reader gives.
#[test]
fn a_cursor_is_written_and_read_as_its_four_values() {
    let written = serde_json::to_string(&key("ready", 2, 10.5, "app/a")).unwrap();
    assert_eq!(written, r#"["ready",2,10.5,"app/a"]"#);
    let read: Key = serde_json::from_str(&written).unwrap();
    assert_eq!(
        read.compare(&key("ready", 2, 10.5, "app/a"), Order::Stage),
        Ordering::Equal
    );
    for malformed in [
        r#"["ready",2]"#,
        r#"["ready","2",1.0,"a"]"#,
        r#"{"stage":"ready"}"#,
        "",
    ] {
        assert_eq!(
            serde_json::from_str::<Key>(malformed)
                .unwrap_err()
                .to_string(),
            serde_json::from_str::<(String, i64, f64, String)>(malformed)
                .unwrap_err()
                .to_string()
        );
    }
}

/// A task's subtasks page by priority: the second page starts where the first ended, repeating
/// and skipping none, whatever stage each is at.
#[tokio::test]
async fn a_tasks_subtasks_page_by_priority_across_the_cursor() {
    let mut records = vec![task("app/parent".into(), "in_progress", 2, 0.0)];
    let statuses = ["ready", "in_progress", "review", "blocked"];
    records.extend((1..=40).map(|n: usize| {
        let mut child = task(
            format!("app/parent/part-{n}"),
            statuses[n % statuses.len()],
            i64::try_from(n % 3).unwrap(),
            0.0,
        );
        child.parent = Some("app/parent".into());
        child
    }));
    let first = listing_of(&records, "parent=app/parent").await;
    let next = listing_of(
        &records,
        &format!("parent=app/parent&cursor={}", cursor(&first)),
    )
    .await;
    let mut expected: Vec<(usize, usize)> = (1..=40).map(|n| (n % 3, n)).collect();
    expected.sort_unstable();
    let expected: Vec<String> = expected
        .into_iter()
        .map(|(_, n)| format!("app/parent/part-{n}"))
        .collect();
    assert_eq!(slugs(&first), expected[..30]);
    assert_eq!(slugs(&next), expected[30..]);
}
