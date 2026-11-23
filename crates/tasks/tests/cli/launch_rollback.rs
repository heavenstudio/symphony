//! A failed launch must release its partial setup so the orchestrator can retry it.

use super::launch::Terminals;
use symphony_board::{Database, Health, NewTask, Store};

/// A trigger named `name` on `table` that fails each row an `event` (`INSERT`, `UPDATE`) writes
/// where `when` holds, raising `why`: a database error the board's own writes then meet.
fn refusing(board: &Store, name: &str, event: &str, table: &str, when: &str, why: &str) {
    send(
        board,
        &format!(
            "CREATE OR REPLACE FUNCTION fixture_refuse() RETURNS trigger LANGUAGE plpgsql AS $f$ \
             BEGIN RAISE EXCEPTION '%', TG_ARGV[0]; END $f$; \
             CREATE TRIGGER {name} BEFORE {event} ON {table} FOR EACH ROW WHEN ({when}) \
             EXECUTE FUNCTION fixture_refuse('{why}');"
        ),
    );
}

/// `sql` sent to the board's database as it is.
fn send(board: &Store, sql: &str) {
    let Database::PostgreSql(client) = board.database();
    client.simple(sql).unwrap();
}

fn failed_handle(terminals: &Terminals, actor: &str) -> String {
    let children: Vec<_> = terminals
        .board()
        .sessions()
        .unwrap()
        .into_iter()
        .filter(|s| s.row.parent.as_deref() == Some(actor))
        .collect();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].health, Health::Left);
    children[0].row.handle.clone()
}

fn released(terminals: &Terminals, slug: &str) {
    let board = terminals.board();
    let task = board.get(slug).unwrap();
    let reserved = task.holders("reserved");
    assert!(reserved.is_empty(), "{slug} reservations: {reserved:?}");
    let launches = board.task_sessions(slug).unwrap();
    assert!(launches.is_empty(), "{slug} launch records: {launches:?}");
    let addresses = terminals.addresses();
    assert!(addresses.is_empty(), "terminals: {addresses:?}");
}

#[test]
fn a_missing_parent_feature_branch_releases_the_slice_handle() {
    let p = Terminals::new("missing-parent-feature");
    let actor = p.orchestrator();
    let board = p.board();
    board
        .create(&NewTask {
            parent: Some("sim/jump".into()),
            ..NewTask::new("sim/jump/slice", "Slice", &actor)
        })
        .unwrap();
    let parent_roles = board.get("sim/jump").unwrap().roles;
    let failed = p.as_(&actor, &["launch", "sim/jump/slice"]);
    assert_eq!(failed.code, 1, "{} {}", failed.out, failed.err);
    assert!(
        failed.err.contains("create parent feature branch sim/jump"),
        "{}",
        failed.err
    );
    failed_handle(&p, &actor);
    released(&p, "sim/jump/slice");
    assert_eq!(
        board.get("sim/jump").unwrap().roles,
        parent_roles,
        "failed slice setup must preserve the parent's roles"
    );
    let checkout = p.dir.join("wt/sim-jump-slice");
    assert!(
        !checkout.exists(),
        "unexpected checkout {}",
        checkout.display()
    );
}

#[test]
fn a_failing_post_checkout_hook_removes_the_owned_checkout_and_allows_retry() {
    use std::os::unix::fs::PermissionsExt;
    for detached in [false, true] {
        let p = Terminals::new(&format!("post-checkout-failure-{detached}"));
        let actor = p.orchestrator();
        let hook = p.dir.join("lamp-works/.git/hooks/post-checkout");
        std::fs::write(&hook, "#!/bin/sh\necho checkout-hook-failed >&2\nexit 1\n").unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut args = vec!["launch", "sim/jump"];
        if detached {
            args.push("--detached");
        }
        let failed = p.as_(&actor, &args);
        assert_eq!(failed.code, 1, "{} {}", failed.out, failed.err);
        assert!(
            failed.err.contains("checkout-hook-failed"),
            "{}",
            failed.err
        );
        failed_handle(&p, &actor);
        released(&p, "sim/jump");
        let checkout = p.dir.join("wt/sim-jump");
        assert!(
            !checkout.exists(),
            "the hook failure left checkout {}",
            checkout.display()
        );
        std::fs::remove_file(hook).unwrap();
        let retried = p.as_(&actor, &args);
        assert_eq!(retried.code, 0, "{} {}", retried.out, retried.err);
        assert_eq!(p.board().task_sessions("sim/jump").unwrap().len(), 1);
    }
}

