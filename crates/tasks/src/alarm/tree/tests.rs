//! The tree over the alarm's window on hand-made tasks and lines: what a move's line says, when a
//! review mirror and an instruction subtask counted as work of their own, and a timeline counted
//! only then.

use super::*;
use crate::alarm::tests::line;

/// `slug`, made at `created` under `parent`; `instruction` an instruction subtask.
fn task(slug: &str, created: f64, parent: Option<&str>, instruction: bool) -> Task {
    Task {
        id: 0,
        slug: slug.into(),
        title: "T".into(),
        body: String::new(),
        status: "review".into(),
        priority: 2,
        parent: parent.map(String::from),
        branch: None,
        created_by: String::new(),
        created,
        updated: created,
        links: Vec::new(),
        depends: Vec::new(),
        roles: Vec::new(),
        instruction,
        fields: Default::default(),
    }
}

/// `slug`'s moves in the tree, each a system line at its time.
fn moves_of<'a>(slug: &'a str, lines: &'a [Line]) -> HashMap<&'a str, Vec<(f64, Moved<'a>)>> {
    let moved: Vec<(f64, Moved<'_>)> = (lines.iter())
        .filter_map(|line| Some((line.ts, moved(line)?)))
        .collect();
    HashMap::from([(slug, moved)])
}

#[test]
fn a_move_in_the_tree_says_the_parent_it_went_under_and_the_one_it_left() {
    let system = |text: &str| line("sim/a/i", 1, 0.0, "system", text);
    let cases = [
        (
            "w put it under sim/a (was nothing)",
            Some((Some("sim/a"), None)),
        ),
        (
            "w put it under nothing (was sim/a)",
            Some((None, Some("sim/a"))),
        ),
        (
            "w put it under sim/b (was sim/a)",
            Some((Some("sim/b"), Some("sim/a"))),
        ),
        // A line from before the board said what a move left: the tree taken as it was.
        ("w put it under sim/b", Some((Some("sim/b"), Some("sim/b")))),
        // A field's text that reads like a move is none.
        ("w set notes to x put it under sim/a (was nothing)", None),
    ];
    for (text, expected) in cases {
        let line = system(text);
        let said = moved(&line).map(|m| (m.to, m.from));
        assert_eq!(said, expected, "{text}");
    }
    let posted = line(
        "sim/a/i",
        1,
        0.0,
        "msg",
        "w put it under sim/a (was nothing)",
    );
    assert_eq!(moved(&posted), None, "only a system line");
}

#[test]
fn a_mirror_counts_on_its_own_until_the_task_it_reviews_is_made() {
    let tasks = [
        task("review/sim/a", 10.0, None, false),
        task("sim/a", 50.0, None, false),
    ];
    let counted: Vec<&Task> = tasks.iter().collect();
    let own = own_over(&tasks, &counted, &HashMap::new(), 0.0, 100.0);
    assert_eq!(
        own.get("review/sim/a").map(Vec::as_slice),
        Some(&[(0.0, false), (10.0, true), (50.0, false)][..]),
        "nothing before it was made"
    );
    assert!(
        !own.contains_key("sim/a"),
        "work of its own all through: {own:?}"
    );
}

#[test]
fn a_recorded_branch_alias_changes_the_mirrors_count_only_when_its_task_is_made() {
    let mut feature = task("feature/independent-name", 50.0, None, false);
    feature.branch = Some("change/branch".into());
    let tasks = [task("review/change/branch", 10.0, None, false), feature];
    let counted: Vec<&Task> = tasks.iter().collect();
    let own = own_over(&tasks, &counted, &HashMap::new(), 0.0, 100.0);
    assert_eq!(
        own.get("review/change/branch").map(Vec::as_slice),
        Some(&[(0.0, false), (10.0, true), (50.0, false)][..]),
        "an alias made later must not rewrite the earlier mirror history: {own:?}"
    );
    assert!(!own.contains_key("feature/independent-name"), "{own:?}");
}

#[test]
fn an_instruction_subtask_counts_with_its_parent_only_while_under_it() {
    let tasks = [
        task("sim/p", -10.0, None, false),
        task("sim/p/i", -5.0, Some("sim/p"), true),
    ];
    let counted: Vec<&Task> = tasks.iter().collect();
    let lines = [
        line(
            "sim/p/i",
            1,
            30.0,
            "system",
            "o put it under nothing (was sim/p)",
        ),
        line(
            "sim/p/i",
            2,
            60.0,
            "system",
            "o put it under sim/p (was nothing)",
        ),
    ];
    let moves = moves_of("sim/p/i", &lines);
    let own = own_over(&tasks, &counted, &moves, 0.0, 100.0);
    assert_eq!(
        own.get("sim/p/i").map(Vec::as_slice),
        Some(&[(0.0, false), (30.0, true), (60.0, false)][..])
    );
    // A move from before the board said what it left takes the tree as it was before it.
    let legacy = [line("sim/p/i", 1, 30.0, "system", "o put it under sim/p")];
    let moves = moves_of("sim/p/i", &legacy);
    let own = own_over(&tasks, &counted, &moves, 0.0, 100.0);
    assert_eq!(
        own.get("sim/p/i").map(Vec::as_slice),
        Some(&[(0.0, false)][..])
    );
}

#[test]
fn work_of_its_own_now_that_never_moved_in_the_window_is_left_out() {
    let tasks = [
        task("sim/x", -10.0, None, false),
        task("review/sim/gone", -10.0, None, false),
        task("sim/x/i", -5.0, None, true),
    ];
    let counted: Vec<&Task> = tasks.iter().collect();
    let own = own_over(&tasks, &counted, &HashMap::new(), 0.0, 100.0);
    assert!(own.is_empty(), "{own:?}");
}

#[test]
fn a_timeline_counts_only_while_its_task_was_work_of_its_own() {
    let status = [(0.0, Some("review")), (20.0, Some("done"))];
    assert_eq!(
        own_only(&status, &[(0.0, false), (10.0, true)]),
        [(0.0, None), (10.0, Some("review")), (20.0, Some("done"))]
    );
    assert_eq!(
        own_only(&status, &[(0.0, true), (5.0, false)]),
        [(0.0, Some("review")), (5.0, None)]
    );
}
