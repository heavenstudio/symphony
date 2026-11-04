//! `restore`: a closed session comes back in a new pane, in its worktree, resuming its conversation.

use crate::common::refused;
use crate::{Hooked, World, git};
use std::path::Path;
use tasks::launch::herdr::Herdr;
use tasks::launch::{Ask, PROMPT_ENTERS, PROMPT_WAIT, Started, restore, restore_unanswered};
use tasks::rows::TaskSessionRow;
use tasks::{Error, NewLaunch};

#[path = "restore_review.rs"]
mod review_instructions;

/// The fake discovers the id requested by Claude; Codex ids are supplied in its resume case.
fn launched(w: &World) -> Started {
    w.try_launch(&Ask {
        kind: Some("claude".into()),
        ..w.ask("sim/jump")
    })
    .unwrap()
}

/// Its pane goes, and the board records the closure.
fn close_pane(w: &World, s: &TaskSessionRow) {
    w.edit_panes(|panes| panes.retain(|p| p["pane_id"] != s.pane.as_str()));
    w.store()
        .close_task_session(&s.slug, &s.handle, "idle", "restore line")
        .unwrap();
}

fn restored(w: &World, slug: &str, handle: Option<&str>) -> Result<Vec<Started>, Error> {
    restore(w.store(), &w.herdr(), &w.machine(), slug, handle, None)
}

#[test]
fn recovery_resumes_once_per_post_for_closed_or_missing_open_conversations() {
    for closed in [false, true] {
        let w = World::new();
        let s = launched(&w).row;
        w.store().claim(&s.slug, &s.handle).unwrap();
        if closed {
            close_pane(&w, &s);
        } else {
            w.edit_panes(|panes| panes.clear());
        }
        let post = w
            .store()
            .say(&s.slug, "owner", "reconcile", Some("@executor"))
            .unwrap();
        let mut warnings = Vec::new();
        let recover = |store: &tasks::Store, warnings: &mut Vec<u8>| {
            restore_unanswered(store, &w.herdr(), &w.machine(), warnings).unwrap()
        };
        w.f.advance(29.0 * 60.0);
        assert!(recover(w.store(), &mut warnings).is_empty());
        w.f.advance(2.0 * 60.0);
        let lines = recover(w.store(), &mut warnings);
        assert_eq!(lines.len(), 1, "{warnings:?}");
        assert!(lines[0].contains(&format!("post #{}", post.id)));
        assert_eq!(w.calls_of(&["agent", "start"]).len(), 2);
        let current = w.store().task_sessions(&s.slug).unwrap().remove(0);
        assert_eq!(current.claude_session, s.claude_session);
        close_pane(&w, &current);
        assert!(
            recover(&w.f.other(), &mut warnings).is_empty(),
            "durable once-per-post claim"
        );
        w.store().say(&s.slug, &s.handle, "answered", None).unwrap();
        assert!(recover(w.store(), &mut warnings).is_empty());
        let next = w
            .store()
            .say(&s.slug, "owner", "new request", Some(&s.handle))
            .unwrap();
        w.f.advance(31.0 * 60.0);
        assert_eq!(recover(w.store(), &mut warnings).len(), 1);
        assert_eq!(w.calls_of(&["agent", "start"]).len(), 3);
        let recorded: Vec<_> = w
            .store()
            .thread(&s.slug)
            .unwrap()
            .into_iter()
            .filter(|p| p.sender == "board" && p.text.starts_with("restored "))
            .collect();
        assert_eq!(recorded.len(), 2);
        assert!(recorded[1].text.contains(&format!("post #{}", next.id)));
        assert!(warnings.is_empty(), "{warnings:?}");
    }
}

