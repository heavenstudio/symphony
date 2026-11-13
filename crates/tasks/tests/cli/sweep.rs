//! `reap` and `clean` as commands, each its own process on a private board and the fake hc: what
//! they say they closed and why each terminal left open stays, the Symphony's log (the sweep's
//! stderr) told once per reason, and a sweep that finds another holding the lock.

use crate::Ran;
use crate::launch::{NO_PID, Terminals};
use serde_json::json;
use std::fs;
use symphony_board::launch::hc::board_of;
use symphony_board::launch::reap::lock_path;
use symphony_board::{NewLaunch, NewSession, NewTask};

// -- reap -------------------------------------------------------------------------------

#[test]
fn a_reap_closes_an_agent_idle_for_two_sweeps_where_tasks_reap_state_names_its_state() {
    let mut p = Terminals::new("reap");
    let state = p.dir.join("server-reap.json");
    p.set("TASKS_REAP_STATE", &state.display().to_string());
    let worker = p
        .board()
        .register(&NewSession::new("worker"))
        .unwrap()
        .row
        .handle;
    p.idle_terminal(&worker);
    let now = crate::common::NOW;
    let said = |ran: Ran| (ran.code, ran.out, ran.err);
    let once =
        format!("pane t1 of {worker} (idle once: it closes if still idle at the next sweep)");
    // A dry run says why it keeps each terminal; a sweep tells the Symphony's log (its stderr) once.
    assert_eq!(
        said(p.binary(now, &["reap", "--dry-run"])),
        (
            0,
            format!("keeps {once}\nnothing to close\n"),
            String::new()
        )
    );
    assert!(!state.exists(), "a dry run keeps no state");
    assert_eq!(
        said(p.binary(now, &["reap"])),
        (
            0,
            format!("idle once, closed if still idle next sweep: {worker}\n"),
            format!("· idle sweep keeps {once}\n")
        )
    );
    assert!(state.exists());
    let later = now + symphony_board::launch::reap::REAP_SECONDS;
    let finished = format!("pane t1 of {worker} (idle for two sweeps, its work finished)");
    assert_eq!(
        said(p.binary(later, &["reap", "--dry-run"])),
        (0, format!("would close {finished}\n"), String::new())
    );
    assert_eq!(
        said(p.binary(later, &["reap", "--dry-run", "--as", &worker])),
        (
            0,
            format!(
                "keeps pane t1 of {worker} (it is the session this sweep runs for)\nnothing to close\n"
            ),
            String::new()
        ),
        "never the caller's own"
    );
    assert_eq!(
        said(p.binary(later, &["reap"])),
        (0, format!("closed {finished}\n"), String::new())
    );
    assert_eq!(p.addresses(), Vec::<String>::new());
}

#[test]
fn a_reap_tells_the_log_why_it_keeps_an_idle_terminal_once_per_reason() {
    let mut p = Terminals::new("reap-told");
    p.set(
        "TASKS_REAP_STATE",
        &p.dir.join("reap.json").display().to_string(),
    );
    let board = p.board();
    let orch = p.orchestrator();
    let executor = board
        .register(&NewSession::new("worker"))
        .unwrap()
        .row
        .handle;
    board.claim("sim/jump", &executor).unwrap();
    p.idle_terminal(&executor);
    let now = crate::common::NOW;
    let held = format!(
        "· idle sweep keeps pane t1 of {executor} (it is executor of sim/jump, in progress: it closes once idle an hour)\n"
    );
    // sim/jump, filed through the store, has no orchestrator. orch waits once, new, which marks
    // sim/jump waiting and gives it nothing, and by the sweep it has stayed past the first live
    // line, listening: the sweep gives it sim/jump first, and says so; a dry run says what it
    // would give and gives nothing.
    let first = p.binary(now, &["wait", "--timeout", "0", "--as", &orch]);
    assert_eq!(first.code, 0, "{}", first.err);
    let stayed = now + symphony_board::health::LIVE_SECONDS + 1.0;
    let mut later = p.env.clone();
    later.vars.insert("TASKS_NOW".into(), stayed.to_string());
    symphony_board::cli::connect(&later)
        .unwrap()
        .touch(&orch)
        .unwrap();
    let dry = p.binary(stayed, &["reap", "--dry-run"]);
    assert!(
        (dry.out).starts_with(&format!(
            "would give sim/jump to {orch} (it waited for an orchestrator)\n"
        )),
        "{}",
        dry.out
    );
    assert!(
        board
            .get("sim/jump")
            .unwrap()
            .holders("orchestrator")
            .is_empty()
    );
    let ran = p.binary(stayed, &["reap"]);
    assert_eq!(
        (ran.code, ran.out, ran.err),
        (
            0,
            format!("gave sim/jump to {orch} (it waited for an orchestrator)\nnothing to close\n"),
            held.clone()
        )
    );
    let ran = p.binary(now + 300.0, &["reap"]);
    assert_eq!(
        (ran.code, ran.out.as_str(), ran.err.as_str()),
        (0, "nothing to close\n", ""),
        "said once, and nothing is left to give"
    );
    // Its reason changes: told again.
    board
        .say("sim/jump", &orch, "status?", Some(&executor))
        .unwrap();
    let ran = p.binary(now + 600.0, &["reap"]);
    assert!(
        ran.err.starts_with(&format!(
            "· idle sweep keeps pane t1 of {executor} (an addressed post #"
        )) && ran.err.ends_with(" on sim/jump waits for its answer)\n"),
        "{}",
        ran.err
    );
    assert_eq!(p.addresses(), ["t1"]);
}

