use super::tasks;
use symphony_api::{Role, Task};

fn task(slug: &str, status: &str, updated: f64, roles: &[(&str, &str)]) -> Task {
    Task {
        slug: slug.into(),
        title: format!("The {slug} task"),
        body: format!("The brief of {slug}."),
        status: status.into(),
        priority: 0,
        parent: None,
        created_by: "owner".into(),
        created: 0.0,
        updated,
        links: Vec::new(),
        depends: Vec::new(),
        roles: roles
            .iter()
            .map(|(handle, role)| Role {
                handle: (*handle).into(),
                role: (*role).into(),
                since: 0.0,
                expires: None,
                health: "live".into(),
            })
            .collect(),
        instruction: false,
        fields: Default::default(),
        waits_on: None,
        review_rounds: None,
        from: None,
        workflow: None,
    }
}

fn slugs(shown: &[Task]) -> Vec<&str> {
    shown.iter().map(|task| task.slug.as_str()).collect()
}

#[test]
fn an_agent_shows_its_five_last_finished_tasks_in_the_boards_order_without_briefs() {
    let mut board: Vec<Task> = (0..7)
        .map(|n| {
            let status = if n % 2 == 0 { "done" } else { "cancelled" };
            task(
                &format!("old/{n}"),
                status,
                f64::from(n),
                &[("amber", "reviewer")],
            )
        })
        .collect();
    // The board's order is not the order they finished in.
    board.reverse();
    let shown = tasks(&board, ["amber"]);
    assert_eq!(
        slugs(&shown),
        ["old/6", "old/5", "old/4", "old/3", "old/2"],
        "the five updated last, as the board lists them"
    );
    assert!(shown.iter().all(|task| task.body.is_empty()));
    assert_eq!(
        shown[0].title, "The old/6 task",
        "the rest of the task is kept"
    );
}

#[test]
fn open_work_is_shown_in_every_role_but_orchestrator_and_finished_work_in_any() {
    let board = [
        task("open/execute", "in_progress", 1.0, &[("amber", "executor")]),
        task("open/review", "review", 1.0, &[("amber", "reviewer")]),
        task("open/reserved", "ready", 1.0, &[("amber", "reserved")]),
        task(
            "open/orchestrate",
            "blocked",
            1.0,
            &[("amber", "orchestrator")],
        ),
        task(
            "done/orchestrated",
            "done",
            1.0,
            &[("amber", "orchestrator")],
        ),
        task("open/other", "in_progress", 1.0, &[("brisk", "executor")]),
        task("done/other", "done", 1.0, &[("brisk", "executor")]),
    ];
    assert_eq!(
        slugs(&tasks(&board, ["amber"])),
        [
            "open/execute",
            "open/review",
            "open/reserved",
            "done/orchestrated"
        ]
    );
    assert_eq!(
        slugs(&tasks(&board, ["amber", "brisk"])),
        [
            "open/execute",
            "open/review",
            "open/reserved",
            "done/orchestrated",
            "open/other",
            "done/other"
        ],
        "a task shown for two agents is listed once"
    );
    assert!(tasks(&board, []).is_empty(), "no agent, no task");
    assert!(tasks(&[], ["amber"]).is_empty());
}

#[test]
fn every_agent_keeps_five_finished_tasks_of_its_own_whatever_the_others_finished() {
    let mut board: Vec<Task> = (0..6)
        .map(|n| {
            task(
                &format!("amber/{n}"),
                "done",
                f64::from(n),
                &[("amber", "executor")],
            )
        })
        .collect();
    board.extend((0..6).map(|n| {
        task(
            &format!("brisk/{n}"),
            "done",
            f64::from(10 + n),
            &[("brisk", "executor")],
        )
    }));
    let shown = tasks(&board, ["amber", "brisk"]);
    assert_eq!(
        slugs(&shown),
        [
            "amber/1", "amber/2", "amber/3", "amber/4", "amber/5", "brisk/1", "brisk/2", "brisk/3",
            "brisk/4", "brisk/5"
        ]
    );
}
