//! `launch`: the handle, the worktree and its branch, the pane, claude and its prompt, and the record
//! on the task; what a failure undoes.

use crate::common::refused;
use crate::{Hooked, World, git};
use serde_json::{Value, json};
use std::cell::Cell;
use std::fs;
use std::path::Path;
use tasks::Error;
use tasks::launch::herdr::Herdr;
use tasks::launch::{Ask, LaunchError, PROMPT_ENTERS, PROMPT_WAIT, START_TRIES};

fn flag_value<'a>(call: &'a [String], flag: &str) -> &'a str {
    let at = call
        .iter()
        .position(|a| a == flag)
        .expect("the flag is there");
    &call[at + 1]
}

/// The arguments `agent start` gave claude.
fn claude_args(call: &[String]) -> &[String] {
    let at = call.iter().position(|a| a == "--").unwrap();
    &call[at + 1..]
}

#[test]
fn an_executor_gets_a_branch_named_for_the_slug_in_a_worktree_under_wt() {
    let w = World::new();
    let s = w.launch("sim/jump").row;
    let path = w.wt.join("sim-jump");
    assert_eq!(s.worktree, path.display().to_string());
    assert_eq!(s.branch, "sim/jump");
    assert_eq!(
        git(&path, &["rev-parse", "--abbrev-ref", "HEAD"]),
        "sim/jump"
    );
}

#[test]
fn a_reviewer_gets_a_detached_checkout_of_main() {
    let w = World::new();
    let s = w.review("sim/jump").row;
    let path = Path::new(&s.worktree);
    assert_eq!(s.branch, "");
    assert_eq!(git(path, &["rev-parse", "--abbrev-ref", "HEAD"]), "HEAD");
    assert_eq!(
        git(path, &["rev-parse", "HEAD"]),
        git(&w.repo, &["rev-parse", "main"])
    );
}

#[test]
fn the_detached_flag_gives_an_executor_a_detached_checkout_too() {
    let w = World::new();
    let ask = Ask {
        detached: true,
        ..w.ask("sim/jump")
    };
    assert_eq!(w.try_launch(&ask).unwrap().row.branch, "");
}

#[test]
fn an_explicit_branch_is_used_for_the_worktree() {
    let w = World::new();
    let ask = Ask {
        branch: Some("tools/other".into()),
        ..w.ask("sim/jump")
    };
    let s = w.try_launch(&ask).unwrap().row;
    assert_eq!(s.branch, "tools/other");
    assert_eq!(
        git(
            Path::new(&s.worktree),
            &["rev-parse", "--abbrev-ref", "HEAD"]
        ),
        "tools/other"
    );
}

#[test]
fn an_existing_branch_is_checked_out_rather_than_made() {
    let w = World::new();
    git(&w.repo, &["branch", "sim/jump"]);
    let s = w.launch("sim/jump").row;
    assert_eq!(
        git(
            Path::new(&s.worktree),
            &["rev-parse", "--abbrev-ref", "HEAD"]
        ),
        "sim/jump"
    );
}

#[test]
fn a_given_worktree_is_used_as_it_is() {
    let w = World::new();
    let given = w.dir().join("already");
    fs::create_dir(&given).unwrap();
    let ask = Ask {
        worktree: Some(given.clone()),
        ..w.ask("sim/jump")
    };
    let s = w.try_launch(&ask).unwrap().row;
    assert_eq!(s.worktree, given.display().to_string());
    assert!(!w.wt.join("sim-jump").exists());
}

#[test]
fn a_given_worktree_that_does_not_exist_is_refused() {
    let w = World::new();
    let nowhere = w.dir().join("nowhere");
    let ask = Ask {
        worktree: Some(nowhere.clone()),
        ..w.ask("sim/jump")
    };
    assert_eq!(
        refused(w.try_launch(&ask)),
        format!("--worktree {} is not a directory", nowhere.display())
    );
    assert_eq!(w.store().task_sessions("sim/jump").unwrap(), vec![]);
}

#[test]
fn a_given_worktree_records_the_branch_it_has_checked_out_or_none_when_detached() {
    let w = World::new();
    let (given, detached) = (w.dir().join("given"), w.dir().join("detached"));
    git(
        &w.repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "other/work",
            &given.display().to_string(),
            "main",
        ],
    );
    git(
        &w.repo,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            &detached.display().to_string(),
            "main",
        ],
    );
    w.task("sim/dock", None);
    for (slug, path, branch) in [
        ("sim/jump", given, "other/work"),
        ("sim/dock", detached, ""),
    ] {
        let ask = Ask {
            worktree: Some(path),
            ..w.ask(slug)
        };
        assert_eq!(w.try_launch(&ask).unwrap().row.branch, branch, "{slug}");
    }
}

#[test]
fn a_reviewer_beside_an_executor_gets_a_worktree_of_its_own() {
    let w = World::new();
    let a = w.launch("sim/jump").row.worktree;
    let b = w.review("sim/jump").row.worktree;
    let c = w.review("sim/jump").row.worktree;
    assert_eq!(
        [a, b, c].map(|p| Path::new(&p)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned()),
        ["sim-jump", "sim-jump-review", "sim-jump-review-2"].map(String::from)
    );
}

