//! The hc client (`Binary`, `Hc`'s calls) against the fake as a real binary, and the fake itself.

use crate::{FAKE, Hooked, SPACE, World};
use serde_json::json;
use std::collections::BTreeMap;
use symphony_board::cli::Env;
use symphony_board::launch::LaunchError;
use symphony_board::launch::hc::{Binary, Hc, board_of, is_not_found, text_of};

fn binary(named: &str) -> Binary {
    let mut vars = BTreeMap::new();
    vars.insert("HC_BIN".to_string(), named.to_string());
    Binary::of(&Env {
        vars,
        cwd: std::env::temp_dir(),
    })
}

fn addresses(found: &[symphony_board::launch::hc::Object]) -> Vec<String> {
    found.iter().map(|t| text_of(t, "address")).collect()
}

#[test]
fn ls_lists_live_terminals_and_the_boards_own_with_finished_ones() {
    let w = World::new();
    let ours = w.add_terminal(true, Some("amber-heron"), json!({}));
    let ended = w.add_terminal(true, None, json!({"state": "finished"}));
    let other = w.add_terminal(
        true,
        None,
        json!({"labels": {"sf.board": "web-bench/tasks"}}),
    );
    assert_eq!(addresses(&w.hc().live().unwrap()), [ours.clone(), other]);
    assert_eq!(addresses(&w.hc().terminals().unwrap()), [ours, ended]);
    assert_eq!(
        w.calls_of(&["ls"]),
        [
            vec!["ls", "--json"],
            vec![
                "ls",
                "--all",
                "--label",
                "sf.board=lamp-works/tasks",
                "--json"
            ],
        ]
    );
}

#[test]
fn a_failing_call_names_the_call_and_hcs_error_kind() {
    let mut w = World::new();
    w.set("FAKE_HC_FAIL", "ls");
    assert_eq!(
        w.hc().live(),
        Err(LaunchError("hc ls: fake_failure: ls failed".into()))
    );
    let gone = w.hc().json(&["kill", "t404"]).unwrap_err();
    assert_eq!(gone.0, "hc kill t404: not_found: no terminal \"t404\"");
    assert!(is_not_found(&gone));
}

#[test]
fn a_missing_or_unsplittable_binary_is_a_launch_error() {
    let missing = binary("/nonexistent/hc").live();
    assert!(
        matches!(&missing, Err(LaunchError(why)) if why.starts_with("hc ls: ")),
        "{missing:?}"
    );
    assert_eq!(
        binary("'unclosed").live(),
        Err(LaunchError("HC_BIN: No closing quotation".into()))
    );
    assert_eq!(
        binary("   ").live(),
        Err(LaunchError("HC_BIN names no command".into()))
    );
}

#[test]
fn the_binary_comes_from_hc_bin_split_as_a_shell_splits_it() {
    let w = World::new();
    let mut env = w.env.clone();
    env.vars
        .insert("HC_BIN".into(), format!("sh -c '\"$0\" \"$@\"' {FAKE}"));
    assert!(Binary::of(&env).live().unwrap().is_empty());
    assert_eq!(w.calls(), [["ls", "--json"].map(String::from).to_vec()]);
}

#[test]
fn an_answer_that_is_not_json_or_no_list_is_named() {
    for (named, why) in [
        ("echo not-json", "hc ls printed no JSON"),
        ("true", "hc ls printed no JSON"),
        ("sh -c 'echo {}'", "hc ls answered no list of terminals"),
        (
            r#"sh -c 'echo "{\"terminals\":[1]}"'"#,
            "hc ls listed a terminal that is no object",
        ),
        ("sh -c 'echo said >&2; exit 3'", "hc ls: said"),
        ("sh -c 'echo out; exit 3'", "hc ls: out"),
    ] {
        assert_eq!(
            binary(named).live(),
            Err(LaunchError(why.into())),
            "{named}"
        );
    }
}

#[test]
fn close_kills_a_live_terminal_then_removes_it_and_a_gone_one_is_already_closed() {
    let w = World::new();
    let live = w.add_terminal(true, None, json!({}));
    let ended = w.add_terminal(true, None, json!({"state": "finished"}));
    w.hc().close(&live).unwrap();
    w.hc().close(&ended).unwrap();
    w.hc().close("t404").unwrap();
    assert!(w.addresses().is_empty());
    let changes: Vec<Vec<String>> = w
        .calls()
        .into_iter()
        .filter(|c| c[0] != "ls")
        .map(|c| c[..2].to_vec())
        .collect();
    assert_eq!(
        changes,
        [
            ["get", &live],
            ["kill", &live],
            ["rm", &live],
            ["get", &ended],
            ["rm", &ended],
            ["get", "t404"]
        ]
        .map(|c| c.map(String::from).to_vec())
    );
}

