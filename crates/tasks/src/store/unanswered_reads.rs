//! The lookup's reads on owned scratch boards: what each mode reads and answers.

use super::*;
use crate::NewTask;

/// A board of `work` in progress under `worker`, its subtask `work-child`, an unrelated task and a
/// post to the worker on `work`; and that post.
fn board() -> (Fixture, String, Message) {
    let f = Fixture::new(Seed::default());
    f.task("work");
    f.task("unrelated");
    f.task_with(NewTask {
        parent: Some("work".into()),
        ..NewTask::new("work-child", "child", "owner")
    });
    let worker = f.session("worker");
    f.store.claim("work", &worker).unwrap();
    let post = f
        .store
        .say("work", "owner", "continue", Some(&worker))
        .unwrap();
    (f, worker, post)
}

#[test]
fn the_reads_answer_only_the_asked_threads_handles_and_answers() {
    let (f, worker, post) = board();
    let other = f.session("worker");
    f.store
        .say("unrelated", &worker, "a word elsewhere", None)
        .unwrap();
    f.store
        .say("unrelated", "owner", "to someone else", Some(&other))
        .unwrap();
    let answer = f
        .store
        .say("work-child", &worker, "done below", None)
        .unwrap();
    f.store.say("work-child", "owner", "a word", None).unwrap();
    let named: Vec<Vec<String>> = f
        .store
        .bounded_read(
            Statement::from(pg::naming()),
            json!({"handles": [&worker, "absent"]}),
        )
        .map_err(recovery_error)
        .unwrap();
    // Its claim on work is a system record it made; the post on work addresses it.
    assert_eq!(named, [vec!["work".to_string()], vec![]]);
    let read: Threads = f
        .store
        .bounded_read(
            Statement::from(pg::threads()),
            json!({"slugs": ["work", "absent"], "launched": ["work", "work-child"]}),
        )
        .map_err(recovery_error)
        .unwrap();
    assert_eq!(read.threads.len(), 2);
    assert!(read.threads[1].is_empty());
    assert!(
        (read.threads[0].iter()).all(|row| row.slug == "work"),
        "{:?}",
        read.threads
    );
    assert!(read.threads[0].iter().any(|row| row.id == post.id));
    assert!(read.launches.is_empty());
    let found: Vec<Vec<i64>> = f
        .store
        .bounded_read(
            Statement::from(pg::below()),
            json!({"asks": [
                {"handle": &worker, "after": post.id, "below": ["work-child"]},
                {"handle": &worker, "after": answer.id, "below": ["work-child"]},
                {"handle": "owner", "after": 0, "below": ["absent"]},
            ]}),
        )
        .map_err(recovery_error)
        .unwrap();
    assert_eq!(found, [vec![answer.id], vec![], vec![]]);
}

/// A recorded launch of `handle` on `slug` as `role`.
fn launch(f: &Fixture, slug: &str, handle: &str, role: &str) {
    let row = crate::rows::TaskSessionRow {
        slug: slug.into(),
        handle: handle.into(),
        role: role.into(),
        claude_session: format!("conversation-{slug}"),
        pane: format!("w1:{slug}"),
        started: f.now(),
        ..Default::default()
    };
    f.store
        .raw()
        .row("task_session", json!([slug, handle]))
        .create(serde_json::to_value(row).unwrap())
        .unwrap();
}

#[test]
fn launches_on_the_task_and_its_instructions_are_read_by_range() {
    let (f, worker, _) = board();
    f.task_with(NewTask {
        parent: Some("work".into()),
        instruction: true,
        ..NewTask::new("work-review", "review it", "owner")
    });
    let reviewer = f.session("reviewer");
    launch(&f, "work", &worker, "executor");
    launch(&f, "work-review", &reviewer, "reviewer");
    launch(&f, "unrelated", &reviewer, "reviewer");
    let read: Threads = f
        .store
        .bounded_read(
            Statement::from(pg::threads()),
            json!({"slugs": ["work"], "launched": ["work", "work-review"]}),
        )
        .map_err(recovery_error)
        .unwrap();
    let mut launched: Vec<_> = (read.launches.iter())
        .map(|l| (l.slug.as_str(), l.handle.as_str(), l.role.as_str()))
        .collect();
    crate::order::sort_by(&mut launched, &Ord::cmp);
    assert_eq!(
        launched,
        [
            ("work", worker.as_str(), "executor"),
            ("work-review", reviewer.as_str(), "reviewer")
        ]
    );
}