#[test]
fn a_worktree_that_exists_is_refused_not_overwritten() {
    let w = World::new();
    fs::create_dir_all(w.wt.join("sim-jump")).unwrap();
    assert_eq!(
        refused(w.try_launch(&w.ask("sim/jump"))),
        format!(
            "{} exists: give it with --worktree, or remove it",
            w.wt.join("sim-jump").display()
        )
    );
    assert_eq!(w.store().task_sessions("sim/jump").unwrap(), vec![]);
}

#[test]
fn claude_starts_with_a_session_id_chosen_up_front_and_the_model() {
    let w = World::new();
    let s = w
        .try_launch(&Ask {
            kind: Some("claude".into()),
            ..w.ask("sim/jump")
        })
        .unwrap()
        .row;
    let [start] = <[_; 1]>::try_from(w.calls_of(&["agent", "start"])).unwrap();
    assert_eq!(flag_value(&start, "--kind"), "claude");
    assert_eq!(start[2], s.handle);
    assert_eq!(s.claude_session, "00000001-0000-4000-8000-000000000000");
    assert_eq!(
        claude_args(&start),
        [
            "--session-id",
            &s.claude_session,
            "-n",
            &s.handle,
            "--permission-mode",
            "auto",
            "--model",
            "sonnet"
        ]
    );
}

#[test]
fn codex_is_the_default_and_records_the_conversation_from_the_prompt_check() {
    for (model, expected) in [("", "gpt-6.1-sol"), ("chosen-model", "chosen-model")] {
        let w = World::new();
        let mut herdr = Hooked::new(&w);
        herdr.after = Box::new(|args| {
            if args[..2] == ["agent", "prompt"] {
                w.edit_panes(|panes| {
                    panes.last_mut().unwrap()["agent_session"] = serde_json::json!({
                        "agent": "codex", "kind": "id", "value": "codex-conversation"
                    });
                });
            }
        });
        let ask = Ask {
            model: model.into(),
            ..w.ask("sim/jump")
        };
        let s = tasks::launch::launch(w.store(), &herdr, &w.machine(), &ask)
            .unwrap()
            .row;
        assert_eq!(s.claude_session, "codex:codex-conversation");
        let [start] = <[_; 1]>::try_from(w.calls_of(&["agent", "start"])).unwrap();
        assert_eq!(flag_value(&start, "--kind"), "codex");
        assert_eq!(
            claude_args(&start),
            [
                "-s",
                "workspace-write",
                "-m",
                expected,
                "-c",
                "model_reasoning_effort=\"xhigh\"",
                "-c",
                "sandbox_workspace_write.network_access=true",
                "-c",
                &format!(
                    "shell_environment_policy.set.TASKS_SESSION=\"{}\"",
                    s.handle
                ),
            ]
        );
        assert_eq!(w.calls_of(&["agent", "list"]).len(), 1);
        assert_eq!(w.store().task_sessions("sim/jump").unwrap(), [s]);
    }
}

#[test]
fn hooks_review_is_declined_during_one_start_before_prompting_and_recording_the_session() {
    for (selected, expected) in [
        (1, vec![vec!["down", "down"], vec!["enter"]]),
        (2, vec![vec!["down"], vec!["enter"]]),
        (3, vec![vec!["enter"]]),
    ] {
        let mut w = World::new();
        let mut screen = include_str!("fixtures/codex-hooks.txt").replace("› 1.", "  1.");
        screen = screen.replace(&format!("  {selected}."), &format!("› {selected}."));
        let file = w.dir().join("hooks-screen.txt");
        std::fs::write(&file, screen).unwrap();
        w.set("FAKE_HERDR_START_SCREEN", file.to_str().unwrap());
        w.set("FAKE_HERDR_START_SELECTED", &selected.to_string());
        let s = w.launch("sim/jump").row;
        assert_eq!(s.claude_session, "codex:startup-probe-session");
        assert_eq!(w.store().task_sessions("sim/jump").unwrap(), [s]);
        let starts = w.calls_of(&["agent", "start"]);
        assert_eq!(starts.len(), 1);
        assert!(!starts[0].iter().any(|s| s == "--timeout"));
        let keys = w.calls_of(&["pane", "send-keys"]);
        assert_eq!(
            keys.iter()
                .map(|s| s[3..].iter().map(String::as_str).collect::<Vec<_>>())
                .collect::<Vec<_>>(),
            expected
        );
        let state = w.state();
        let pane = state["panes"].as_array().unwrap().last().unwrap();
        assert_eq!(pane["confirmed_choice"], 3);
        assert_eq!(pane["interactive_ready"], true);
        let calls = w.calls();
        let keys_at = calls
            .iter()
            .position(|s| s.starts_with(&["pane".into(), "send-keys".into()]))
            .unwrap();
        let prompt_at = calls
            .iter()
            .position(|s| s.starts_with(&["agent".into(), "prompt".into()]))
            .unwrap();
        let enter_at = calls
            .iter()
            .rposition(|s| s.starts_with(&["pane".into(), "send-keys".into()]))
            .unwrap();
        assert!(enter_at < prompt_at);
        if selected != 3 {
            assert!(
                calls[keys_at + 1..enter_at].iter().any(|s| {
                    s.starts_with(&["pane".into(), "read".into()])
                        && s.iter().any(|v| v == "detection")
                }),
                "Enter needs a fresh chooser read after navigation: {calls:?}"
            );
        }
    }
}

