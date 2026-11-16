//! Work finishing starts a sweep by itself (src/sweep/trigger.rs): the command that finished the work
//! returns at once, and the main checkout's build of `tasks` sweeps, detached, from the main
//! checkout, into `symphony/tasks/sweep.log`. A test's board starts none unless told
//! (`TASKS_SWEEP=on`); hc is the crate's `fake-hc`, holding no terminals. The build is this test's
//! (`TASKS_BIN`), but in the tests that drop it: there the main checkout's `.target/bin/tasks` links
//! a stand-in, or `PATH` holds one, or there is none.
//!
//! A sweep that starts is waited for in its log. One that must not start is checked at once and
//! for certain: a sweep is started only after its log is opened (and the hourly one after its stamp is
//! written), both before the command returns, so a command that started none left neither.

use crate::common::Seed;
use crate::{IDLE_HOURS, Repo, write};
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;
use symphony_board::NewTask;
use symphony_board::cli::Env;
use symphony_board::sweep::trigger::{HOURLY_SECONDS, LOG, STAMP, folder};

/// `tasks ARGS` as `handle`, from `cwd`, the sweep let start on this board, with `change` made to
/// the environment: what it printed (it must succeed).
fn run(
    r: &Repo,
    handle: &str,
    cwd: &Path,
    args: &[&str],
    change: impl FnOnce(&mut BTreeMap<String, String>),
) -> String {
    let mut vars = r.vars();
    let hc = r.dir.join("hc.json");
    if !hc.exists() {
        fs::write(&hc, r#"{"terminals": [], "layout": {"spaces": []}}"#).unwrap();
    }
    let state = hc.to_string_lossy().into_owned();
    for (key, value) in [
        ("TASKS_SWEEP", "on"),
        ("TASKS_BIN", env!("CARGO_BIN_EXE_tasks")),
        ("HC_BIN", env!("CARGO_BIN_EXE_fake-hc")),
        ("FAKE_HC_STATE", state.as_str()),
        ("TASKS_SESSION", handle),
    ] {
        vars.insert(key.into(), value.into());
    }
    change(&mut vars);
    let env = Env {
        vars,
        cwd: cwd.to_path_buf(),
    };
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = symphony_board::cli::main(&args, &env, &mut "".as_bytes(), &mut out, &mut err);
    let (out, err) = (
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    );
    assert_eq!(code, 0, "{args:?}: {out}{err}");
    out
}

fn home(r: &Repo) -> PathBuf {
    folder(&r.root.join(".git"))
}

fn log(r: &Repo) -> PathBuf {
    home(r).join(LOG)
}

/// The log once `count` of its lines start with `start`, waited for up to the backstop of the
/// minute it once had.
fn logged(r: &Repo, start: &str, count: usize) -> String {
    let read = || std::fs::read_to_string(log(r)).unwrap_or_default();
    let found = test_time::until(
        format!("{count} {start:?} lines in the log"),
        test_time::backstop(Duration::from_secs(60)),
        || {
            Some(read())
                .filter(|text| text.lines().filter(|l| l.starts_with(start)).count() >= count)
        },
    );
    found.unwrap_or_else(|stall| panic!("{stall}; the log:\n{}", read()))
}

/// The log once it holds `count` sweeps' summaries.
fn swept(r: &Repo, count: usize) -> String {
    logged(r, "worktree sweep: ", count)
}

/// An executable shell script at `path` under `dir`, running `body`.
fn script(dir: &Path, path: &str, body: &str) -> PathBuf {
    write(dir, path, &format!("#!/bin/sh\n{body}\n"));
    let file = dir.join(path);
    fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).unwrap();
    file
}