#[test]
fn a_database_error_taking_the_reviewers_role_leaves_its_handle_and_allows_a_retry() {
    let p = Terminals::new("setup-db-failure");
    let actor = p.orchestrator();
    let board = p.board();
    board.claim("sim/jump", &actor).unwrap();
    board.move_to("sim/jump", "review", &actor).unwrap();
    refusing(
        &board,
        "unavailable",
        "INSERT",
        "role",
        "NEW.role = 'reviewer'",
        "review role unavailable",
    );
    let args = ["launch", "sim/jump", "--role", "reviewer"];
    let failed = p.as_(&actor, &args);
    assert_eq!(failed.code, 2, "{} {}", failed.out, failed.err);
    assert!(
        failed.err.contains("review role unavailable"),
        "{}",
        failed.err
    );
    let handle = failed_handle(&p, &actor);
    released(&p, "sim/jump");
    let task = board.get("sim/jump").unwrap();
    let reviewers = task.holders("reviewer");
    assert!(
        !reviewers.contains(&handle.as_str()),
        "failed handle {handle} still reviews: {reviewers:?}"
    );
    send(&board, "DROP TRIGGER unavailable ON role;");
    let retried = p.as_(&actor, &args);
    assert_eq!(retried.code, 0, "{} {}", retried.out, retried.err);
    assert_eq!(board.task_sessions("sim/jump").unwrap().len(), 1);
}

#[test]
fn a_failed_final_launch_write_closes_the_terminal_and_allows_a_fresh_launch() {
    for supplied in [false, true] {
        let p = Terminals::new(&format!("record-db-failure-{supplied}"));
        let actor = p.orchestrator();
        let board = p.board();
        let given = p.dir.join("given");
        std::fs::create_dir_all(&given).unwrap();
        std::fs::write(given.join("keep"), "existing work").unwrap();
        let given_arg = given.to_string_lossy();
        let mut args = vec!["launch", "sim/jump"];
        if supplied {
            args.extend(["--worktree", &given_arg]);
        }
        refusing(
            &board,
            "unavailable",
            "INSERT",
            "task_session",
            "true",
            "launch record unavailable",
        );
        let failed = p.as_(&actor, &args);
        assert_eq!(failed.code, 2, "{} {}", failed.out, failed.err);
        assert!(
            failed.err.contains("launch record unavailable"),
            "{}",
            failed.err
        );
        failed_handle(&p, &actor);
        released(&p, "sim/jump");
        assert_eq!(p.calls().iter().filter(|c| c[0] == "kill").count(), 1);
        assert_eq!(
            std::fs::read_to_string(given.join("keep")).unwrap(),
            "existing work"
        );
        let checkout = p.dir.join("wt/sim-jump");
        assert!(
            !checkout.exists(),
            "unexpected checkout {}",
            checkout.display()
        );
        send(&board, "DROP TRIGGER unavailable ON task_session;");
        let retried = p.as_(&actor, &args);
        assert_eq!(retried.code, 0, "{} {}", retried.out, retried.err);
        assert_eq!(board.task_sessions("sim/jump").unwrap().len(), 1);
    }
}

#[test]
fn cleanup_failures_keep_the_original_error_and_name_the_resource_still_to_clean() {
    for terminal_failure in [false, true] {
        let mut p = Terminals::new(&format!("rollback-failure-{terminal_failure}"));
        let actor = p.orchestrator();
        let board = p.board();
        refusing(
            &board,
            "unavailable",
            "INSERT",
            "task_session",
            "true",
            "launch record unavailable",
        );
        if terminal_failure {
            p.set("FAKE_HC_FAIL", "kill");
        } else {
            refusing(
                &board,
                "cannot_leave",
                "INSERT OR UPDATE",
                "session",
                "NEW.left_at IS NOT NULL",
                "leave unavailable",
            );
        }
        let failed = p.as_(&actor, &["launch", "sim/jump"]);
        assert_eq!(failed.code, 2, "{} {}", failed.out, failed.err);
        assert!(
            failed.err.contains("launch record unavailable"),
            "{}",
            failed.err
        );
        assert!(
            failed.err.contains("launch cleanup incomplete"),
            "{}",
            failed.err
        );
        if terminal_failure {
            let [terminal]: [String; 1] = p.addresses().try_into().unwrap();
            assert!(
                failed.err.contains(&format!("terminal {terminal}:")),
                "{}",
                failed.err
            );
            failed_handle(&p, &actor);
            let task = board.get("sim/jump").unwrap();
            let reserved = task.holders("reserved");
            assert!(reserved.is_empty(), "reservations: {reserved:?}");
            let checkout = p.dir.join("wt/sim-jump");
            assert!(
                checkout.exists(),
                "a possibly running agent lost checkout {}",
                checkout.display()
            );
        } else {
            let task = board.get("sim/jump").unwrap();
            let [handle]: [&str; 1] = task.holders("reserved").try_into().unwrap();
            assert!(
                failed.err.contains(&format!("release {handle}:")),
                "{}",
                failed.err
            );
            assert!(failed.err.contains("leave unavailable"), "{}", failed.err);
            let addresses = p.addresses();
            assert!(addresses.is_empty(), "terminals: {addresses:?}");
            let checkout = p.dir.join("wt/sim-jump");
            assert!(
                !checkout.exists(),
                "unexpected checkout {}",
                checkout.display()
            );
        }
    }
}