#[test]
fn failed_startup_keys_reap_the_pending_cli_and_leave_no_launch_or_prompt() {
    let mut w = World::new();
    let file = w.dir().join("hooks-screen.txt");
    std::fs::write(&file, include_str!("fixtures/codex-hooks.txt")).unwrap();
    let pid_file = w.dir().join("start.pid");
    w.set("FAKE_HERDR_START_SCREEN", file.to_str().unwrap());
    w.set("FAKE_HERDR_START_PID", pid_file.to_str().unwrap());
    w.set("FAKE_HERDR_FAIL", "pane send-keys");
    let why = w.try_launch(&w.ask("sim/jump")).unwrap_err().to_string();
    assert!(why.contains("herdr pane send-keys failed"), "{why}");
    let pid: i64 = std::fs::read_to_string(pid_file).unwrap().parse().unwrap();
    assert!(!tasks::sys::pid_alive(pid));
    assert_eq!(w.calls_of(&["agent", "start"]).len(), 1);
    assert!(w.calls_of(&["agent", "prompt"]).is_empty());
    assert!(w.store().task_sessions("sim/jump").unwrap().is_empty());
    assert!(w.pane_ids().is_empty());
}

#[test]
fn a_dropped_down_never_confirms_trust_and_startup_times_out_without_a_retry() {
    let mut w = World::new();
    let file = w.dir().join("hooks-screen.txt");
    std::fs::write(&file, include_str!("fixtures/codex-hooks.txt")).unwrap();
    w.set("FAKE_HERDR_START_SCREEN", file.to_str().unwrap());
    w.set("FAKE_HERDR_START_WAIT_MS", "2000");
    w.set("FAKE_HERDR_DROP_DOWN", "1");
    let why = w.try_launch(&w.ask("sim/jump")).unwrap_err().to_string();
    assert!(why.contains("timed out waiting for agent startup"), "{why}");
    assert_eq!(w.calls_of(&["agent", "start"]).len(), 1);
    let keys = w.calls_of(&["pane", "send-keys"]);
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0][3..], ["down", "down"]);
    let calls = w.calls();
    let moved_at = calls
        .iter()
        .position(|s| s.starts_with(&["pane".into(), "send-keys".into()]))
        .unwrap();
    assert!(calls[moved_at + 1..].iter().any(|s| {
        s.starts_with(&["pane".into(), "read".into()]) && s.iter().any(|v| v == "detection")
    }));
    assert!(w.calls_of(&["agent", "prompt"]).is_empty());
    assert!(w.store().task_sessions("sim/jump").unwrap().is_empty());
    assert!(w.pane_ids().is_empty());
}

#[test]
fn a_pending_nonchooser_gets_no_keys_and_keeps_the_original_startup_failure() {
    let mut w = World::new();
    let file = w.dir().join("other-screen.txt");
    std::fs::write(&file, "Update available\n1. Update now\n2. Skip").unwrap();
    w.set("FAKE_HERDR_START_SCREEN", file.to_str().unwrap());
    w.set("FAKE_HERDR_START_WAIT_MS", "2000");
    let why = w.try_launch(&w.ask("sim/jump")).unwrap_err().to_string();
    assert!(why.contains("herdr agent start failed:"), "{why}");
    assert!(why.contains("timed out waiting for agent startup"), "{why}");
    assert!(!w.calls_of(&["pane", "read"]).is_empty());
    assert!(w.calls_of(&["pane", "send-keys"]).is_empty());
    assert_eq!(w.calls_of(&["agent", "start"]).len(), 1);
    assert!(w.calls_of(&["agent", "prompt"]).is_empty());
    assert!(w.store().task_sessions("sim/jump").unwrap().is_empty());
    assert!(w.pane_ids().is_empty());
}

#[test]
fn no_model_starts_claude_with_its_own() {
    let w = World::new();
    let ask = Ask {
        kind: Some("claude".into()),
        model: String::new(),
        ..w.ask("sim/jump")
    };
    w.try_launch(&ask).unwrap();
    let start = &w.calls_of(&["agent", "start"])[0];
    assert!(!start.contains(&"--model".to_string()), "{start:?}");
}

#[test]
fn the_prompt_is_work_slug_handle_alone_or_followed_by_a_given_prompt() {
    for (kind, work) in [(None, "$work"), (Some("claude".into()), "/work")] {
        let w = World::new();
        let s = w
            .try_launch(&Ask {
                kind: kind.clone(),
                ..w.ask("sim/jump")
            })
            .unwrap()
            .row;
        w.task("sim/dock", None);
        let ask = Ask {
            kind,
            prompt: "  Read the brief.  ".into(),
            ..w.ask("sim/dock")
        };
        let dock = w.try_launch(&ask).unwrap().row;
        let prompts = &w.state()["prompts"];
        assert_eq!(prompts[0][0], s.handle.as_str());
        assert_eq!(
            prompts[0][1],
            format!("{work} sim/jump {}", s.handle).as_str()
        );
        assert_eq!(prompts[1][0], dock.handle.as_str());
        assert_eq!(
            prompts[1][1],
            format!("{work} sim/dock {} Read the brief.", dock.handle).as_str()
        );
    }
}