#[test]
fn a_departed_executor_replacement_resumes_without_reviving_its_predecessor() {
    for before_departure in [false, true] {
        let w = World::new();
        let a = launched(&w).row;
        w.store().claim(&a.slug, &a.handle).unwrap();
        w.store()
            .release(&a.slug, &a.handle, Some("executor"))
            .unwrap();
        close_pane(&w, &a);
        let b = w
            .try_launch(&Ask {
                kind: Some("claude".into()),
                worktree: Some(a.worktree.clone().into()),
                ..w.ask(&a.slug)
            })
            .unwrap()
            .row;
        w.store().claim(&b.slug, &b.handle).unwrap();
        w.store().move_to(&b.slug, "review", &b.handle).unwrap();
        assert_eq!(a.worktree, b.worktree);
        assert_ne!(a.handle, b.handle);
        if !before_departure {
            w.store().leave(&b.handle).unwrap();
        }
        let post = w
            .store()
            .say(&b.slug, "owner", "finish the fix", Some("@executor"))
            .unwrap();
        if before_departure {
            w.store().leave(&b.handle).unwrap();
        }
        close_pane(&w, &b);
        assert!(
            w.store()
                .get(&b.slug)
                .unwrap()
                .holders("executor")
                .is_empty()
        );
        assert_eq!(w.store().task_sessions(&b.slug).unwrap().len(), 2);
        w.f.advance(31.0 * 60.0);
        let digest = tasks::cli::pull::digest(w.store(), &[], None, w.f.now(), &[]).unwrap();
        let lines = digest.lines().join("\n");
        assert_eq!(digest.count("unanswered"), 1, "{lines}");
        assert!(
            lines.contains(&format!("just tasks restore {} {}", b.slug, b.handle)),
            "{lines}"
        );
        assert!(!lines.contains(&a.handle), "{lines}");
        let mut warnings = Vec::new();
        let recovered =
            restore_unanswered(w.store(), &w.herdr(), &w.machine(), &mut warnings).unwrap();
        assert_eq!(recovered.len(), 1, "{recovered:?} {warnings:?}");
        assert!(recovered[0].contains(&b.handle));
        assert!(recovered[0].contains(&format!("post #{}", post.id)));
        assert_eq!(w.calls_of(&["agent", "start"]).len(), 3);
        let sessions = w.store().task_sessions(&b.slug).unwrap();
        let old = sessions.iter().find(|s| s.handle == a.handle).unwrap();
        let resumed = sessions.iter().find(|s| s.handle == b.handle).unwrap();
        assert!(old.closed.is_some());
        assert!(resumed.closed.is_none());
        assert_eq!(resumed.claude_session, b.claude_session);
        close_pane(&w, resumed);
        assert!(
            restore_unanswered(&w.f.other(), &w.herdr(), &w.machine(), &mut warnings)
                .unwrap()
                .is_empty()
        );
        assert_eq!(w.calls_of(&["agent", "start"]).len(), 3);
        assert!(warnings.is_empty(), "{warnings:?}");
    }
}

#[test]
fn recovery_leaves_answered_present_and_unrecorded_conversations_alone() {
    for case in [
        "answered",
        "status-answer",
        "present",
        "unrecorded",
        "native",
        "native-open",
        "unknown-open",
    ] {
        let w = World::new();
        let s = launched(&w).row;
        w.store().claim(&s.slug, &s.handle).unwrap();
        let post = w
            .store()
            .say(&s.slug, "owner", "reconcile", Some(&s.handle))
            .unwrap();
        if case == "answered" {
            w.store().say(&s.slug, &s.handle, "answer", None).unwrap();
        }
        if case == "status-answer" {
            w.store().move_to(&s.slug, "blocked", &s.handle).unwrap();
        }
        if case.ends_with("-open") {
            w.edit_panes(|panes| panes.clear());
        } else if case != "present" {
            close_pane(&w, &s);
        }
        if case == "unrecorded" {
            w.store()
                .record_launch(&NewLaunch::new(&s.slug, &s.handle, &s.role))
                .unwrap();
        }
        w.f.advance(31.0 * 60.0);
        if case.starts_with("native") || case == "unknown-open" {
            w.store().client().query(
                "UPSERT type::record('callback_state', $handle) SET handle = $handle, address = 'codex:alive-thread', host = 'this-host', confirmed_until = $until, lease = $until RETURN NONE;",
                serde_json::json!({"handle":s.handle, "until":w.f.now()+if case == "unknown-open" { -1.0 } else { 390.0 }}),
            ).unwrap();
        }
        let mut warnings = Vec::new();
        assert!(
            restore_unanswered(w.store(), &w.herdr(), &w.machine(), &mut warnings)
                .unwrap()
                .is_empty(),
            "{case}: {warnings:?}"
        );
        assert_eq!(
            w.calls_of(&["agent", "start"]).len(),
            1,
            "{case}: post #{}",
            post.id
        );
        assert!(warnings.is_empty());
        let digest = tasks::cli::pull::digest(w.store(), &[], None, w.f.now(), &[]).unwrap();
        if case == "unrecorded" {
            assert!(
                digest
                    .lines()
                    .iter()
                    .any(|line| line.contains("just tasks launch sim/jump --role executor"))
            );
        }
        if case.ends_with("-open") {
            assert_eq!(digest.count("unanswered"), 0, "{case}");
        }
    }
}

