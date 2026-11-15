//! Task counts and brief rendering retain their task-page boundaries.

use super::*;
use crate::test_tree::Tree;

#[test]
fn project_task_counts_exclude_children_and_grandchildren() {
    let task = |slug: &str, status: &str, parent: Option<&str>| Task {
        slug: slug.into(),
        title: slug.into(),
        body: String::new(),
        status: status.into(),
        priority: 2,
        parent: parent.map(String::from),
        created_by: "owner".into(),
        created: 0.0,
        updated: 0.0,
        links: Vec::new(),
        depends: Vec::new(),
        roles: Vec::new(),
        instruction: false,
        fields: BTreeMap::new(),
    };
    let board = Tasks {
        version: "fixture".into(),
        sessions: Vec::new(),
        tasks: vec![
            task("feature", "in_progress", None),
            task("reviewed", "review", None),
            task("finished", "done", None),
            task("feature/implementation", "in_progress", Some("feature")),
            task("feature/check", "review", Some("feature")),
            task("feature/check/detail", "review", Some("feature/check")),
        ],
    };
    assert_eq!(
        counts(&board),
        TaskCounts {
            in_progress: 1,
            in_review: 1,
            agents_live: 0
        }
    );
}

/// Under its parent, a finished instruction to do or review the parent is no row of its own; a
/// slice, an errand and any open subtask are.
#[test]
fn only_a_finished_instruction_to_work_on_or_review_its_parent_is_left_out_under_it() {
    let staffing = [
        "feature/work",
        "feature/review",
        "feature/staff-review-1",
        "feature/staff-review-12",
        "feature/part/work",
        "work",
    ];
    for slug in staffing {
        for finished in ["done", "cancelled"] {
            assert!(
                !listed_under_parent(slug, true, finished),
                "{slug} {finished}"
            );
        }
        for open in [
            "concept",
            "design",
            "backlog",
            "ready",
            "in_progress",
            "review",
            "blocked",
        ] {
            assert!(listed_under_parent(slug, true, open), "{slug} {open}");
        }
        // A slice of that name is work on the parent's branch, listed whatever its status.
        assert!(listed_under_parent(slug, false, "done"), "slice {slug}");
    }
    let errands = [
        "feature/visual",
        "feature/merge-main",
        "feature/review-merge",
        "feature/rework",
        "feature/work-2",
        "feature/reviews",
        "feature/staff-review-",
        "feature/staff-review-x",
        "feature/staff-review-2b",
        "feature/work/notes",
    ];
    for slug in errands {
        for status in ["done", "cancelled", "ready"] {
            assert!(listed_under_parent(slug, true, status), "{slug} {status}");
        }
    }
}

#[test]
fn a_brief_within_the_limit_is_its_whole_reply() {
    let tree = Tree::new();
    let render = Render::new(&tree.root);
    for body in [
        "One paragraph.",
        "# A heading\n\nA paragraph, and:\n\n- a list\n",
    ] {
        assert_eq!(
            brief_part(&render, body, 0, BRIEF_LIMIT),
            (reply_blocks(&render, body), body.len())
        );
    }
}