// -- clean ------------------------------------------------------------------------------

#[test]
fn clean_runs_cleanup_then_one_sweep_and_says_why_each_terminal_it_leaves_open_stays() {
    let mut p = Terminals::new("clean");
    p.set(
        "TASKS_REAP_STATE",
        &p.dir.join("reap.json").display().to_string(),
    );
    let board = p.board();
    let orch = p.orchestrator();
    let session = |kind: &str| board.register(&NewSession::new(kind)).unwrap().row.handle;
    let [done, reviewer, chat, caller, busy] =
        ["worker", "reviewer", "chat", "worker", "worker"].map(session);
    // `done` and the caller each executed a task that was cancelled.
    for (slug, handle) in [("sim/dock", &done), ("sim/hop", &caller)] {
        board.create(&NewTask::new(slug, "x", &orch)).unwrap();
        board.claim(slug, handle).unwrap();
        board.move_to(slug, "cancelled", handle).unwrap();
    }
    // `busy` took sim/jump to review, which `reviewer` reviews.
    board.claim("sim/jump", &busy).unwrap();
    board.move_to("sim/jump", "review", &busy).unwrap();
    board.take_role("sim/jump", &reviewer, "reviewer").unwrap();
    let agent = |address: &str, handle: &str, status: &str| {
        json!({
            "address": address, "name": handle, "state": "live", "status": status,
            "agent": "claude-code", "pid": NO_PID, "cwd": "/somewhere", "turn": 3,
            "argv": ["claude", "--session-id", format!("c-{address}"), "-n", handle, "go"],
            "labels": {"symphony.board": board_of(&p.env), "symphony.handle": handle},
        })
    };
    let ended = json!({
        "address": "t7", "name": null, "state": "finished", "status": null, "agent": null,
        "pid": NO_PID, "cwd": "/somewhere", "turn": 0, "argv": ["zsh"],
        "labels": {"symphony.board": board_of(&p.env)},
    });
    p.set_state(json!({"terminals": [
        agent("t1", &done, "idle"),
        agent("t2", &reviewer, "idle"),
        agent("t3", &orch, "idle"),
        agent("t4", &chat, "idle"),
        agent("t5", &caller, "idle"),
        agent("t6", &busy, "working"),
        ended,
    ], "layout": {"spaces": []}}));
    let lines = |verb: &str| {
        [
            format!(
                "{verb} pane t1 of {done} (its tasks are finished); restore: just tasks restore sim/dock {done}"
            ),
            format!("{verb} pane t7 (its program ended)"),
            format!(
                "kept pane t2 of {reviewer} (it is reviewer of sim/jump, in review: it closes once idle an hour)"
            ),
            format!("kept pane t3 of {orch} (an orchestrator never closes)"),
            format!("kept pane t4 of {chat} (a chat never closes)"),
            format!("kept pane t5 of {caller} (it is the session this sweep runs for)"),
            format!("kept pane t6 of {busy} (its agent is working)"),
            String::new(),
        ]
        .join("\n")
    };
    let now = crate::common::NOW;
    let dry = p.binary(now, &["clean", "--dry-run", "--as", &caller]);
    assert_eq!(
        (dry.code, dry.out, dry.err),
        (0, lines("would close"), String::new())
    );
    assert_eq!(p.addresses().len(), 7, "a dry run closes nothing");
    let ran = p.binary(now, &["clean", "--as", &caller]);
    assert_eq!(
        (ran.code, ran.out, ran.err),
        (0, lines("closed"), String::new())
    );
    assert_eq!(p.addresses(), ["t2", "t3", "t4", "t5", "t6"]);
}

