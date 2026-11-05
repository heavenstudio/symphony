//! The priority command, including immediate ordering changes on a scratch board.

use super::Case;

#[test]
fn changing_p2_to_p0_immediately_updates_next_show_list_and_pull() {
    let c = Case::new();
    let actor = c.register("worker");
    for (slug, title, priority) in [
        ("sim/first", "First urgent task", "1"),
        ("sim/second", "Second urgent task", "1"),
        ("sim/promote", "Promote this task", "2"),
    ] {
        c.ok(&actor, &["create", slug, title, "--priority", priority]);
    }
    assert_eq!(
        c.ok(&actor, &["next"]),
        "sim/first  p1  First urgent task\n"
    );
    assert_eq!(
        c.ok(&actor, &["priority", "sim/promote", "0"]),
        "sim/promote is p0\n"
    );
    assert_eq!(
        c.ok(&actor, &["next"]),
        "sim/promote  p0  Promote this task\n"
    );
    let shown = c.ok(&actor, &["show", "sim/promote"]);
    assert!(shown.contains("sim/promote — Promote this task  [ready, p0]"));
    assert!(shown.contains(&format!("{actor} changed priority p2 → p0")));
    let listed = c.ok(&actor, &["list"]);
    let lines: Vec<&str> = listed.lines().collect();
    assert_eq!(lines.len(), 3);
    for (line, slug, priority) in [
        (lines[0], "sim/promote", "p0"),
        (lines[1], "sim/first", "p1"),
        (lines[2], "sim/second", "p1"),
    ] {
        assert!(line.starts_with(priority), "{line}");
        assert!(line.contains(slug), "{line}");
    }
    let digest = symphony_board::cli::pull::digest(&c.store(), &[], None, super::common::NOW, &[])
        .unwrap()
        .lines();
    assert_eq!(
        &digest[1..],
        [
            "  ready_unstaffed p0 sim/promote — Promote this task",
            "  ready_unstaffed p1 sim/first — First urgent task",
            "  ready_unstaffed p1 sim/second — Second urgent task",
        ]
    );
}

#[test]
fn bad_input_and_a_missing_task_exit_2_without_changing_the_board() {
    let c = Case::new();
    let actor = c.register("worker");
    c.ok(&actor, &["create", "sim/jump", "Jump departure"]);
    let before = c.store().dump().unwrap();
    for (args, reason) in [
        (vec!["priority", "sim/jump", "5"], "invalid choice: '5'"),
        (vec!["priority", "sim/jump", "-1"], "invalid choice: '-1'"),
        (
            vec!["priority", "sim/jump", "high"],
            "invalid choice: 'high'",
        ),
        (vec!["priority", "sim/jump"], "required: priority"),
        (
            vec!["priority", "sim/missing", "0"],
            "no task 'sim/missing'",
        ),
    ] {
        let ran = c.run_as(&actor, &args, "");
        assert_eq!(ran.code, 2, "{args:?}: {}", ran.err);
        assert!(ran.out.is_empty(), "{args:?}: {}", ran.out);
        assert!(ran.err.contains(reason), "{args:?}: {}", ran.err);
        assert_eq!(c.store().dump().unwrap(), before, "{args:?}");
    }
    let ran = c.run_as("", &["priority", "sim/jump", "0"], "");
    assert_eq!(ran.code, 2);
    assert!(ran.err.contains("TASKS_SESSION"), "{}", ran.err);
    assert_eq!(c.store().dump().unwrap(), before);
}

#[test]
fn explicit_actor_works_on_either_side_and_help_lists_the_command() {
    let c = Case::new();
    let actor = c.register("worker");
    c.ok(&actor, &["create", "sim/jump", "Jump departure"]);
    assert_eq!(
        c.ok("", &["--as", &actor, "priority", "sim/jump", "4"]),
        "sim/jump is p4\n"
    );
    assert_eq!(
        c.ok("", &["priority", "sim/jump", "0", "--as", &actor]),
        "sim/jump is p0\n"
    );
    let history = c.store().thread("sim/jump").unwrap();
    assert_eq!(history.last().unwrap().sender, actor);
    let before = c.store().dump().unwrap();
    assert!(
        c.ok("", &["--help"])
            .contains("create,priority,field,claim")
    );
    assert!(
        c.ok("", &["priority", "--help"])
            .starts_with("usage: just tasks priority [-h] [--as ACTOR] slug {0,1,2,3,4}\n")
    );
    assert_eq!(c.store().dump().unwrap(), before);
}