#[test]
fn restore_resumes_the_recorded_session_in_the_recorded_worktree() {
    let w = World::new();
    let s = launched(&w).row;
    close_pane(&w, &s);
    let [again] = <[_; 1]>::try_from(restored(&w, "sim/jump", None).unwrap()).unwrap();
    assert!(!again.stuck);
    let start = w.calls_of(&["agent", "start"]).pop().unwrap();
    assert_eq!(start[2], s.handle);
    let rest = &start[start.iter().position(|a| a == "--").unwrap() + 1..];
    assert_eq!(
        rest,
        [
            "--resume",
            &s.claude_session,
            "-n",
            &s.handle,
            "--permission-mode",
            "auto",
            "--model",
            "sonnet"
        ]
    );
    assert_eq!(w.pane_ids().last(), Some(&again.row.pane));
    let [read] = <[_; 1]>::try_from(w.store().task_sessions("sim/jump").unwrap()).unwrap();
    assert_eq!(
        (read.closed, read.pane.as_str()),
        (None, again.row.pane.as_str())
    );
    assert_eq!(read, again.row);
}

#[test]
fn a_codex_conversation_resumes_with_its_id_and_handle_environment() {
    let w = World::new();
    let s = w.launch("sim/jump").row;
    let s = w
        .store()
        .record_launch(&NewLaunch {
            claude_session: "codex:thread-to-resume".into(),
            worktree: s.worktree,
            branch: s.branch,
            pane: s.pane,
            model: "chosen-model".into(),
            ..NewLaunch::new("sim/jump", &s.handle, &s.role)
        })
        .unwrap();
    close_pane(&w, &s);
    let [again] = <[_; 1]>::try_from(restored(&w, "sim/jump", None).unwrap()).unwrap();
    let start = w.calls_of(&["agent", "start"]).pop().unwrap();
    assert_eq!(start[2], s.handle);
    assert_eq!(start[3..5], ["--kind", "codex"]);
    let rest = &start[start.iter().position(|a| a == "--").unwrap() + 1..];
    assert_eq!(
        rest,
        [
            "-s",
            "workspace-write",
            "-m",
            "chosen-model",
            "-c",
            "model_reasoning_effort=\"xhigh\"",
            "-c",
            "sandbox_workspace_write.network_access=true",
            "-c",
            &format!(
                "shell_environment_policy.set.TASKS_SESSION=\"{}\"",
                s.handle
            ),
            "resume",
            "thread-to-resume",
        ]
    );
    assert_eq!(again.row.claude_session, "codex:thread-to-resume");
    assert_eq!(again.row.closed, None);
    assert_eq!(w.pane_ids(), [again.row.pane]);
}