#[test]
fn clean_says_why_an_ended_terminal_on_screen_stays_once_cleanup_closed_its_record() {
    let mut p = Terminals::new("clean-shown");
    p.set(
        "TASKS_REAP_STATE",
        &p.dir.join("reap.json").display().to_string(),
    );
    let board = p.board();
    p.orchestrator();
    let worker = board
        .register(&NewSession::new("worker"))
        .unwrap()
        .row
        .handle;
    board.claim("sim/jump", &worker).unwrap();
    board
        .record_launch(&NewLaunch {
            slug: "sim/jump".into(),
            handle: worker.clone(),
            role: "executor".into(),
            claude_session: "c".into(),
            pane: "t1".into(),
            ..NewLaunch::default()
        })
        .unwrap();
    // Its program ended, in the tab the window shows: cleanup closes its record, never the
    // terminal, which the sweep keeps, saying why.
    p.set_state(json!({
        "terminals": [{
            "address": "t1", "name": worker, "state": "finished", "status": null, "agent": null,
            "pid": NO_PID, "cwd": "/somewhere", "turn": 3, "argv": ["claude"],
            "labels": {
                "symphony.board": board_of(&p.env), "symphony.handle": worker, "herdr.tab": "tb-code",
            },
        }],
        "layout": {"spaces": []},
        "screen": {"tab": "tb-code", "agent": null, "overlays": [], "floats": {}, "errors": []},
    }));
    let lines = |verb: &str| {
        format!(
            "{verb} pane t1 of {worker} (its pane is gone); restore: just tasks restore sim/jump \
             {worker}\nkept pane t1 of {worker} (on screen in the Herdr app)\n"
        )
    };
    let now = crate::common::NOW;
    let dry = p.binary(now, &["clean", "--dry-run"]);
    assert_eq!(
        (dry.code, dry.out, dry.err),
        (0, lines("would close"), String::new())
    );
    let ran = p.binary(now, &["clean"]);
    assert_eq!(
        (ran.code, ran.out, ran.err),
        (0, lines("closed"), String::new())
    );
    assert_eq!(p.addresses(), ["t1"]);
    let [record] = <[_; 1]>::try_from(board.sessions_of(&worker).unwrap()).unwrap();
    assert!(record.closed.is_some(), "{record:?}");
}

#[test]
fn clean_with_no_terminals_says_there_is_nothing_to_clean_up() {
    let mut p = Terminals::new("clean-empty");
    p.set(
        "TASKS_REAP_STATE",
        &p.dir.join("reap.json").display().to_string(),
    );
    for args in [&["clean", "--dry-run"][..], &["clean"]] {
        let ran = p.binary(crate::common::NOW, args);
        assert_eq!(
            (ran.code, ran.out.as_str(), ran.err.as_str()),
            (0, "nothing to clean up\n", ""),
            "{args:?}"
        );
    }
}

// -- the lock ---------------------------------------------------------------------------

#[test]
fn a_reap_while_another_sweep_holds_the_lock_says_so() {
    let mut p = Terminals::new("reap-held");
    let state = p.dir.join("reap.json");
    p.set("TASKS_REAP_STATE", &state.display().to_string());
    let held = fs::File::create(lock_path(&state)).unwrap();
    held.try_lock().unwrap();
    let ran = p.binary(crate::common::NOW, &["reap"]);
    assert_eq!(
        (ran.code, ran.out.as_str(), ran.err.as_str()),
        (
            0,
            "another sweep is running (the Symphony's timer): nothing done\n",
            ""
        )
    );
    assert_eq!(p.calls(), Vec::<Vec<String>>::new());
}

#[test]
fn a_clean_while_another_sweep_holds_the_lock_still_cleans_up_and_says_it_swept_nothing() {
    let mut p = Terminals::new("clean-held");
    let state = p.dir.join("reap.json");
    p.set("TASKS_REAP_STATE", &state.display().to_string());
    let board = p.board();
    p.orchestrator();
    let worker = board
        .register(&NewSession::new("worker"))
        .unwrap()
        .row
        .handle;
    board.claim("sim/jump", &worker).unwrap();
    board.move_to("sim/jump", "cancelled", &worker).unwrap();
    p.idle_terminal(&worker);
    let held = fs::File::create(lock_path(&state)).unwrap();
    held.try_lock().unwrap();
    let ran = p.binary(crate::common::NOW, &["clean"]);
    let said = format!(
        "closed pane t1 of {worker} (its tasks are finished); restore: just tasks restore sim/jump \
         {worker}\nanother sweep is running (the Symphony's timer): no idle sweep now\n"
    );
    assert_eq!((ran.code, ran.out, ran.err), (0, said, String::new()));
    assert_eq!(p.addresses(), Vec::<String>::new());
}
