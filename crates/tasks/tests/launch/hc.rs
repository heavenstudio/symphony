//! The hc client (`Binary`, `Hc`'s calls) against the fake as a real binary, and the fake itself.

use crate::{FAKE, Hooked, SPACE, World};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::time::Duration;
use symphony_board::cli::Env;
use symphony_board::launch::LaunchError;
use symphony_board::launch::hc::{Binary, ENDING_VAR, Hc, board_of, is_not_found, text_of};
use symphony_board::sys::Command;

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
        (
            "echo not-json",
            "hc ls printed no JSON: expected ident at line 1 column 2",
        ),
        (
            "true",
            "hc ls printed no JSON: EOF while parsing a value at line 1 column 0",
        ),
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
fn a_kill_that_leaves_its_terminal_live_a_while_waits_for_it_to_finish_before_the_remove() {
    let w = World::new();
    // Its program exits at the third look after the kill; hc refuses to remove it until then.
    let slow = w.add_terminal(true, None, json!({"ending": 3}));
    w.hc().close(&slow).unwrap();
    assert!(w.addresses().is_empty());
    let changes: Vec<String> = w
        .calls()
        .into_iter()
        .filter(|c| c[0] != "ls")
        .map(|c| c[..2].join(" "))
        .collect();
    let looked = format!("get {slow}");
    assert_eq!(
        changes,
        [
            looked.clone(),
            format!("kill {slow}"),
            looked.clone(),
            looked.clone(),
            looked,
            format!("rm {slow}")
        ]
    );
    // One hc forgets while it ends is closed already: nothing is left to remove.
    let gone = w.add_terminal(true, None, json!({"ending": 3}));
    let mut hooked = Hooked::new(&w);
    hooked.instead = Box::new(|args| {
        let forgot = args[0] == "get" && w.calls_of(&["kill"]).len() == 2;
        forgot.then(|| {
            Err(LaunchError(format!(
                "hc get {}: not_found: no terminal",
                args[1]
            )))
        })
    });
    hooked.close(&gone).unwrap();
    assert_eq!(w.calls_of(&["rm", &gone]), Vec::<Vec<String>>::new());
}

#[test]
fn a_killed_terminal_is_given_ten_seconds_to_end_unless_the_environment_says() {
    assert_eq!(binary(FAKE).ending(), Duration::from_secs(10));
    let mut w = World::new();
    assert_eq!(w.hc().ending(), Duration::from_secs(120), "a world's own");
    w.set(ENDING_VAR, "0.5");
    assert_eq!(w.hc().ending(), Duration::from_millis(500));
    assert_eq!(Hooked::new(&w).ending(), Duration::from_millis(500));
}

/// hc as the world runs it, giving a killed terminal a fifth of a second to end.
struct Brief<'a>(Hooked<'a>);

impl Hc for Brief<'_> {
    fn run(&self, args: &[&str]) -> Result<String, LaunchError> {
        self.0.run(args)
    }

    fn feed(&self, args: &[&str], input: &str) -> Result<String, LaunchError> {
        self.0.feed(args, input)
    }

    fn app(&self, args: &[&str]) -> Result<String, LaunchError> {
        self.0.app(args)
    }

    fn caller(&self) -> Option<String> {
        self.0.caller()
    }

    fn board(&self) -> String {
        self.0.board()
    }

    fn ending(&self) -> Duration {
        Duration::from_millis(200)
    }
}

