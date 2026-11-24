//! Making tasks, finding them by document, and the tree of subtasks.

use crate::common::{Fixture, NOW, Seed, chain, refused, under};
use symphony_board::health::LIVE_SECONDS;
use symphony_board::{Health, NewTask};

fn jump() -> NewTask {
    NewTask::new("sim/jump", "Jump departure", "owner")
}

fn slugs(tasks: Vec<symphony_board::Task>) -> Vec<String> {
    tasks.into_iter().map(|t| t.slug).collect()
}

#[test]
fn a_task_is_made_on_the_record_and_read_back() {
    let f = Fixture::new(Seed::default());
    let made = f
        .store
        .create(&NewTask {
            body: "Do the thing".into(),
            priority: 1,
            links: vec!["docs/SPEC.md#Jump".into()],
            ..jump()
        })
        .unwrap();
    assert_eq!(f.store.get("sim/jump").unwrap(), made);
    assert_eq!(
        (
            made.title.as_str(),
            made.body.as_str(),
            made.status.as_str(),
            made.priority
        ),
        ("Jump departure", "Do the thing", "ready", 1)
    );
    assert_eq!(
        (made.created_by.as_str(), made.created, made.updated),
        ("owner", NOW, NOW)
    );
    assert_eq!((made.parent, made.instruction), (None, false));
    assert_eq!(made.links, ["docs/SPEC.md#Jump"]);
    assert!(made.depends.is_empty() && made.roles.is_empty());
    let thread = f.store.thread("sim/jump").unwrap();
    let said: Vec<_> = thread
        .iter()
        .map(|m| (m.sender.as_str(), m.kind.as_str(), m.text.as_str(), m.ts))
        .collect();
    assert_eq!(said, [("owner", "system", "created by owner", NOW)]);
}

#[test]
fn a_title_is_trimmed_and_tasks_are_numbered_in_the_order_made() {
    let f = Fixture::new(Seed::default().task("sim/old", "ready"));
    let a = f.task_with(NewTask::new("sim/a", "  Spaced out  ", "owner"));
    let b = f.task("sim/b");
    assert_eq!(a.title, "Spaced out");
    assert!(1 < a.id && a.id < b.id, "{} {}", a.id, b.id);
}

#[test]
fn a_session_makes_a_task_on_the_record_and_is_seen() {
    let f = Fixture::new(Seed::default());
    let h = f.session("orchestrator");
    f.advance(LIVE_SECONDS + 1.0);
    let made = f.task_with(NewTask::new("sim/jump", "Jump departure", &h));
    assert_eq!(made.created_by, h);
    assert_eq!(f.health(&h), Health::Live);
    assert_eq!(f.said("sim/jump"), [format!("created by {h}")]);
    assert_eq!(
        refused(f.store.create(&NewTask::new("sim/b", "B", "ghost-owl"))),
        f.no_session("ghost-owl")
    );
}

#[test]
fn a_title_a_priority_and_a_first_status_are_checked() {
    let f = Fixture::new(Seed::default());
    let refusal = |new: NewTask| refused(f.store.create(&new));
    assert_eq!(
        refusal(NewTask {
            title: "  ".into(),
            ..jump()
        }),
        "a task needs a title"
    );
    for priority in [-1, 5] {
        assert_eq!(
            refusal(NewTask { priority, ..jump() }),
            "priority runs 0 (most urgent) to 4"
        );
    }
    assert_eq!(
        refusal(NewTask {
            status: "done".into(),
            ..jump()
        }),
        "a new task starts as backlog, concept, design, ready, not 'done'"
    );
    for (slug, status) in [
        ("sim/a", "backlog"),
        ("sim/c", "concept"),
        ("sim/d", "design"),
    ] {
        let made = f.task_with(NewTask {
            status: status.into(),
            ..NewTask::new(slug, "A", "owner")
        });
        assert_eq!(made.status, status);
    }
}

#[test]
fn a_bad_slug_is_refused() {
    let f = Fixture::new(Seed::default());
    for slug in [
        "",
        "Sim/Jump",
        "sim jump",
        "/sim",
        "sim/",
        "sim//jump",
        "-sim",
        "sim/-x",
        "sim_x",
    ] {
        assert_eq!(
            refused(f.store.create(&NewTask {
                slug: slug.into(),
                ..jump()
            })),
            format!("slug '{slug}' must look like area/short-name (lowercase, digits, dashes)")
        );
    }
    for slug in ["a", "t0", "a/b/c", "sim/jump-2", "9/x-"] {
        f.task(slug);
    }
}