#[test]
fn the_restored_pane_opens_in_the_worktree_in_its_streams_tab() {
    let w = World::new();
    let s = launched(&w).row;
    close_pane(&w, &s);
    restored(&w, "sim/jump", None).unwrap();
    let state = w.state();
    let last = state["panes"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(last["cwd"].as_str(), Some(s.worktree.as_str()));
    // the stream's first tab is empty now, so a pane of it opens the next one
    let create = w.calls_of(&["tab", "create"]).pop().unwrap();
    assert_eq!(create[2..4], ["--label", "code-2"]);
}

#[test]
fn the_restored_agent_is_told_what_happened() {
    let w = World::new();
    let s = launched(&w).row;
    close_pane(&w, &s);
    restored(&w, "sim/jump", None).unwrap();
    let state = w.state();
    let last = state["prompts"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(last[0].as_str(), Some(s.handle.as_str()));
    assert_eq!(
        last[1].as_str(),
        Some(format!(
            "You were restored after your pane was closed. First run `just tasks callback --as {h}`, then `just tasks inbox --as {h}` and continue /work sim/jump.", h = s.handle
        ).as_str())
    );
}

#[test]
fn a_restored_prompt_left_in_the_input_line_is_sent_with_enter() {
    let mut w = World::new();
    let s = launched(&w).row;
    close_pane(&w, &s);
    w.set("FAKE_HERDR_STUCK", "1");
    let [again] = <[_; 1]>::try_from(restored(&w, "sim/jump", None).unwrap()).unwrap();
    assert!(!again.stuck);
    assert_eq!(
        w.calls_of(&["pane", "send-keys", &again.row.pane, "enter"])
            .len(),
        1
    );
}

#[test]
fn an_open_session_is_not_restored() {
    let w = World::new();
    launched(&w);
    assert_eq!(restored(&w, "sim/jump", None).unwrap(), vec![]);
    assert_eq!(w.calls_of(&["agent", "start"]).len(), 1);
}

#[test]
fn only_the_named_handle_is_restored_and_every_closed_one_by_default() {
    let w = World::new();
    let a = launched(&w).row;
    let b = w.review("sim/jump").row;
    close_pane(&w, &a);
    close_pane(&w, &b);
    let [again] = <[_; 1]>::try_from(restored(&w, "sim/jump", Some(&b.handle)).unwrap()).unwrap();
    assert_eq!(again.row.handle, b.handle);
    let open: Vec<String> = w
        .store()
        .task_sessions("sim/jump")
        .unwrap()
        .into_iter()
        .filter(|s| s.closed.is_none())
        .map(|s| s.handle)
        .collect();
    assert_eq!(open, std::slice::from_ref(&b.handle));

    let w = World::new();
    let a = launched(&w).row;
    let b = w.review("sim/jump").row;
    close_pane(&w, &a);
    close_pane(&w, &b);
    let mut handles: Vec<String> = restored(&w, "sim/jump", None)
        .unwrap()
        .into_iter()
        .map(|s| s.row.handle)
        .collect();
    handles.sort();
    let mut both = vec![a.handle, b.handle];
    both.sort();
    assert_eq!(handles, both);
}

#[test]
fn a_missing_worktree_is_made_again_on_its_branch_or_detached_without_one() {
    let w = World::new();
    let s = launched(&w).row;
    close_pane(&w, &s);
    git(&w.repo, &["worktree", "remove", "--force", &s.worktree]);
    restored(&w, "sim/jump", None).unwrap();
    assert_eq!(
        git(
            Path::new(&s.worktree),
            &["rev-parse", "--abbrev-ref", "HEAD"]
        ),
        "sim/jump"
    );

    let w = World::new();
    let s = w.review("sim/jump").row;
    close_pane(&w, &s);
    git(&w.repo, &["worktree", "remove", "--force", &s.worktree]);
    restored(&w, "sim/jump", None).unwrap();
    assert_eq!(
        git(
            Path::new(&s.worktree),
            &["rev-parse", "--abbrev-ref", "HEAD"]
        ),
        "HEAD"
    );
}

#[test]
fn a_handle_that_never_ran_on_the_task_or_a_task_never_launched_is_refused() {
    let w = World::new();
    assert_eq!(
        refused(restored(&w, "sim/jump", Some("ghost-owl"))),
        "ghost-owl has no recorded session on sim/jump"
    );
    assert_eq!(
        refused(restored(&w, "sim/jump", None)),
        "sim/jump has no recorded sessions: it was not launched with `just tasks launch`"
    );
    launched(&w);
    assert_eq!(
        refused(restored(&w, "sim/jump", Some("ghost-owl"))),
        "ghost-owl has no recorded session on sim/jump"
    );
}

#[test]
fn a_restore_without_a_recorded_conversation_is_refused() {
    for identity in ["", "codex:"] {
        let w = World::new();
        let s = launched(&w).row;
        w.store()
            .close_task_session("sim/jump", &s.handle, "x", "r")
            .unwrap();
        w.store()
            .record_launch(&NewLaunch {
                claude_session: identity.into(),
                worktree: s.worktree.clone(),
                branch: s.branch.clone(),
                pane: s.pane.clone(),
                model: s.model.clone(),
                ..NewLaunch::new("sim/jump", &s.handle, &s.role)
            })
            .unwrap();
        w.store()
            .close_task_session("sim/jump", &s.handle, "x", "r")
            .unwrap();
        assert_eq!(
            refused(restored(&w, "sim/jump", None)),
            format!(
                "{} has no recorded conversation id: its conversation cannot be resumed (start it again with \
             `just tasks launch`)",
                s.handle
            )
        );
        assert_eq!(w.calls_of(&["agent", "start"]).len(), 1);
    }
}

#[test]
fn a_restored_handle_that_had_left_can_act_again() {
    let w = World::new();
    let s = launched(&w).row;
    w.store().leave(&s.handle).unwrap();
    close_pane(&w, &s);
    restored(&w, "sim/jump", None).unwrap();
    w.store().touch(&s.handle).unwrap();
    assert_eq!(w.f.health(&s.handle).as_str(), "live");
}

#[test]
fn a_failed_start_closes_the_new_pane_and_reopens_nothing() {
    let mut w = World::new();
    let s = launched(&w).row;
    close_pane(&w, &s);
    w.set("FAKE_HERDR_FAIL", "agent start");
    let why = refused(restored(&w, "sim/jump", None));
    assert!(why.starts_with("herdr agent start failed: "), "{why}");
    assert_eq!(w.pane_ids(), Vec::<String>::new());
    assert!(
        w.store().task_sessions("sim/jump").unwrap()[0]
            .closed
            .is_some()
    );
}

#[test]
fn an_unconfirmed_restore_closes_its_new_pane_and_keeps_the_record_closed() {
    for fail in [
        "agent list",
        "pane read",
        "empty pane",
        "missing agent",
        "pane send-keys",
    ] {
        let mut w = World::new();
        let s = launched(&w).row;
        close_pane(&w, &s);
        w.store().leave(&s.handle).unwrap();
        let before = w.store().task_sessions("sim/jump").unwrap();
        w.set("FAKE_HERDR_STUCK", "1");
        w.set("FAKE_HERDR_FAIL", fail);
        let inner = w.herdr();
        let herdr = Hooked {
            instead: Box::new(move |args| {
                if fail == "missing agent" && args == ["agent", "list"] {
                    inner.run(args).unwrap();
                    return Some(Ok(r#"{"result": {"agents": []}}"#.into()));
                }
                (fail == "empty pane" && args[..2] == ["pane", "read"]).then(|| Ok(String::new()))
            }),
            ..Hooked::new(&w)
        };
        let why = refused(restore(
            w.store(),
            &herdr,
            &w.machine(),
            "sim/jump",
            Some(&s.handle),
            None,
        ));
        let start = w.calls_of(&["agent", "start"]).pop().unwrap();
        let pane = &start[start.iter().position(|arg| arg == "--pane").unwrap() + 1];
        assert_eq!(
            why,
            format!("could not confirm that the agent in pane {pane} took its prompt"),
            "{fail}"
        );
        assert_eq!(w.calls_of(&["agent", "prompt"]).len(), 2, "{fail}");
        let looks = if fail == "pane send-keys" {
            1
        } else {
            PROMPT_ENTERS + 1
        };
        // The original launch took one look; restore must exhaust its own observation budget.
        assert_eq!(w.calls_of(&["agent", "list"]).len(), 1 + looks, "{fail}");
        assert_eq!(*w.sleeps.borrow(), vec![PROMPT_WAIT; 1 + looks]);
        assert_eq!(w.calls_of(&["pane", "close", pane]).len(), 1, "{fail}");
        assert_eq!(w.pane_ids(), Vec::<String>::new(), "{fail}");
        assert_eq!(
            w.store().task_sessions("sim/jump").unwrap(),
            before,
            "{fail}"
        );
        assert_eq!(w.f.health(&s.handle).as_str(), "left", "{fail}");
    }
}
