//! `tasks sweep` as the command line, in this process: what only the command line does (it asks
//! `lsof` itself, takes the window from a flag or the environment, refuses what it cannot read, holds
//! the lock and writes the stamp) and how a sweep work finishing started says itself.

use crate::children::Child as _;
use crate::common::Seed;
use crate::{HeldCwd, Repo, write};
use std::collections::BTreeMap;
use std::path::Path;
use symphony_board::cli::Env;
use symphony_board::sweep::trigger::{STAMP, folder};
use symphony_board::{NewLaunch, sweep::RECENT_ENV};

/// `tasks ARGS` from `cwd` on the repository's board, with `change` made to its environment: the exit
/// code, what it printed and what it said on stderr.
fn run(
    r: &Repo,
    cwd: &Path,
    args: &[&str],
    change: impl FnOnce(&mut BTreeMap<String, String>),
) -> (i32, String, String) {
    let mut vars = r.vars();
    change(&mut vars);
    let env = Env {
        vars,
        cwd: cwd.to_path_buf(),
    };
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = symphony_board::cli::main(&args, &env, &mut "".as_bytes(), &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

/// `tasks sweep ARGS` from the main checkout, every worktree idle.
fn sweep(r: &Repo, args: &[&str]) -> (i32, String, String) {
    r.idle(&[]);
    let mut all = vec!["sweep"];
    all.extend(args);
    run(r, &r.root, &all, |_| {})
}

fn stamp(r: &Repo) -> std::path::PathBuf {
    folder(&r.root.join(".git")).join(STAMP)
}

#[test]
fn a_dry_run_says_what_it_would_and_a_real_sweep_says_when_and_why_first_then_removes() {
    // The only tests that ask the real `lsof` are the command line's: both passes here do.
    let r = Repo::new();
    let wt = r.detached("reviewer");
    let held = symphony_board::sweep::size::shown(symphony_board::sweep::size::bytes(&wt));
    let (code, out, err) = sweep(&r, &["--dry-run"]);
    assert_eq!(code, 0, "{err}");
    assert!(wt.is_dir(), "{out}");
    assert!(
        out.contains(&format!(
            "worktree {}: {held}; no task; merged into main; would remove: detached at ",
            wt.display()
        )) && out.contains(&format!(
            "worktree {}: no task; merged into main; kept: the main checkout\n",
            r.root.display()
        )) && out.ends_with(&format!(
            "; would free {held} of the {held} its 1 worktree measured hold\n"
        )),
        "{out}"
    );
    // Only a sweep that removes writes the stamp the hourly sweep waits on.
    assert!(!stamp(&r).exists());
    // The sweep work finishing started says when and why first.
    let (code, out, err) = sweep(&r, &["--background", "sim/jump is done"]);
    assert_eq!(code, 0, "{err}");
    let first = out.lines().next().unwrap();
    assert!(
        first.starts_with('[') && first.ends_with("] sim/jump is done"),
        "{out}"
    );
    r.assert_removed(&wt, &out);
    assert!(
        out.ends_with(&format!(
            "worktree sweep: removed 1 of 2 worktrees, deleted 0 merged worktree-agent-* branches; \
             freed {held}\n"
        )),
        "{out}"
    );
    assert!(stamp(&r).is_file());
}

#[test]
fn busy_trees_with_escaped_names_are_kept_in_real_and_dry_sweeps() {
    let r = Repo::new();
    let mut busy = Vec::new();
    let mut children = Vec::new();
    for name in ["tab\tname", "café"] {
        let wt = r.detached(name);
        children.push(HeldCwd::at(&wt.join("sim")));
        busy.push(wt);
    }
    for args in [&["--dry-run"][..], &[]] {
        r.idle(&[]);
        let done = symphony_board::sys::output(
            symphony_board::sys::Command::of(env!("CARGO_BIN_EXE_tasks"))
                .arg("sweep")
                .args(args)
                .cleared()
                .envs(r.vars())
                .env("LC_ALL", "C")
                .env("TASKS_SWEEP", "off")
                .current_dir(&r.root),
        )
        .unwrap();
        assert!(
            done.status.success(),
            "{}",
            String::from_utf8_lossy(&done.stderr)
        );
        let out = String::from_utf8(done.stdout).unwrap();
        for wt in &busy {
            r.assert_kept(wt, &out);
        }
    }
    drop(children);
}

#[test]
fn inherited_repository_and_index_variables_do_not_redirect_sweep_operations() {
    for dirty_caller in [true, false] {
        let r = Repo::new();
        let wt = r.detached("candidate");
        let dirty = if dirty_caller { &r.root } else { &wt };
        write(
            dirty,
            "sim/world.gd",
            "extends RefCounted\n# keep my edit\n",
        );
        r.idle(&[]);
        let done = symphony_board::sys::output(
            symphony_board::sys::Command::of(env!("CARGO_BIN_EXE_tasks"))
                .arg("sweep")
                .cleared()
                .envs(r.vars())
                .env("TASKS_SWEEP", "off")
                .env("GIT_DIR", r.root.join(".git"))
                .env("GIT_WORK_TREE", &r.root)
                .env("GIT_COMMON_DIR", r.root.join(".git"))
                .env("GIT_INDEX_FILE", r.root.join(".git/index"))
                .env(
                    "GIT_OBJECT_DIRECTORY",
                    r.dir.join("not-an-object-directory"),
                )
                .env(
                    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
                    r.dir.join("not-an-alternate"),
                )
                .env("GIT_NAMESPACE", "inherited-namespace")
                .current_dir(&r.root),
        )
        .unwrap();
        assert!(
            done.status.success(),
            "{}",
            String::from_utf8_lossy(&done.stderr)
        );
        let out = String::from_utf8(done.stdout).unwrap();
        if dirty_caller {
            r.assert_removed(&wt, &out);
        } else {
            r.assert_kept(&wt, &out);
        }
        assert!(
            std::fs::read_to_string(dirty.join("sim/world.gd"))
                .unwrap()
                .contains("keep my edit")
        );
    }
}

#[test]
fn the_worktree_running_the_sweep_keeps_itself() {
    let r = Repo::new();
    let wt = r.detached("running");
    r.idle(&[]);
    let (code, out, err) = run(&r, &wt.join("sim"), &["sweep"], |_| {});
    assert_eq!(code, 0, "{err}");
    r.assert_kept(&wt, &out);
    assert!(out.contains("removed 0 of 2 worktrees"), "{out}");
}

/// The socket folder of a board no server answers: a connection to it is refused at once.
const UNREACHED: &str = "/nonexistent/symphony-unreached";

#[test]
fn a_board_it_cannot_reach_removes_nothing() {
    let r = Repo::new();
    let wt = r.detached("unknowable");
    r.idle(&[]);
    let (code, out, err) = run(&r, &r.root, &["sweep"], |vars| {
        vars.insert("TASKS_SERVER".into(), format!("postgresql:{UNREACHED}"));
    });
    assert_eq!(code, 2, "{out}");
    assert!(err.starts_with("✖ ") && err.contains(UNREACHED), "{err}");
    r.assert_kept(&wt, &out);
}

#[test]
fn outside_a_repository_it_is_refused() {
    let r = Repo::new();
    let (code, _, err) = run(&r, &r.dir, &["sweep", "--dry-run"], |_| {});
    assert_eq!(code, 2, "{err}");
    assert!(
        err.starts_with("✖ git rev-parse --show-toplevel: "),
        "{err}"
    );
}

#[test]
fn the_window_is_a_flag_or_an_environment_variable() {
    let r = Repo::new();
    let by_flag = r.detached("idle-an-hour");
    let by_variable = r.detached("touched-just-now");
    Repo::age(&by_flag, 1.0);
    let (_, out, _) = run(&r, &r.root, &["sweep"], |_| {});
    r.assert_kept(&by_flag, &out);
    let (code, out, err) = run(&r, &r.root, &["sweep", "--recent-hours", "0.5"], |_| {});
    assert_eq!(code, 0, "{err}");
    r.assert_removed(&by_flag, &out);
    // Touched within the flag's half hour, so that pass kept it; an hour idle, the variable's
    // half hour lets it go.
    r.assert_kept(&by_variable, &out);
    Repo::age(&by_variable, 1.0);
    let (code, out, err) = run(&r, &r.root, &["sweep"], |vars| {
        vars.insert(RECENT_ENV.into(), "0.5".into());
    });
    assert_eq!(code, 0, "{err}");
    r.assert_removed(&by_variable, &out);
}

#[test]
fn a_window_that_is_not_a_number_of_hours_is_refused() {
    let r = Repo::new();
    let wt = r.detached("idle");
    r.idle(&[]);
    for (window, said) in [
        (
            "two hours",
            "SYMPHONY_SWEEP_RECENT_HOURS=\"two hours\" is not a number of hours",
        ),
        (
            "-1",
            "the window of recent activity must be 0 hours or more, not -1",
        ),
    ] {
        let (code, out, err) = run(&r, &r.root, &["sweep"], |vars| {
            vars.insert(RECENT_ENV.into(), window.into());
        });
        assert_eq!(code, 2, "{out}");
        assert!(err.starts_with(&format!("✖ {said}")), "{err}");
        assert!(wt.is_dir());
    }
    // The sweep work finishing started says when and why before its refusal too.
    let (code, _, err) = run(&r, &r.root, &["sweep", "--background", "hourly"], |vars| {
        vars.insert(RECENT_ENV.into(), "soon".into());
    });
    assert_eq!(code, 2);
    assert!(
        err.starts_with("✖ [") && err.contains("] hourly: SYMPHONY_SWEEP_RECENT_HOURS="),
        "{err}"
    );
    assert!(wt.is_dir());
}

#[test]
fn a_flag_that_is_not_a_number_is_a_usage_error() {
    let r = Repo::new();
    let (code, _, err) = run(&r, &r.root, &["sweep", "--recent-hours", "soon"], |_| {});
    assert_eq!(code, 2);
    assert!(
        err.contains("argument --recent-hours: invalid float value: 'soon'"),
        "{err}"
    );
}

#[test]
fn the_tasks_named_let_their_closed_sessions_worktrees_go_at_once() {
    let r = Repo::with(
        Seed::default()
            .task("sim/jump", "done")
            .task("sim/dock", "cancelled"),
    );
    let mut trees = Vec::new();
    for slug in ["sim/jump", "sim/dock"] {
        let wt = r.merged(&slug.replace('/', "-"), slug);
        let handle = r.bind(&wt);
        r.board
            .store
            .record_launch(&NewLaunch {
                worktree: wt.to_string_lossy().into_owned(),
                ..NewLaunch::new(slug, &handle, "executor")
            })
            .unwrap();
        r.board
            .store
            .close_task_session(slug, &handle, "done", "just tasks restore")
            .unwrap();
        trees.push(wt);
    }
    let other = r.detached("unrelated");
    // Touched just now and bound to live sessions: without the tasks, the window keeps them, and
    // the worktree of no task with them.
    let (_, out, _) = run(&r, &r.root, &["sweep"], |_| {});
    for wt in trees.iter().chain([&other]) {
        r.assert_kept(wt, &out);
    }
    // The tasks named visit only their worktrees, even while a full pass is held: the third, which
    // a full pass would now remove, stays.
    Repo::age(&other, crate::IDLE_HOURS);
    let held = symphony_board::sweep::trigger::hold(&r.root.join(".git"))
        .unwrap()
        .unwrap();
    let args = ["sweep", "--task", "sim/jump", "--task", "sim/dock"];
    let (code, out, err) = run(&r, &r.root, &args, |_| {});
    assert_eq!(code, 0, "{out}{err}");
    for wt in &trees {
        r.assert_removed(wt, &out);
    }
    r.assert_kept(&other, &out);
    drop(held);
}

#[test]
fn a_task_it_cannot_find_is_refused() {
    let r = Repo::new();
    let (code, _, err) = run(&r, &r.root, &["sweep", "--task", "sim/nowhere"], |_| {});
    assert_eq!(code, 2);
    assert!(
        err.starts_with("✖ ") && err.contains("sim/nowhere"),
        "{err}"
    );
}

#[test]
fn a_sweep_work_finishing_started_never_starts_the_boards_server() {
    let r = Repo::new();
    let (code, _, err) = run(&r, &r.root, &["sweep", "--background", "hourly"], |vars| {
        vars.remove("TASKS_SERVER");
        vars.remove("TASKS_DATABASE");
    });
    assert_eq!(code, 2);
    assert!(err.starts_with("✖ no server of the board in "), "{err}");
    let home = folder(&r.root.join(".git"));
    assert!(
        !home.join("server.json").exists(),
        "no board server was started"
    );
}

#[test]
fn a_busy_trigger_returns_without_opening_the_board_connection() {
    let r = Repo::new();
    // Held by a commit main does not have, which keeps it before any pass scans the processes.
    let wt = r.detached("kept");
    r.commit(&wt, "extends RefCounted\n# keep this commit\n");
    let request = symphony_board::sweep::Request {
        hours: Some(0.0),
        background: Some("first"),
        ..Default::default()
    };
    // The connector runs after taking the lock and before reading the board. All ten callers
    // must return while it holds that lock, without trying their invalid board destination.
    let lines = symphony_board::sweep::command_connect(&r.env(&r.root), &request, || {
        for _ in 0..10 {
            let (code, out, err) = run(&r, &r.root, &["sweep", "--background", "busy"], |vars| {
                vars.insert("TASKS_SERVER".into(), "not-a-valid-address".into());
            });
            assert_eq!(code, 0, "{out}{err}");
            assert!(out.contains("another pass requested"), "{out}");
        }
        Ok(&r.board.store)
    })
    .unwrap();
    assert_eq!(
        lines
            .iter()
            .filter(|line| line.starts_with("worktree sweep: "))
            .count(),
        2,
        "{lines:?}"
    );
    r.assert_kept(&wt, &lines.join("\n"));
}