#[test]
fn a_slug_is_made_once() {
    let f = Fixture::new(Seed::default());
    f.task("sim/jump");
    assert_eq!(
        refused(f.store.create(&jump())),
        "task sim/jump already exists"
    );
}

#[test]
fn a_dependency_a_parent_and_an_assignee_must_exist() {
    let f = Fixture::new(Seed::default().task("sim/first", "ready"));
    assert_eq!(
        refused(f.store.create(&NewTask {
            depends: vec!["sim/first".into(), "sim/nothing".into()],
            ..jump()
        })),
        "no task 'sim/nothing'"
    );
    assert_eq!(
        refused(f.store.create(&under("sim/jump", "sim/nothing"))),
        "no task 'sim/nothing'"
    );
    assert_eq!(
        refused(f.store.create(&NewTask {
            assignee: Some("no-body".into()),
            ..under("sim/jump", "sim/first")
        })),
        "no session 'no-body' to assign it to"
    );
    assert_eq!(f.store.tasks(None).unwrap().len(), 1);
}

#[test]
fn links_and_dependencies_are_kept_once_each() {
    let f = Fixture::new(
        Seed::default()
            .task("sim/first", "done")
            .task("sim/second", "ready"),
    );
    let made = f.task_with(NewTask {
        links: vec!["docs/b.md".into(), "docs/a.md".into(), "docs/a.md".into()],
        depends: vec!["sim/second".into(), "sim/first".into(), "sim/second".into()],
        ..jump()
    });
    assert_eq!(made.links, ["docs/a.md", "docs/b.md"]);
    assert_eq!(made.depends, ["sim/first", "sim/second"]);
}

#[test]
fn a_list_filters_by_status_and_sorts_by_priority() {
    let f = Fixture::new(Seed::default());
    for (slug, priority, status) in [
        ("sim/low", 3, "ready"),
        ("sim/high", 0, "ready"),
        ("sim/later", 2, "backlog"),
    ] {
        f.task_with(NewTask {
            priority,
            status: status.into(),
            ..NewTask::new(slug, "A", "owner")
        });
    }
    assert_eq!(
        slugs(f.store.tasks(Some("ready")).unwrap()),
        ["sim/high", "sim/low"]
    );
    assert_eq!(f.store.tasks(None).unwrap().len(), 3);
}

#[test]
fn linked_to_finds_tasks_by_document_and_heading() {
    let f = Fixture::new(Seed::default());
    for (slug, priority, links) in [
        ("sim/a", 2, vec!["docs/SPEC.md#Jump"]),
        ("sim/b", 2, vec!["docs/SPEC.md"]),
        ("sim/c", 2, vec!["docs/VISION.md", "docs/SPEC.md.bak"]),
        ("sim/d", 0, vec!["docs/SPEC.md#Dock", "docs/SPEC.md"]),
    ] {
        f.task_with(NewTask {
            priority,
            links: links.into_iter().map(String::from).collect(),
            ..NewTask::new(slug, "A", "owner")
        });
    }
    assert_eq!(
        slugs(f.store.linked_to("docs/SPEC.md").unwrap()),
        ["sim/d", "sim/a", "sim/b"]
    );
    assert!(f.store.linked_to("docs/NONE.md").unwrap().is_empty());
}

#[test]
fn a_top_level_task_cannot_be_an_instruction() {
    let f = Fixture::new(Seed::default());
    let instruction = NewTask {
        instruction: true,
        ..NewTask::new("a", "A", "owner")
    };
    assert_eq!(
        refused(f.store.create(&instruction)),
        "an instruction task is a subtask: it needs a parent"
    );
    f.task("a");
    let made = f.task_with(NewTask {
        instruction: true,
        ..under("a/m", "a")
    });
    assert_eq!(
        (made.instruction, made.parent.as_deref()),
        (true, Some("a"))
    );
}