#[test]
fn a_terminal_hc_forgets_while_it_closes_is_closed() {
    let w = World::new();
    let live = w.add_terminal(true, None, json!({}));
    let ended = w.add_terminal(true, None, json!({"state": "finished"}));
    let mut hooked = Hooked::new(&w);
    hooked.instead = Box::new(|args| {
        let gone = |t: &str| LaunchError(format!("hc {} {t}: not_found: no terminal", args[0]));
        matches!(args[0], "kill" | "rm").then(|| Err(gone(args[1])))
    });
    hooked.close(&live).unwrap();
    hooked.close(&ended).unwrap();
    assert_eq!(w.calls_of(&["get"]).len(), 2);
}

#[test]
fn a_get_answered_with_no_object_or_a_rename_the_window_answers_with_an_error_is_named() {
    assert_eq!(
        binary(r#"sh -c 'echo "[1]"'"#).get("t1"),
        Err(LaunchError("hc get t1 answered no object".into()))
    );
    let w = World::new();
    let mut env = w.env.clone();
    env.vars.insert(
        "HERDR_APP_BIN".into(),
        r#"sh -c 'echo "{\"error\":\"no such space\"}"'"#.into(),
    );
    assert_eq!(
        Binary::of(&env).rename("space", "sp-x", "y"),
        Err(LaunchError(
            "herdr-app do space.rename: no such space".into()
        ))
    );
}

#[test]
fn a_close_hc_refuses_is_an_error() {
    let mut w = World::new();
    let live = w.add_terminal(true, None, json!({}));
    w.set("FAKE_HC_FAIL", "kill");
    assert_eq!(
        w.hc().close(&live),
        Err(LaunchError(format!(
            "hc kill {live}: fake_failure: kill failed"
        )))
    );
    assert_eq!(w.live(), [live]);
}

#[test]
fn get_answers_none_for_a_terminal_hc_does_not_know() {
    let w = World::new();
    let t = w.add_terminal(true, Some("amber-heron"), json!({}));
    let found = w.hc().get(&t).unwrap().unwrap();
    assert_eq!(
        found["status"],
        json!({"app": "claude-code", "state": "idle"})
    );
    assert_eq!(found["foreground"]["name"], "claude");
    assert_eq!(
        w.hc().get("amber-heron").unwrap().unwrap()["address"],
        t.as_str()
    );
    assert_eq!(w.hc().get("t404").unwrap(), None);
}

#[test]
fn the_layout_is_the_windows_else_the_document_the_core_keeps() {
    let mut w = World::new();
    let names = |w: &World| -> Vec<String> {
        w.hc()
            .layout()
            .unwrap()
            .iter()
            .map(|s| text_of(s, "name"))
            .collect()
    };
    assert_eq!(names(&w), ["lamp-works"]);
    assert!(w.calls_of(&["connect"]).is_empty());
    w.set("FAKE_HC_FAIL", "app ls");
    assert_eq!(names(&w), ["lamp-works"]);
    assert_eq!(w.calls_of(&["connect"]).len(), 1);
    w.set("FAKE_HC_FAIL", "app ls,connect");
    assert_eq!(
        w.hc().layout(),
        Err(LaunchError(
            "hc connect: fake_failure: connect failed".into()
        ))
    );
}

#[test]
fn a_layout_without_spaces_or_a_document_the_core_does_not_hold_is_named() {
    let w = World::new();
    let mut state = w.state();
    state["layout"] = json!({"spaces": "none"});
    w.set_state(state);
    assert_eq!(
        w.hc().layout(),
        Err(LaunchError(
            "the Herdr app's layout holds no list of spaces".into()
        ))
    );
    assert_eq!(
        w.calls_of(&["connect"]).len(),
        1,
        "the window's was no list"
    );
    let mut env = w.env.clone();
    env.vars.insert(
        "HC_BIN".into(),
        r#"sh -c 'cat >/dev/null; echo "{\"id\":2,\"result\":{\"value\":null}}"'"#.into(),
    );
    env.vars.insert("HERDR_APP_BIN".into(), "false".into());
    assert_eq!(
        Binary::of(&env).layout(),
        Err(LaunchError("the core holds no Herdr app document".into()))
    );
}

#[test]
fn a_rename_the_window_refuses_is_an_error() {
    let mut w = World::new();
    w.hc().rename("space", SPACE, "renamed").unwrap();
    assert_eq!(w.state()["layout"]["spaces"][0]["name"], "renamed");
    assert_eq!(
        w.hc().rename("tab", "tb-none", "x"),
        Err(LaunchError(
            "herdr-app do tab.rename: no tab \"tb-none\"".into()
        ))
    );
    w.set("FAKE_HC_FAIL", "app do");
    assert_eq!(
        w.hc().rename("space", SPACE, "again"),
        Err(LaunchError(
            "herdr-app do space.rename: the window did not answer".into()
        ))
    );
}

#[test]
fn the_caller_is_hc_terminal_id_and_the_board_is_the_project_and_its_database() {
    let mut w = World::new();
    assert_eq!(w.hc().caller(), None);
    w.set("HC_TERMINAL_ID", "");
    assert_eq!(w.hc().caller(), None);
    w.set("HC_TERMINAL_ID", "t7");
    assert_eq!(w.hc().caller().as_deref(), Some("t7"));
    assert_eq!(w.hc().board(), "lamp-works/tasks");
    w.set("TASKS_DATABASE", "scratch");
    assert_eq!(w.hc().board(), "lamp-works/scratch");
    // From a worktree, the project is still the main checkout's.
    let wt = w.dir().join("lamp-works-other");
    crate::git(
        &w.repo,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            &wt.display().to_string(),
        ],
    );
    let env = Env {
        vars: w.env.vars.clone(),
        cwd: wt,
    };
    assert_eq!(board_of(&env), "lamp-works/scratch");
}