#[test]
fn everything_is_recorded_on_the_task_and_the_session() {
    let w = World::new();
    let s = w.launch("sim/jump").row;
    let [read] = <[_; 1]>::try_from(w.store().task_sessions("sim/jump").unwrap()).unwrap();
    assert_eq!(read, s);
    assert_eq!(
        (
            read.role.as_str(),
            read.model.as_str(),
            read.started,
            read.closed
        ),
        ("executor", "sonnet", w.f.now(), None)
    );
    assert!(read.pane.starts_with("w1:p"), "{}", read.pane);
    assert!(w.has_pane(&read.pane));
    let board = w.store().session(&s.handle).unwrap().row;
    assert_eq!(
        (
            board.kind.as_str(),
            board.worktree.as_str(),
            board.branch.as_str(),
            board.parent
        ),
        (
            "worker",
            s.worktree.as_str(),
            "sim/jump",
            Some(w.orch.clone())
        )
    );
    assert!(
        w.f.said("sim/jump")
            .iter()
            .any(|m| m.contains("launched") && m.contains(&s.handle)),
        "{:?}",
        w.f.said("sim/jump")
    );
}

#[test]
fn the_executor_has_the_task_reserved_for_its_work_command() {
    let w = World::new();
    let s = w.launch("sim/jump").row;
    assert_eq!(
        w.store().get("sim/jump").unwrap().holders("reserved"),
        [s.handle.as_str()]
    );
}

#[test]
fn a_reviewer_takes_the_reviewer_role() {
    let w = World::new();
    let s = w.review("sim/jump").row;
    assert_eq!(
        w.store().get("sim/jump").unwrap().holders("reviewer"),
        [s.handle.as_str()]
    );
    assert_eq!(w.store().session(&s.handle).unwrap().row.kind, "reviewer");
}

#[test]
fn a_launched_reviewer_of_a_review_subtask_reserves_it_and_judges_its_parent() {
    let w = World::new();
    w.task("sim/jump/review-x", Some("sim/jump"));
    let s = w.review("sim/jump/review-x").row;
    w.store().claim("sim/jump/review-x", &s.handle).unwrap();
    assert!(
        w.store()
            .get("sim/jump")
            .unwrap()
            .holders("reviewer")
            .contains(&s.handle.as_str())
    );
    assert_eq!(
        w.store().task_sessions("sim/jump/review-x").unwrap()[0].role,
        "reviewer"
    );
}

#[test]
fn an_instruction_launches_on_detached_main_at_any_depth() {
    for slug in ["sim/jump/r2", "sim/jump/r2/deep"] {
        let w = World::new();
        w.launch("sim/jump"); // the root's branch may already be in use
        w.task("sim/jump/r2", Some("sim/jump"));
        w.task("sim/jump/r2/deep", Some("sim/jump/r2"));
        let s = w.launch(slug).row;
        assert_eq!((s.slug.as_str(), s.branch.as_str()), (slug, ""));
        assert_eq!(
            Path::new(&s.worktree)
                .file_name()
                .unwrap()
                .to_str()
                .unwrap(),
            slug.replace('/', "-")
        );
        assert_eq!(
            git(
                Path::new(&s.worktree),
                &["rev-parse", "--abbrev-ref", "HEAD"]
            ),
            "HEAD"
        );
        assert_eq!(
            git(Path::new(&s.worktree), &["rev-parse", "HEAD"]),
            git(&w.repo, &["rev-parse", "main"])
        );
    }
}

#[test]
fn an_explicit_root_branch_checked_out_elsewhere_is_a_clear_refusal() {
    let w = World::new();
    let first = w.launch("sim/jump").row; // the root's own worktree holds sim/jump
    w.task("sim/jump/r2", Some("sim/jump"));
    let why = refused(w.try_launch(&Ask {
        branch: Some("sim/jump".into()),
        ..w.ask("sim/jump/r2")
    }));
    assert_eq!(
        why,
        format!(
            "branch sim/jump is checked out in {wt}: to continue it, launch with --worktree {wt}; \
             --branch <other> starts another branch; --detached is for read-only work",
            wt = first.worktree
        )
    );
}

#[test]
fn a_regular_subtask_owns_its_branch_even_while_the_parent_is_checked_out() {
    let w = World::new();
    // A ref cannot be both a branch and a directory for another branch. Give the parent a
    // sibling name so this case tests the subtask's default, rather than that ref limitation.
    let parent = w
        .try_launch(&Ask {
            branch: Some("sim/jump-root".into()),
            ..w.ask("sim/jump")
        })
        .unwrap()
        .row;
    assert_eq!(parent.branch, "sim/jump-root");
    w.store()
        .create(&tasks::NewTask {
            parent: Some("sim/jump".into()),
            ..tasks::NewTask::new("sim/jump/build", "Build", "owner")
        })
        .unwrap();
    let s = w.launch("sim/jump/build").row;
    assert_eq!(s.branch, "sim/jump/build");
    assert_eq!(
        git(Path::new(&s.worktree), &["branch", "--show-current"]),
        "sim/jump/build"
    );
}

#[test]
fn a_task_that_is_finished_or_unknown_is_refused() {
    let w = World::new();
    assert_eq!(
        refused(w.try_launch(&w.ask("sim/none"))),
        "no task 'sim/none'"
    );
    w.store().move_to("sim/jump", "cancelled", &w.orch).unwrap();
    assert_eq!(
        refused(w.try_launch(&w.ask("sim/jump"))),
        "sim/jump is cancelled"
    );
    assert_eq!(w.calls(), Vec::<Vec<String>>::new());
}