#[test]
fn a_killed_terminal_is_never_removed_while_the_last_look_found_it_live_or_said_nothing() {
    let w = World::new();
    // Its program outlasts the wait: every look after the kill finds it live.
    let slow = w.add_terminal(true, None, json!({"ending": 1000}));
    let brief = Brief(Hooked::new(&w));
    brief.close(&slow).unwrap();
    assert_eq!(w.calls_of(&["rm", &slow]), Vec::<Vec<String>>::new());
    let last = w.calls().into_iter().rfind(|c| c[0] != "ls").unwrap();
    assert_eq!(last[..2].join(" "), format!("get {slow}"));
    assert!(w.calls_of(&["get", &slow]).len() > 1);
    assert!(w.has_terminal(&slow));
    // A later close that finds it ended removes it.
    w.edit_terminal(&slow, |t| {
        t.insert("ending".into(), json!(1));
    });
    brief.close(&slow).unwrap();
    assert!(!w.has_terminal(&slow));
    assert_eq!(w.calls_of(&["rm", &slow]).len(), 1);

    // A look that fails after the kill reads no state: the last one read, the kill's, said live.
    let unread = w.add_terminal(true, None, json!({"ending": 3}));
    let mut hooked = Hooked::new(&w);
    let (world, killed) = (&w, unread.clone());
    hooked.instead = Box::new(move |args| {
        let failed = args[0] == "get" && !world.calls_of(&["kill", &killed]).is_empty();
        failed.then(|| {
            Err(LaunchError(
                "hc get: unavailable: the core did not answer".into(),
            ))
        })
    });
    hooked.close(&unread).unwrap();
    assert_eq!(w.calls_of(&["kill", &unread]).len(), 1);
    assert_eq!(w.calls_of(&["rm", &unread]), Vec::<Vec<String>>::new());
    assert!(w.has_terminal(&unread));
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
        (json!("codex"), json!("idle"), json!(true))
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
    // Run from a terminal, as an agent there runs it, it is that terminal's child, which the window
    // shows under it: no tab.
    let tabs = |w: &World| {
        w.state()["layout"]["spaces"][1]["tabs"]
            .as_array()
            .unwrap()
            .len()
    };
    let from_terminal = symphony_board::sys::output(
        Command::new(FAKE)
            .args(["run", "--bg", "--label", "herdr.space=sp-new"])
            .args(["--label", "herdr.tab=tb-kid", "--json", "--", "zsh"])
            .env_clear()
            .envs(&w.env.vars)
            .env("HC_TERMINAL_ID", address),
    )
    .unwrap();
    let child: Value = serde_json::from_slice(&from_terminal.stdout).unwrap();
    let child = w.terminal(child["address"].as_str().unwrap());
    assert_eq!(child["parent"], address);
    assert_eq!(tabs(&w), 1);
    // The board's own call from that terminal hands hc none of its session: no child of it, even
    // without `--no-parent`, so the window puts it in its tab.
    let mut w = w;
    w.set("HC_TERMINAL_ID", address);
    let board = w.hc().json(&[
        "run",
        "--bg",
        "--label",
        "herdr.space=sp-new",
        "--label",
        "herdr.tab=tb-board",
        "--",
        "zsh",
    ]);
    let board = w.terminal(board.unwrap()["address"].as_str().unwrap());
    assert_eq!(board["parent"], json!(null));
    assert_eq!(tabs(&w), 2);
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
    // Forced, as hc, it types into the dialog.
    assert!(w.hc().json(&["send", &t, "1", "--force"]).is_ok());
}

#[test]
fn the_fakes_send_answers_as_hcs_whether_the_turn_started_and_the_status_it_went_in_at() {
    let w = World::new();
    let ready = w.add_terminal(true, None, json!({"turn": 4}));
    let deaf = w.add_terminal(true, None, json!({"deaf": true}));
    let shell = w.add_terminal(false, None, json!({}));
    for (t, started, now) in [
        (&ready, json!(true), json!("working")),
        (&deaf, json!(false), json!("idle")),
        (&shell, json!(null), json!(null)),
    ] {
        let before = w.terminal(t)["status"].clone();
        let turn = w.terminal(t)["turn"].clone();
        assert_eq!(
            w.hc().json(&["send", t, "go"]).unwrap(),
            json!({"started": started, "status": before, "turn": turn}),
            "{t}"
        );
        assert_eq!(
            (w.terminal(t)["status"].clone(), w.sent(t)),
            (now, vec!["go".to_string()])
        );
    }
}