#[test]
fn the_fake_runs_a_terminal_into_the_window_and_refuses_a_name_taken() {
    let w = World::new();
    let started = w
        .hc()
        .json(&[
            "run",
            "--bg",
            "--name",
            "amber-heron",
            "--cwd",
            "/x",
            "--label",
            "herdr.space=sp-new",
            "--label",
            "herdr.tab=tb-new",
            "--no-parent",
            "--outlive-parent",
            "--keep",
            "--no-notify",
            "--",
            "env",
            "A=1",
            "codex",
            "hi",
        ])
        .unwrap();
    let address = started["address"].as_str().unwrap();
    let t = w.terminal(address);
    assert_eq!(
        (t["agent"].clone(), t["status"].clone(), t["keep"].clone()),
        (json!("codex"), json!("working"), json!(true))
    );
    assert_eq!(t["outlive_parent"], true);
    assert_eq!(t["notify"], false);
    let spaces = &w.state()["layout"]["spaces"];
    assert_eq!(
        spaces[1],
        json!({"id": "sp-new", "name": "", "tabs": [{"id": "tb-new", "name": ""}]})
    );
    let again = w
        .hc()
        .json(&["run", "--bg", "--name", "amber-heron", "--", "zsh"]);
    assert_eq!(
        again,
        Err(LaunchError(
            "hc run: name_taken: amber-heron is taken".into()
        ))
    );
    // Run from a terminal, it is that terminal's child, which the window shows under it: no tab.
    let mut w = w;
    w.set("HC_TERMINAL_ID", address);
    let label = [
        "--label",
        "herdr.space=sp-new",
        "--label",
        "herdr.tab=tb-kid",
    ];
    let child = w
        .hc()
        .json(&[&["run", "--bg"], &label[..], &["--", "zsh"]].concat());
    let child = w.terminal(child.unwrap()["address"].as_str().unwrap());
    assert_eq!(child["parent"], address);
    assert_eq!(
        w.state()["layout"]["spaces"][1]["tabs"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn the_fake_keeps_a_live_terminal_from_rm_and_a_blocked_agent_from_send() {
    let w = World::new();
    let t = w.add_terminal(true, None, json!({"status": "blocked"}));
    assert_eq!(
        w.hc().json(&["rm", &t]),
        Err(LaunchError(format!(
            "hc rm {t}: live: {t} is still running"
        )))
    );
    assert_eq!(
        w.hc().send(&t, "hello"),
        Err(LaunchError(format!(
            "hc send {t}: blocked: it waits on a person"
        )))
    );
    assert_eq!(
        w.hc().json(&["attach", &t]),
        Err(LaunchError(format!(
            "hc attach {t}: unsupported: fake hc has no attach"
        )))
    );
}
