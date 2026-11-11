//! Real filesystem/Store/view attribution with explicitly supplied process inventories.

use super::*;
use std::fs;
use std::os::unix::fs::symlink;

#[test]
fn a_process_behind_any_inner_marker_cannot_keep_the_outer_agent_alive() {
    let c = Case::new();
    let tree = linked_worktree("marker-boundaries");
    let handle = c.subagent_in(&tree);
    c.past_the_lost_line();
    c.processes_in(vec![process(&tree, NOW)]);
    assert_eq!(c.agent(&handle).health, Health::Live, "outer owner");
    c.processes_in(vec![]);
    assert_eq!(c.agent(&handle).health, Health::Lost, "complete absence");

    for name in [
        "directory",
        "malformed",
        "valid",
        "dangling",
        "symlinked-valid",
        "symlink-loop",
    ] {
        let inner = tree.join(name);
        let deep = inner.join("component");
        fs::create_dir_all(&deep).unwrap();
        let marker = inner.join(".git");
        match name {
            "directory" => fs::create_dir(&marker).unwrap(),
            "malformed" => fs::write(&marker, "not a repository marker\n").unwrap(),
            "dangling" => symlink("missing-admin", &marker).unwrap(),
            "symlink-loop" => symlink(".git", &marker).unwrap(),
            _ => {
                let admin = tree.parent().unwrap().join(format!("admin-{name}"));
                fs::create_dir(&admin).unwrap();
                fs::write(admin.join("HEAD"), "ref: refs/heads/inner\n").unwrap();
                let contents = format!("gitdir: {}\n", admin.display());
                if name == "valid" {
                    fs::write(&marker, contents).unwrap();
                } else {
                    let target = admin.join("marker");
                    fs::write(&target, contents).unwrap();
                    symlink(target, &marker).unwrap();
                }
            }
        }
        assert!(marker.symlink_metadata().is_ok(), "{name}");
        for cwd in [&inner, &deep] {
            if matches!(name, "valid" | "symlinked-valid") {
                assert_eq!(
                    symphony_board::git::checkout(cwd),
                    Some((inner.clone(), "inner".into()))
                );
            } else {
                assert_eq!(symphony_board::git::checkout(cwd), None, "{name}: {cwd:?}");
            }
            c.processes_in(vec![process(cwd, NOW)]);
            let a = c.agent(&handle);
            assert_eq!(
                (a.state, a.health),
                ("gone", Health::Lost),
                "{name}: {cwd:?}"
            );
        }
    }
    c.processes_in(vec![process(&tree.join("ordinary/component"), NOW)]);
    assert_eq!(c.agent(&handle).health, Health::Live, "absent markers");
}

#[test]
fn unreadable_boundary_evidence_is_unknown_only_for_a_possible_owner() {
    let c = Case::new();
    let tree = linked_worktree("unreadable-boundary");
    let handle = c.subagent_in(&tree);
    c.past_the_lost_line();
    let looped = tree.join("loop");
    symlink("loop", &looped).unwrap();
    let cwd = looped.join("component");
    let error = cwd.join(".git").symlink_metadata().unwrap_err();
    assert_ne!(error.kind(), std::io::ErrorKind::NotFound);
    c.processes_in(vec![process(&cwd, NOW)]);
    let a = c.agent(&handle);
    assert_eq!((a.state, a.health), ("running_test", Health::Unknown));
    assert_eq!(a.detail.unwrap().phase, "unknown");
    c.processes_in(vec![process(&cwd, NOW - 60.0)]);
    assert_eq!(c.agent(&handle).health, Health::Lost, "outside the stay");
    c.processes_in(vec![process(&cwd, NOW), process(&tree, NOW)]);
    assert_eq!(c.agent(&handle).health, Health::Live, "known owner wins");
    c.processes_in(vec![process(
        &tree.with_file_name("foreign/loop/component"),
        NOW,
    )]);
    assert_eq!(c.agent(&handle).health, Health::Lost, "foreign cwd");
    *c.processes.borrow_mut() = Some(ProcessScan {
        listed: vec![],
        complete: false,
    });
    assert_eq!(
        c.agent(&handle).health,
        Health::Unknown,
        "partial inventory is separate"
    );
}