#[test]
fn a_role_that_is_no_launched_role_is_refused() {
    let w = World::new();
    let ask = Ask {
        role: "orchestrator".into(),
        ..w.ask("sim/jump")
    };
    assert_eq!(
        refused(w.try_launch(&ask)),
        "a launched session is an executor or a reviewer, not 'orchestrator'"
    );
}

#[test]
fn a_role_the_board_refuses_leaves_the_handle_and_makes_nothing() {
    let w = World::new();
    let other = w.f.session("worker");
    w.store().reserve("sim/jump", &other, 3600.0).unwrap();
    assert!(matches!(
        w.try_launch(&w.ask("sim/jump")),
        Err(Error::Refused(_))
    ));
    let left: Vec<String> = w
        .store()
        .sessions()
        .unwrap()
        .into_iter()
        .filter(|s| s.row.kind == "worker" && s.row.handle != other)
        .map(|s| s.health.as_str().to_string())
        .collect();
    assert_eq!(left, ["left"]);
    assert!(!w.wt.join("sim-jump").exists());
    assert_eq!(w.pane_ids(), Vec::<String>::new());
}

#[test]
fn a_failed_agent_start_closes_the_pane_undoes_the_worktree_and_leaves_the_handle() {
    let mut w = World::new();
    w.set("FAKE_HERDR_FAIL", "agent start");
    let why = refused(w.try_launch(&w.ask("sim/jump")));
    assert!(why.starts_with("herdr agent start failed: "), "{why}");
    assert!(why.contains("fake_failure"), "{why}");
    assert_eq!(w.pane_ids(), Vec::<String>::new());
    assert_eq!(w.store().task_sessions("sim/jump").unwrap(), vec![]);
    assert!(!w.wt.join("sim-jump").exists());
    let workers: Vec<String> = w
        .store()
        .sessions()
        .unwrap()
        .into_iter()
        .filter(|s| s.row.kind == "worker")
        .map(|s| s.health.as_str().to_string())
        .collect();
    assert_eq!(workers, ["left"]);
}

#[test]
fn agent_start_is_tried_again_while_the_new_shell_is_not_ready() {
    let mut w = World::new();
    w.set("FAKE_HERDR_BUSY", "2");
    let s = w.launch("sim/jump").row;
    assert_eq!(w.calls_of(&["agent", "start"]).len(), 3);
    assert_eq!(w.sleeps.borrow()[..2], [2.0, 2.0]);
    assert_eq!(w.pane_ids().last(), Some(&s.pane));
}

#[test]
fn agent_start_gives_up_after_its_tries() {
    let mut w = World::new();
    w.set("FAKE_HERDR_BUSY", "99");
    let why = refused(w.try_launch(&w.ask("sim/jump")));
    assert!(why.contains("agent_pane_busy"), "{why}");
    assert_eq!(w.calls_of(&["agent", "start"]).len(), START_TRIES as usize);
    assert_eq!(w.pane_ids(), Vec::<String>::new());
}

// -- where the pane goes: a tab per stream (`code`, `review`), four panes at most, `code-2` past that

#[test]
fn a_launch_with_no_stream_tab_opens_it_named_for_the_role_in_the_worktree() {
    let w = World::new();
    let s = w.launch("sim/jump").row;
    let [create] = <[_; 1]>::try_from(w.calls_of(&["tab", "create"])).unwrap();
    assert_eq!(
        create[2..],
        [
            "--label",
            "code",
            "--cwd",
            &s.worktree,
            "--env",
            &format!("TASKS_SESSION={}", s.handle),
            "--no-focus"
        ]
    );
    assert_eq!(w.calls_of(&["pane", "split"]), Vec::<Vec<String>>::new());
    assert!(w.has_pane(&s.pane));
}