/// Every post `handles` owe, as the reap asks.
fn owing(f: &Fixture, handles: &[&str]) -> Vec<(String, String, i64)> {
    let board = f.store.board().unwrap();
    let asked: BTreeSet<String> = handles.iter().map(|h| h.to_string()).collect();
    let mut owed: Vec<_> = (f
        .store
        .pending_posts(&board, Asked::Handles(&asked))
        .unwrap())
    .into_iter()
    .map(|p| (p.handle, p.slug, p.post.id))
    .collect();
    crate::order::sort_by(&mut owed, &Ord::cmp);
    owed
}

#[test]
fn a_handle_owes_the_posts_of_the_threads_that_name_it_and_of_no_other_handle() {
    let (f, worker, post) = board();
    let other = f.session("worker");
    // A finished task keeps its direct post; a role post goes to its holder by its claim.
    f.task("finished");
    f.store
        .raw()
        .row("task", "finished")
        .set(json!({"status": "done"}))
        .unwrap();
    let finished = f
        .store
        .say("finished", "owner", "one more thing", Some(&worker))
        .unwrap();
    let role = f
        .store
        .say("work", "owner", "and this", Some("@executor"))
        .unwrap();
    let elsewhere = f
        .store
        .say("unrelated", "owner", "for the other", Some(&other))
        .unwrap();
    let mut expected = vec![
        (worker.clone(), "finished".to_string(), finished.id),
        (worker.clone(), "work".to_string(), post.id),
        (worker.clone(), "work".to_string(), role.id),
    ];
    crate::order::sort_by(&mut expected, &Ord::cmp);
    assert_eq!(owing(&f, &[&worker]), expected);
    assert_eq!(
        owing(&f, &[&other]),
        [(other.clone(), "unrelated".into(), elsewhere.id)]
    );
    assert_eq!(owing(&f, &["absent"]), []);
    // Its answer below the task answers the posts on it before the answer, not one after it.
    f.store
        .say("work-child", &worker, "done below", None)
        .unwrap();
    let later = f
        .store
        .say("work", "owner", "after that", Some(&worker))
        .unwrap();
    assert_eq!(
        owing(&f, &[&worker]),
        [
            (worker.clone(), "finished".to_string(), finished.id),
            (worker.clone(), "work".to_string(), later.id),
        ]
    );
}

#[test]
fn a_released_holder_still_owes_the_role_posts_its_history_addresses() {
    let (f, worker, post) = board();
    f.store.say("work", &worker, "on it", None).unwrap();
    f.store
        .raw()
        .table("role")
        .matching(json!({"slug": "work", "handle": worker}))
        .delete()
        .unwrap();
    let role = f
        .store
        .say("work", "owner", "still there?", Some("@executor"))
        .unwrap();
    assert!(role.id > post.id);
    assert_eq!(
        owing(&f, &[&worker]),
        [(worker.clone(), "work".to_string(), role.id)]
    );
}

#[test]
fn a_reviewer_with_no_launch_on_its_task_takes_its_latest_reviewer_launch() {
    let (f, worker, _) = board();
    f.store.move_to("work", "review", &worker).unwrap();
    let reviewer = f.session("reviewer");
    f.store.take_role("work", &reviewer, "reviewer").unwrap();
    launch(&f, "unrelated", &reviewer, "reviewer");
    let post = f
        .store
        .say("work", &worker, "please look", Some("@reviewer"))
        .unwrap();
    let board = f.store.board().unwrap();
    let asked = BTreeSet::from([reviewer.clone()]);
    let pending = f
        .store
        .pending_posts(&board, Asked::Handles(&asked))
        .unwrap();
    assert_eq!(pending.len(), 1, "{pending:?}");
    assert_eq!(pending[0].post.id, post.id);
    assert_eq!(pending[0].role, "reviewer");
    let launch = pending[0].launch.as_ref().expect("its launch elsewhere");
    assert_eq!(
        (launch.slug.as_str(), launch.role.as_str()),
        ("unrelated", "reviewer")
    );
}

#[test]
fn overdue_asks_only_open_tasks_and_posts_half_an_hour_old_of_a_gone_agent() {
    let (f, worker, post) = board();
    let board = f.store.board().unwrap();
    let now = f.now();
    let overdue = |board: &Board, now: f64| {
        (f.store
            .pending_posts(board, Asked::Overdue(&board.tasks, now))
            .unwrap())
        .into_iter()
        .map(|p| (p.handle, p.post.id))
        .collect::<Vec<_>>()
    };
    assert_eq!(overdue(&board, now), [], "a fresh post");
    f.advance(UNANSWERED_SECONDS + 60.0);
    let board = f.store.board().unwrap();
    assert_eq!(overdue(&board, f.now()), [(worker.clone(), post.id)]);
    // Nothing of a finished task is read, though its post was never answered.
    f.store
        .raw()
        .row("task", "work")
        .set(json!({"status": "done"}))
        .unwrap();
    let board = f.store.board().unwrap();
    assert_eq!(overdue(&board, f.now()), []);
}