#[test]
fn nesting_goes_deeper_than_any_board() {
    let f = Fixture::new(Seed::default());
    f.task("t0");
    for depth in 1..30 {
        f.task_with(under(&format!("t{depth}"), &format!("t{}", depth - 1)));
    }
    let up: Vec<String> = (0..29).rev().map(|d| format!("t{d}")).collect();
    assert_eq!(f.store.ancestors("t29").unwrap(), up);
    assert_eq!(slugs(f.store.children("t3").unwrap()), ["t4"]);
    assert_eq!(f.store.descendants("t0").unwrap().len(), 29);
    assert!(f.store.ancestors("t0").unwrap().is_empty());
}

#[test]
fn children_and_descendants_come_in_the_order_made() {
    let (f, _) = chain();
    f.task("other");
    f.task_with(under("a/b/c/d", "a/b/c"));
    f.task_with(under("a/e", "a"));
    assert_eq!(slugs(f.store.children("a").unwrap()), ["a/b", "a/e"]);
    assert_eq!(
        slugs(f.store.descendants("a").unwrap()),
        ["a/b", "a/b/c", "a/b/c/d", "a/e"]
    );
    assert!(f.store.descendants("a/b/c/d").unwrap().is_empty());
    assert!(f.store.children("other").unwrap().is_empty());
}

#[test]
fn the_tree_of_an_unknown_task_is_refused() {
    let f = Fixture::new(Seed::default());
    assert_eq!(refused(f.store.ancestors("nothing")), "no task 'nothing'");
    assert_eq!(refused(f.store.children("nothing")), "no task 'nothing'");
    assert_eq!(refused(f.store.descendants("nothing")), "no task 'nothing'");
}

#[test]
fn a_task_cannot_become_its_own_ancestor() {
    let (f, _) = chain();
    assert_eq!(
        refused(f.store.set_parent("a", Some("a/b/c"), "owner")),
        "a cannot go under a/b/c: it would be its own ancestor"
    );
    assert_eq!(
        refused(f.store.set_parent("a", Some("a"), "owner")),
        "a cannot go under a: it would be its own ancestor"
    );
    assert_eq!(f.store.get("a").unwrap().parent, None);
}

#[test]
fn set_parent_moves_a_task_on_the_record_and_needs_the_parent_to_exist() {
    let (f, orch) = chain();
    f.task("elsewhere");
    f.advance(5.0);
    let moved = f
        .store
        .set_parent("a/b/c", Some("elsewhere"), "owner")
        .unwrap();
    assert_eq!(
        (moved.parent.as_deref(), moved.updated),
        (Some("elsewhere"), f.now())
    );
    // Out from under orch's task, it stays orch's: the move gives it in the same write.
    let said = f.said("a/b/c");
    assert_eq!(
        said[said.len() - 2..],
        [
            "owner put it under elsewhere (was a/b)".to_string(),
            format!("{orch} was given orchestrator (a/b/c moved out from under a/b)"),
        ]
    );
    assert_eq!(moved.holders("orchestrator"), [orch.as_str()]);
    f.advance(LIVE_SECONDS + 1.0);
    let top = f.store.set_parent("a/b/c", None, &orch).unwrap();
    assert_eq!(top.parent, None);
    assert_eq!(
        f.said("a/b/c").last().unwrap(),
        &format!("{orch} put it under nothing (was elsewhere)")
    );
    assert_eq!(f.health(&orch), Health::Live);
    assert_eq!(
        refused(f.store.set_parent("a/b/c", Some("nothing"), "owner")),
        "no task 'nothing'"
    );
    assert_eq!(
        refused(f.store.set_parent("nothing", None, "owner")),
        "no task 'nothing'"
    );
    assert_eq!(
        refused(f.store.set_parent("a/b/c", None, "ghost-owl")),
        f.no_session("ghost-owl")
    );
}

#[test]
fn a_move_decided_on_a_tree_that_changed_is_decided_again() {
    let f = Fixture::new(Seed::default());
    f.task("x");
    f.task("y");
    let other = f.other();
    let late = f.late(move || {
        other.set_parent("y", Some("x"), "owner").unwrap();
    });
    assert_eq!(
        refused(late.set_parent("x", Some("y"), "owner")),
        "x cannot go under y: it would be its own ancestor"
    );
    assert_eq!(f.store.ancestors("y").unwrap(), ["x"]);
    assert!(f.store.ancestors("x").unwrap().is_empty());
}