#[test]
fn a_reviewer_goes_to_the_review_tab_and_a_given_tab_overrides_the_role() {
    let w = World::new();
    w.review("sim/jump");
    w.task("sim/dock", None);
    let ask = Ask {
        tab: Some("3d".into()),
        ..w.ask("sim/dock")
    };
    w.try_launch(&ask).unwrap();
    let labels: Vec<String> = w.state()["tabs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["label"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(labels, ["review", "3d"]);
}

#[test]
fn a_tab_with_room_is_split_instead() {
    let w = World::new();
    w.add_tab("w1:t5", "code", 3);
    let s = w.launch("sim/jump").row;
    assert_eq!(w.calls_of(&["tab", "create"]), Vec::<Vec<String>>::new());
    let [split] = <[_; 1]>::try_from(w.calls_of(&["pane", "split"])).unwrap();
    assert_eq!(split[2..5], ["w1:t5-p0", "--direction", "down"]);
    let state = w.state();
    let last = state["panes"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(
        (last["tab_id"].as_str(), last["pane_id"].as_str()),
        (Some("w1:t5"), Some(s.pane.as_str()))
    );
}

#[test]
fn a_tab_with_one_pane_is_split_to_the_right() {
    let w = World::new();
    w.add_tab("w1:t5", "code", 1);
    w.launch("sim/jump");
    assert_eq!(
        w.calls_of(&["pane", "split"])[0][3..5],
        ["--direction", "right"]
    );
}

#[test]
fn short_or_unknown_panes_open_the_next_stream_tab_without_splitting() {
    for rect in [
        json!({"width":200,"height":12}),
        json!({"width":160,"height":24}), // One column short after a right split.
        json!({"width":200}),
        json!({"width":"200","height":80}),
        Value::Null,
    ] {
        let w = World::new();
        w.add_tab("w1:t5", "code", 1);
        let mut state = w.state();
        state["panes"][0]["rect"] = rect;
        w.set_state(state);
        let s = w.launch("sim/jump").row;
        assert!(w.calls_of(&["pane", "split"]).is_empty());
        assert_eq!(
            flag_value(&w.calls_of(&["tab", "create"])[0], "--label"),
            "code-2"
        );
        assert_eq!(
            w.calls_of(&["pane", "layout"])[0][2..],
            ["--pane", "w1:t5-p0"]
        );
        assert!(w.has_pane(&s.pane));
    }
}

#[test]
fn split_direction_and_target_preserve_room_for_the_full_chooser() {
    for (panes, rects, target, direction) in [
        (1, vec![(80, 49)], "w1:t5-p0", "down"),
        (1, vec![(161, 24)], "w1:t5-p0", "right"),
        (3, vec![(200, 24); 3], "w1:t5-p0", "right"),
        (3, vec![(80, 12), (200, 80), (80, 12)], "w1:t5-p1", "down"),
    ] {
        let w = World::new();
        w.add_tab("w1:t5", "code", panes);
        let mut state = w.state();
        for (i, (width, height)) in rects.into_iter().enumerate() {
            state["panes"][i]["rect"] = json!({"width":width,"height":height});
        }
        w.set_state(state);
        w.launch("sim/jump");
        assert!(w.calls_of(&["tab", "create"]).is_empty());
        let [split] = <[_; 1]>::try_from(w.calls_of(&["pane", "split"])).unwrap();
        assert_eq!(split[2..5], [target, "--direction", direction]);
    }
}

#[test]
fn unreadable_layout_opens_the_next_tab_and_the_owner_tab_is_never_used() {
    let mut w = World::new();
    w.add_tab("w1:t5", "code", 1);
    w.set("FAKE_HERDR_FAIL", "pane layout");
    w.launch("sim/jump");
    assert!(w.calls_of(&["pane", "split"]).is_empty());
    assert_eq!(
        flag_value(&w.calls_of(&["tab", "create"])[0], "--label"),
        "code-2"
    );

    let w = World::new();
    w.add_tab("w1:t1", "code", 1);
    w.launch("sim/jump");
    assert!(w.calls_of(&["pane", "split"]).is_empty());
    assert!(w.calls_of(&["pane", "layout"]).is_empty());
    assert_eq!(
        flag_value(&w.calls_of(&["tab", "create"])[0], "--label"),
        "code-2"
    );
}

#[test]
fn a_full_stream_gets_a_second_tab_named_with_a_number_filled_before_a_third_opens() {
    let w = World::new();
    w.add_tab("w1:t5", "code", 4);
    w.launch("sim/jump");
    let create = &w.calls_of(&["tab", "create"])[0];
    assert_eq!(flag_value(create, "--label"), "code-2");

    let w = World::new();
    w.add_tab("w1:t5", "code", 4);
    w.add_tab("w1:t6", "code-2", 2);
    w.launch("sim/jump");
    assert_eq!(w.calls_of(&["tab", "create"]), Vec::<Vec<String>>::new());
    assert_eq!(w.calls_of(&["pane", "split"])[0][2], "w1:t6-p0");

    let w = World::new();
    w.add_tab("w1:t6", "code-2", 4);
    w.add_tab("w1:t5", "code", 4);
    w.launch("sim/jump");
    assert_eq!(
        flag_value(&w.calls_of(&["tab", "create"])[0], "--label"),
        "code-3"
    );
}

#[test]
fn tabs_of_other_streams_are_never_filled() {
    let w = World::new();
    w.add_tab("w1:t5", "infra", 1);
    w.add_tab("w1:t6", "codex", 1);
    w.add_tab("w1:t7", "code-x", 1);
    w.launch("sim/jump");
    assert_eq!(w.calls_of(&["pane", "split"]), Vec::<Vec<String>>::new());
    assert_eq!(w.calls_of(&["tab", "create"]).len(), 1);
}

// -- the prompt, taken or still sitting in the input line (umber-ferret-929, 2026-10-08)

#[test]
fn a_prompt_the_agent_takes_is_looked_at_once_and_left() {
    let w = World::new();
    let s = w.launch("sim/jump");
    assert!(!s.stuck);
    let calls = w.calls();
    let prompt = calls
        .iter()
        .position(|c| c[..2] == ["agent", "prompt"])
        .unwrap();
    assert_eq!(
        calls[prompt + 1..],
        [vec!["agent".to_string(), "list".to_string()]]
    );
    assert_eq!(*w.sleeps.borrow(), [PROMPT_WAIT]);
    assert_eq!(
        w.calls_of(&["pane", "send-keys"]),
        Vec::<Vec<String>>::new()
    );
}

#[test]
fn a_prompt_left_in_the_input_line_is_sent_with_enter() {
    let mut w = World::new();
    w.set("FAKE_HERDR_STUCK", "1");
    let s = w.launch("sim/jump");
    assert!(!s.stuck);
    let pane = s.row.pane.clone();
    let after: Vec<Vec<String>> = {
        let calls = w.calls();
        let prompt = calls
            .iter()
            .position(|c| c[..2] == ["agent", "prompt"])
            .unwrap();
        calls[prompt + 1..].to_vec()
    };
    let expected: Vec<Vec<String>> = [
        vec!["agent", "list"],
        vec!["pane", "read", &pane, "--lines", "12"],
        vec!["pane", "send-keys", &pane, "enter"],
        vec!["agent", "list"],
    ]
    .iter()
    .map(|c| c.iter().map(|s| s.to_string()).collect())
    .collect();
    assert_eq!(after, expected);
    assert_eq!(*w.sleeps.borrow(), [PROMPT_WAIT, PROMPT_WAIT]);
    let state = w.state();
    let shown = state["panes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["pane_id"] == pane.as_str())
        .unwrap()
        .clone();
    assert_eq!(
        (shown["agent_status"].as_str(), shown["input"].as_str()),
        (Some("working"), Some(""))
    );
}

/// A launch of `sim/jump` through `herdr`.
fn launch_through(w: &World, herdr: &Hooked<'_>) -> tasks::launch::Started {
    tasks::launch::launch(w.store(), herdr, &w.machine(), &w.ask("sim/jump")).unwrap()
}

/// A failed confirmation must undo every resource the new launch owned.
fn refused_launch(w: &World, result: Result<tasks::launch::Started, Error>) -> String {
    let why = refused(result);
    let [start] = <[_; 1]>::try_from(w.calls_of(&["agent", "start"])).unwrap();
    let pane = flag_value(&start, "--pane").to_string();
    assert_eq!(
        why,
        format!("could not confirm that the agent in pane {pane} took its prompt")
    );
    assert!(!w.has_pane(&pane));
    assert_eq!(w.calls_of(&["pane", "close", &pane]).len(), 1);
    assert_eq!(w.store().task_sessions("sim/jump").unwrap(), vec![]);
    assert!(!w.wt.join("sim-jump").exists());
    let workers: Vec<String> = w
        .store()
        .sessions()
        .unwrap()
        .into_iter()
        .filter(|s| s.row.kind == "worker")
        .map(|s| s.health.as_str().to_string())
        .collect();
    assert_eq!(workers, ["left"]);
    pane
}

#[test]
fn a_prompt_still_there_after_the_last_enter_refuses_the_launch_and_cleans_up() {
    let mut w = World::new();
    w.set("FAKE_HERDR_STUCK", "1");
    let enters = Cell::new(0);
    let herdr = Hooked {
        // An enter that sends nothing: the text stays where it is.
        instead: Box::new(|args| {
            (args[..2] == ["pane", "send-keys"]).then(|| {
                enters.set(enters.get() + 1);
                Ok("{}".to_string())
            })
        }),
        ..Hooked::new(&w)
    };
    let pane = refused_launch(
        &w,
        tasks::launch::launch(w.store(), &herdr, &w.machine(), &w.ask("sim/jump")),
    );
    assert_eq!(enters.get(), PROMPT_ENTERS);
    assert_eq!(
        w.calls_of(&["pane", "read", &pane]).len(),
        PROMPT_ENTERS + 1
    );
    assert_eq!(w.sleeps.borrow().len(), PROMPT_ENTERS + 1);
}
#[test]
fn an_enter_herdr_refuses_fails_the_launch_without_recording_a_session() {
    let mut w = World::new();
    w.set("FAKE_HERDR_STUCK", "1");
    w.set("FAKE_HERDR_FAIL", "pane send-keys");
    refused_launch(&w, w.try_launch(&w.ask("sim/jump")));
    assert_eq!(w.calls_of(&["pane", "send-keys"]).len(), 1);
}
#[test]
fn missing_prompt_text_or_unreadable_observations_cannot_confirm_either_agent() {
    for kind in ["claude", "codex"] {
        for fail in [
            "agent list",
            "pane read",
            "empty pane",
            "frozen pane",
            "other input",
        ] {
            let mut w = World::new();
            w.set("FAKE_HERDR_STUCK", "1");
            w.set("FAKE_HERDR_FAIL", fail);
            let herdr = Hooked {
                instead: Box::new(move |args| {
                    if args[..2] != ["pane", "read"] {
                        return None;
                    }
                    let screen = match fail {
                        "empty pane" => "",
                        "frozen pane" => "Claude\n0 tokens\n> ",
                        "other input" => "> unrelated text",
                        _ => return None,
                    };
                    Some(Ok(screen.into()))
                }),
                after: Box::new(|args| {
                    if args[..2] == ["agent", "prompt"] {
                        // A conversation id alone is not evidence that a turn began.
                        w.edit_panes(|panes| {
                            panes.last_mut().unwrap()["agent_session"] = json!({
                                "agent": kind, "kind": "id", "value": "ready-thread"
                            });
                        });
                    }
                }),
                ..Hooked::new(&w)
            };
            let ask = Ask {
                kind: Some(kind.into()),
                ..w.ask("sim/jump")
            };
            refused_launch(
                &w,
                tasks::launch::launch(w.store(), &herdr, &w.machine(), &ask),
            );
            assert_eq!(w.calls_of(&["agent", "prompt"]).len(), 1, "{kind}: {fail}");
            assert_eq!(
                w.calls_of(&["agent", "list"]).len(),
                PROMPT_ENTERS + 1,
                "{kind}: {fail}"
            );
            assert_eq!(*w.sleeps.borrow(), vec![PROMPT_WAIT; PROMPT_ENTERS + 1]);
            assert_eq!(
                w.calls_of(&["pane", "send-keys"]),
                Vec::<Vec<String>>::new(),
                "{kind}: {fail}"
            );
        }
    }
}
#[test]
fn delayed_activity_after_ambiguous_looks_confirms_the_launch_without_another_enter() {
    for kind in ["claude", "codex"] {
        for (observation, ready) in [
            ("absent head", 2),
            ("agent list", 2),
            ("pane read", 2),
            ("missing agent", 2),
            ("absent head", PROMPT_ENTERS + 1),
        ] {
            let w = World::new();
            let looks = Cell::new(0);
            let inner = w.herdr();
            let herdr = Hooked {
                instead: Box::new(|args| {
                    if args == ["agent", "list"] {
                        looks.set(looks.get() + 1);
                        let said = inner.run(args);
                        if looks.get() >= ready {
                            return Some(said);
                        }
                        return Some(match observation {
                            "agent list" => Err(LaunchError("transient list failure".into())),
                            "missing agent" => Ok(r#"{"result":{"agents":[]}}"#.into()),
                            _ => said.map(|said| {
                                let mut said: Value = serde_json::from_str(&said).unwrap();
                                for agent in said["result"]["agents"].as_array_mut().unwrap() {
                                    agent["agent_status"] = if kind == "claude" {
                                        "idle".into()
                                    } else {
                                        "unknown".into()
                                    };
                                }
                                said.to_string()
                            }),
                        });
                    }
                    if args[..2] == ["pane", "read"] {
                        inner.run(args).unwrap();
                        return Some(if observation == "pane read" {
                            Err(LaunchError("transient pane read failure".into()))
                        } else {
                            Ok("Claude\n0 tokens\n> ".into())
                        });
                    }
                    None
                }),
                ..Hooked::new(&w)
            };
            let ask = Ask {
                kind: Some(kind.into()),
                role: "reviewer".into(),
                prompt: "a long reviewer prompt that wraps beyond twelve lines ".repeat(80),
                ..w.ask("sim/jump")
            };
            let started = tasks::launch::launch(w.store(), &herdr, &w.machine(), &ask)
                .unwrap_or_else(|why| panic!("{kind}: {observation}, look {ready}: {why}"));
            assert!(!started.stuck);
            assert!(w.has_pane(&started.row.pane));
            assert!(Path::new(&started.row.worktree).is_dir());
            assert_eq!(w.store().task_sessions("sim/jump").unwrap(), [started.row]);
            assert_eq!(w.calls_of(&["agent", "list"]).len(), ready);
            assert_eq!(*w.sleeps.borrow(), vec![PROMPT_WAIT; ready]);
            assert_eq!(w.calls_of(&["agent", "prompt"]).len(), 1);
            assert!(w.calls_of(&["pane", "send-keys"]).is_empty());
            assert!(w.calls_of(&["pane", "close"]).is_empty());
        }
    }
}

#[test]
fn an_agent_that_is_done_or_asks_something_has_taken_its_prompt() {
    for status in ["done", "blocked"] {
        let mut w = World::new();
        w.set("FAKE_HERDR_STUCK", "1");
        let inner = w.herdr();
        let herdr = Hooked {
            instead: Box::new(move |args| {
                (args == ["agent", "list"]).then(|| {
                    let said = inner.run(args)?;
                    let mut said: serde_json::Value = serde_json::from_str(&said).unwrap();
                    for agent in said["result"]["agents"].as_array_mut().unwrap() {
                        agent["agent_status"] = status.into();
                    }
                    Ok(said.to_string())
                })
            }),
            ..Hooked::new(&w)
        };
        let s = launch_through(&w, &herdr);
        assert!(!s.stuck, "{status}");
        assert_eq!(
            w.calls_of(&["pane", "read"]),
            Vec::<Vec<String>>::new(),
            "{status}"
        );
    }
}

#[test]
fn an_agent_herdr_no_longer_lists_cannot_leave_a_live_launch_record() {
    let mut w = World::new();
    w.set("FAKE_HERDR_STUCK", "1");
    let inner = w.herdr();
    let herdr = Hooked {
        instead: Box::new(|args| {
            (args == ["agent", "list"]).then(|| {
                inner.run(args).unwrap();
                Ok(r#"{"result": {"agents": []}}"#.to_string())
            })
        }),
        ..Hooked::new(&w)
    };
    refused_launch(
        &w,
        tasks::launch::launch(w.store(), &herdr, &w.machine(), &w.ask("sim/jump")),
    );
    assert_eq!(w.calls_of(&["agent", "list"]).len(), PROMPT_ENTERS + 1);
    assert_eq!(*w.sleeps.borrow(), vec![PROMPT_WAIT; PROMPT_ENTERS + 1]);
    assert_eq!(w.calls_of(&["pane", "read"]), Vec::<Vec<String>>::new());
}