/// `checkout`'s `.target/bin/tasks`, linking `build` as its build of `tasks`, as the project's
/// build setup links it.
fn links_its_build(checkout: &Path, build: &Path) {
    let link = checkout.join(".target/bin/tasks");
    fs::create_dir_all(link.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(build, link).unwrap();
}

/// The log says a sweep started for `why`, from the main checkout.
fn started_for(text: &str, why: &str) {
    assert!(
        text.lines()
            .any(|l| l.starts_with('[') && l.ends_with(&format!("] {why}"))),
        "no sweep for {why:?}:\n{text}"
    );
}

/// No sweep started: none opened the log or wrote the stamp.
fn none_started(r: &Repo) {
    assert!(!log(r).exists(), "{:?}", std::fs::read_to_string(log(r)));
    assert!(!home(r).join(STAMP).exists());
}

/// A worktree idle past the window, at main, that nothing keeps.
fn idle_worktree(r: &Repo, name: &str) -> PathBuf {
    let wt = r.detached(name);
    Repo::age(&wt, IDLE_HOURS);
    wt
}

#[test]
fn a_subtask_done_starts_a_sweep_in_which_its_closed_sessions_worktree_goes_at_once() {
    let r = Repo::with(Seed::default().task("sim/jump", "in_progress"));
    r.board
        .store
        .create(&NewTask {
            parent: Some("sim/jump".into()),
            instruction: true,
            ..NewTask::new("sim/jump/do", "Do", "owner")
        })
        .unwrap();
    // Touched just now, its executor live and bound to it: only the finish lets it go.
    let wt = r.detached("do");
    let handle = r.bind(&wt);
    r.board.store.claim("sim/jump/do", &handle).unwrap();
    r.board
        .store
        .record_launch(&symphony_board::NewLaunch {
            worktree: wt.to_string_lossy().into_owned(),
            ..symphony_board::NewLaunch::new("sim/jump/do", &handle, "executor")
        })
        .unwrap();
    r.board
        .store
        .close_task_session("sim/jump/do", &handle, "done", "just tasks restore")
        .unwrap();
    let out = run(&r, &handle, &wt, &["done", "sim/jump/do", "did it"], |_| {});
    assert_eq!(out, "sim/jump/do is done\n");
    let text = swept(&r, 1);
    started_for(&text, "sim/jump/do is done");
    r.assert_removed(&wt, &text);
}

#[test]
fn a_task_moved_to_cancelled_starts_one() {
    let r = Repo::with(
        Seed::default()
            .session("amber-heron")
            .task("sim/jump", "ready"),
    );
    let wt = idle_worktree(&r, "idle");
    run(
        &r,
        "amber-heron",
        &r.root,
        &["move", "sim/jump", "cancelled"],
        |_| {},
    );
    let text = swept(&r, 1);
    started_for(&text, "sim/jump is cancelled");
    r.assert_removed(&wt, &text);
}

#[test]
fn an_approving_verdict_starts_one() {
    let r = Repo::with(
        Seed::default()
            .session("amber-heron")
            .task("sim/jump", "review")
            .role("sim/jump", "amber-heron", "reviewer"),
    );
    let wt = idle_worktree(&r, "idle");
    let out = run(
        &r,
        "amber-heron",
        &r.root,
        &["verdict", "sim/jump", "approve", "fine"],
        |_| {},
    );
    assert_eq!(
        out,
        "sim/jump is approved: done on the board, not on main until it lands; `just land \
         sim/jump --as HANDLE` lands it\n"
    );
    let text = swept(&r, 1);
    started_for(&text, "sim/jump is done");
    r.assert_removed(&wt, &text);
}

#[test]
fn a_session_leaving_starts_one_from_the_main_checkout() {
    // It leaves from its own worktree, which then goes: the sweep did not run from there.
    let r = Repo::new();
    let wt = idle_worktree(&r, "mine");
    let handle = r.bind(&wt);
    Repo::age(&wt, IDLE_HOURS);
    run(&r, &handle, &wt, &["leave"], |_| {});
    let text = swept(&r, 1);
    started_for(&text, &format!("{handle} left"));
    r.assert_removed(&wt, &text);
}

#[test]
fn a_reap_starts_one_and_its_dry_run_none() {
    let r = Repo::with(Seed::default().session("amber-heron"));
    let wt = idle_worktree(&r, "idle");
    run(&r, "amber-heron", &r.root, &["reap", "--dry-run"], |_| {});
    none_started(&r);
    run(&r, "amber-heron", &r.root, &["reap"], |_| {});
    let text = swept(&r, 1);
    started_for(&text, "a reap");
    r.assert_removed(&wt, &text);
}

#[test]
fn inbox_starts_one_at_most_once_an_hour() {
    let r = Repo::with(Seed::default().session("amber-heron"));
    let wt = idle_worktree(&r, "idle");
    run(&r, "amber-heron", &r.root, &["inbox"], |_| {});
    let text = swept(&r, 1);
    started_for(&text, "hourly");
    r.assert_removed(&wt, &text);
    let stamp = home(&r).join(STAMP);
    let written = std::fs::read_to_string(&stamp).unwrap();
    // Within the hour: the stamp is read and left as it is, and nothing starts.
    run(&r, "amber-heron", &r.root, &["inbox"], |_| {});
    assert_eq!(std::fs::read_to_string(&stamp).unwrap(), written);
    // An hour on, the next inbox claims the stamp and starts the next.
    let then: f64 = written.parse().unwrap();
    std::fs::write(&stamp, (then - HOURLY_SECONDS - 1.0).to_string()).unwrap();
    let again = idle_worktree(&r, "idle-again");
    run(&r, "amber-heron", &r.root, &["inbox"], |_| {});
    assert_ne!(std::fs::read_to_string(&stamp).unwrap(), written);
    let text = swept(&r, 2);
    r.assert_removed(&again, &text);
}

#[test]
fn a_test_board_starts_none_unless_told_and_off_starts_none_anywhere() {
    for (sweep, unset_server) in [(None, false), (Some("off"), false), (Some("off"), true)] {
        let r = Repo::with(
            Seed::default()
                .session("amber-heron")
                .task("sim/jump", "ready"),
        );
        idle_worktree(&r, "idle");
        let change = |vars: &mut BTreeMap<String, String>| {
            match sweep {
                Some(value) => vars.insert("TASKS_SWEEP".into(), value.into()),
                None => vars.remove("TASKS_SWEEP"),
            };
            if unset_server {
                // The repository's own board: `off` still starts none (no server here, so the
                // command is not run; the rule is the trigger's own, checked directly).
                vars.remove("TASKS_SERVER");
            }
        };
        if unset_server {
            let mut vars = r.vars();
            change(&mut vars);
            let env = Env {
                vars,
                cwd: r.root.clone(),
            };
            assert!(!symphony_board::sweep::trigger::trigger(
                &env, &r.root, "x", None
            ));
            assert!(!symphony_board::sweep::trigger::hourly(&env, &r.root));
        } else {
            run(
                &r,
                "amber-heron",
                &r.root,
                &["move", "sim/jump", "cancelled"],
                change,
            );
            run(&r, "amber-heron", &r.root, &["inbox"], change);
            run(&r, "amber-heron", &r.root, &["leave"], change);
        }
        none_started(&r);
    }
}

#[test]
fn a_move_that_finishes_nothing_starts_none() {
    let r = Repo::with(
        Seed::default()
            .session("amber-heron")
            .task("sim/jump", "ready"),
    );
    idle_worktree(&r, "idle");
    run(
        &r,
        "amber-heron",
        &r.root,
        &["move", "sim/jump", "blocked", "--on", "parked"],
        |_| {},
    );
    none_started(&r);
}

#[test]
fn outside_a_repository_nothing_starts() {
    let r = Repo::new();
    let mut vars = r.vars();
    vars.insert("TASKS_SWEEP".into(), "on".into());
    let env = Env {
        vars,
        cwd: r.dir.clone(),
    };
    assert!(!symphony_board::sweep::trigger::trigger(
        &env, &r.dir, "x", None
    ));
    assert!(!symphony_board::sweep::trigger::hourly(&env, &r.dir));
    none_started(&r);
}

/// A stand-in build at `name` under the test's folder that says `who` ran it and then sweeps as this
/// test's build would.
fn sweeping_build(r: &Repo, name: &str, who: &str) -> PathBuf {
    script(
        &r.dir,
        name,
        &format!(
            "echo \"{who}: $*\"\nexec '{}' \"$@\"",
            env!("CARGO_BIN_EXE_tasks")
        ),
    )
}

/// The search path with the folder `first` before the test's own.
fn path_with(first: &Path) -> String {
    format!("{}:{}", first.display(), std::env::var("PATH").unwrap())
}

#[test]
fn a_trigger_from_a_worktree_runs_the_main_checkouts_link_not_its_own_nor_the_search_paths() {
    let r = Repo::with(
        Seed::default()
            .session("amber-heron")
            .task("sim/jump", "ready"),
    );
    let gone = idle_worktree(&r, "idle");
    let build = sweeping_build(&r, "root-tasks", "root build");
    links_its_build(&r.root, &build);
    // The worktree it is run from has a build of its own sources, which must not run, and so has
    // the search path.
    let mine = r.detached("mine");
    let own = script(
        &r.dir,
        "own-tasks",
        &format!("touch '{}'", r.dir.join("own-ran").display()),
    );
    links_its_build(&mine, &own);
    script(
        &r.dir,
        "path-bin/tasks",
        &format!("touch '{}'", r.dir.join("path-ran").display()),
    );
    let path = path_with(&r.dir.join("path-bin"));
    run(
        &r,
        "amber-heron",
        &mine,
        &["move", "sim/jump", "cancelled"],
        |vars| {
            vars.remove("TASKS_BIN");
            vars.insert("PATH".into(), path);
        },
    );
    let text = swept(&r, 1);
    assert!(
        text.lines()
            .any(|l| l == "root build: sweep --background=sim/jump is cancelled --task=sim/jump"),
        "{text}"
    );
    started_for(&text, "sim/jump is cancelled");
    r.assert_removed(&gone, &text);
    assert!(!r.dir.join("own-ran").exists(), "{text}");
    assert!(!r.dir.join("path-ran").exists(), "{text}");
}

#[test]
fn a_main_checkout_with_no_link_sweeps_with_the_build_on_the_search_path() {
    // A project that installed its builds rather than linking them.
    let r = Repo::with(
        Seed::default()
            .session("amber-heron")
            .task("sim/jump", "ready"),
    );
    let gone = idle_worktree(&r, "idle");
    sweeping_build(&r, "path-bin/tasks", "installed build");
    let path = path_with(&r.dir.join("path-bin"));
    run(
        &r,
        "amber-heron",
        &r.root,
        &["move", "sim/jump", "cancelled"],
        |vars| {
            vars.remove("TASKS_BIN");
            vars.insert("PATH".into(), path);
        },
    );
    let text = swept(&r, 1);
    assert!(
        text.lines()
            .any(|l| l
                == "installed build: sweep --background=sim/jump is cancelled --task=sim/jump"),
        "{text}"
    );
    r.assert_removed(&gone, &text);
}

#[test]
fn with_no_build_of_the_main_checkout_nothing_sweeps_and_the_log_says_where_one_goes() {
    // No link in the main checkout and none on the search path; a link to a build that is not there.
    for linked in [false, true] {
        let r = Repo::with(
            Seed::default()
                .session("amber-heron")
                .task("sim/jump", "ready"),
        );
        let idle = idle_worktree(&r, "idle");
        if linked {
            links_its_build(&r.root, &r.dir.join("never-built"));
        }
        run(
            &r,
            "amber-heron",
            &r.root,
            &["move", "sim/jump", "cancelled"],
            |vars| {
                vars.remove("TASKS_BIN");
                vars.insert("PATH".into(), "/usr/bin:/bin".into());
            },
        );
        let text = logged(&r, "no sweep: ", 1);
        started_for(&text, "sim/jump is cancelled");
        let said = format!(
            "no sweep: no build of tasks for {}: link the project's build at {}/.target/bin/tasks, \
             install a release on the machine or put tasks on PATH",
            r.root.display(),
            r.root.display()
        );
        assert!(text.lines().any(|l| l == said), "{said}\n{text}");
        assert!(!text.contains("worktree sweep: "), "{text}");
        r.assert_kept(&idle, &text);
    }
}
